use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static SEGMENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Archive {
    root: PathBuf,
    writer: zstd::stream::write::Encoder<'static, BufWriter<File>>,
    bytes: u64,
    max_bytes: u64,
    pending_cursor: Option<i64>,
    pending_events: u64,
    last_checkpoint: std::time::Instant,
}

impl Archive {
    pub fn open(root: impl AsRef<Path>, max_mib: u64) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Self::new_segment(root.as_ref().to_owned(), max_mib * 1024 * 1024)
    }
    fn new_segment(root: PathBuf, max_bytes: u64) -> Result<Self> {
        let name = format!(
            "events-{}-{}-{}.jsonl.zst",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%.6f"),
            std::process::id(),
            SEGMENT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        );
        let path = root.join(name);
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let writer = zstd::stream::write::Encoder::new(BufWriter::new(file), 3)?;
        Ok(Self {
            root,
            writer,
            bytes: 0,
            max_bytes,
            pending_cursor: None,
            pending_events: 0,
            last_checkpoint: std::time::Instant::now(),
        })
    }
    /// Append to the compressed stream and periodically group-commit it. A crash
    /// can replay at most one checkpoint interval; the durable cursor never gets
    /// ahead of durable archive bytes.
    pub fn append(&mut self, time_us: i64, raw: &[u8]) -> Result<bool> {
        // Jetstream messages are already JSON. Keeping them verbatim is both smaller and
        // makes replay independent of this application's current normalization schema.
        serde_json::from_slice::<serde_json::Value>(raw).context("archive payload is not JSON")?;
        self.writer.write_all(raw)?;
        self.writer.write_all(b"\n")?;
        self.bytes += raw.len() as u64 + 1;
        self.pending_cursor = Some(
            self.pending_cursor
                .map_or(time_us, |value| value.max(time_us)),
        );
        self.pending_events += 1;
        let checkpointed = if self.pending_events >= 5_000
            || self.last_checkpoint.elapsed() >= std::time::Duration::from_secs(1)
        {
            self.checkpoint()?;
            true
        } else {
            false
        };
        if self.bytes >= self.max_bytes {
            self.checkpoint()?;
            let replacement = Self::new_segment(self.root.clone(), self.max_bytes)?;
            let old = std::mem::replace(self, replacement);
            old.writer.finish()?.flush()?;
        }
        Ok(checkpointed)
    }
    pub fn checkpoint(&mut self) -> Result<()> {
        if self.pending_events == 0 {
            return Ok(());
        }
        self.writer.flush()?;
        self.writer.get_mut().get_ref().sync_data()?;
        if let Some(time_us) = self.pending_cursor {
            self.save_cursor(time_us)?;
        }
        self.pending_cursor = None;
        self.pending_events = 0;
        self.last_checkpoint = std::time::Instant::now();
        Ok(())
    }
    fn save_cursor(&self, time_us: i64) -> Result<()> {
        // Never move the resume point backwards if upstream messages arrive out of order.
        if Self::cursor(&self.root).is_some_and(|current| current >= time_us) {
            return Ok(());
        }
        let tmp = self.root.join("cursor.tmp");
        let mut cursor = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)?;
        cursor.write_all(time_us.to_string().as_bytes())?;
        cursor.sync_all()?;
        fs::rename(tmp, self.root.join("cursor"))?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
    pub fn cursor(root: impl AsRef<Path>) -> Option<i64> {
        fs::read_to_string(root.as_ref().join("cursor"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
    pub fn total_size(root: impl AsRef<Path>) -> u64 {
        fs::read_dir(root)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.metadata().ok().map(|m| m.len()))
            .sum()
    }
    pub fn prune(root: impl AsRef<Path>, retention_days: u64) -> Result<()> {
        let cutoff =
            std::time::SystemTime::now() - std::time::Duration::from_secs(retention_days * 86400);
        for e in fs::read_dir(root).context("read archive")? {
            let e = e?;
            if e.path().extension().and_then(|s| s.to_str()) == Some("zst")
                && e.metadata()?.modified()? < cutoff
            {
                fs::remove_file(e.path())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_is_compressed_and_cursor_safe() {
        let d = tempfile::tempdir().unwrap();
        let mut a = Archive::open(d.path(), 1).unwrap();
        a.append(42, br#"{"kind":"test"}"#).unwrap();
        a.checkpoint().unwrap();
        assert_eq!(Archive::cursor(d.path()), Some(42));
        assert!(Archive::total_size(d.path()) > 0);
    }

    #[test]
    fn cursor_never_regresses_and_segments_are_unique() {
        let d = tempfile::tempdir().unwrap();
        let mut a = Archive::open(d.path(), 1).unwrap();
        a.append(42, br#"{"kind":"test","time_us":42}"#).unwrap();
        a.append(41, br#"{"kind":"test","time_us":41}"#).unwrap();
        a.checkpoint().unwrap();
        assert_eq!(Archive::cursor(d.path()), Some(42));
        let _second = Archive::open(d.path(), 1).unwrap();
    }
}
