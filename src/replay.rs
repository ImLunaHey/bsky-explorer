use crate::model::NormalizedEvent;
use crate::normalize::{jetstream_time_us, parse_jetstream};
use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct ReplayBounds {
    pub since_time_us: Option<i64>,
    pub until_time_us: Option<i64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReplayStats {
    pub raw_events: u64,
    pub normalized_events: u64,
    pub unsupported_events: u64,
    pub outside_time_range: u64,
}

/// Visits normalized events from a stable, lexically ordered snapshot of archive segments.
/// This reader deliberately has no access to the live resume cursor.
pub fn visit_archive(
    root: &Path,
    bounds: ReplayBounds,
    mut visit: impl FnMut(NormalizedEvent) -> Result<()>,
) -> Result<ReplayStats> {
    if let (Some(since), Some(until)) = (bounds.since_time_us, bounds.until_time_us)
        && since > until
    {
        bail!("since-time-us must be less than or equal to until-time-us");
    }
    let paths = segment_paths(root)?;
    if paths.is_empty() {
        bail!("no .jsonl.zst archive segments found in {}", root.display());
    }

    let mut stats = ReplayStats::default();
    for path in paths {
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let decoder = zstd::stream::read::Decoder::new(file)
            .with_context(|| format!("decode {}", path.display()))?;
        for (line_index, line) in BufReader::new(decoder).split(b'\n').enumerate() {
            let raw =
                line.with_context(|| format!("read {} line {}", path.display(), line_index + 1))?;
            if raw.is_empty() {
                continue;
            }
            stats.raw_events += 1;
            let time_us = jetstream_time_us(&raw)
                .with_context(|| format!("parse {} line {}", path.display(), line_index + 1))?;
            if bounds.since_time_us.is_some_and(|since| time_us < since)
                || bounds.until_time_us.is_some_and(|until| time_us > until)
            {
                stats.outside_time_range += 1;
                continue;
            }
            match parse_jetstream(&raw)
                .with_context(|| format!("normalize {} line {}", path.display(), line_index + 1))?
            {
                Some(event) => {
                    visit(event)?;
                    stats.normalized_events += 1;
                }
                None => stats.unsupported_events += 1,
            }
        }
    }
    Ok(stats)
}

fn segment_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("events-") && name.ends_with(".jsonl.zst"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_segment(path: &Path, lines: &[&[u8]]) {
        let file = File::create(path).unwrap();
        let mut encoder = zstd::stream::write::Encoder::new(file, 1).unwrap();
        for line in lines {
            encoder.write_all(line).unwrap();
            encoder.write_all(b"\n").unwrap();
        }
        encoder.finish().unwrap();
    }

    #[test]
    fn replays_in_order_with_bounds_without_touching_cursor() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("cursor"), "999").unwrap();
        let post = |time, rkey| {
            format!(
                r#"{{"did":"did:plc:a","time_us":{time},"kind":"commit","commit":{{"operation":"create","collection":"app.bsky.feed.post","rkey":"{rkey}","record":{{"text":"hello"}}}}}}"#
            )
        };
        let late = post(20, "late");
        let early = post(10, "early");
        write_segment(&dir.path().join("events-b.jsonl.zst"), &[late.as_bytes()]);
        write_segment(&dir.path().join("events-a.jsonl.zst"), &[early.as_bytes()]);
        let mut keys = Vec::new();
        let stats = visit_archive(
            dir.path(),
            ReplayBounds {
                since_time_us: Some(10),
                until_time_us: Some(15),
            },
            |event| {
                keys.push(event.rkey);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(keys, ["early"]);
        assert_eq!(stats.raw_events, 2);
        assert_eq!(stats.normalized_events, 1);
        assert_eq!(stats.outside_time_range, 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("cursor")).unwrap(),
            "999"
        );
    }

    #[test]
    fn rejects_invalid_bounds_and_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        write_segment(&dir.path().join("events-a.jsonl.zst"), &[b"not-json"]);
        assert!(visit_archive(dir.path(), ReplayBounds::default(), |_| Ok(())).is_err());
        assert!(
            visit_archive(
                dir.path(),
                ReplayBounds {
                    since_time_us: Some(2),
                    until_time_us: Some(1)
                },
                |_| Ok(())
            )
            .is_err()
        );
    }
}
