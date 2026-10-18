//! Music charts are materialized off the HTTP path. Only play/scrobble records
//! count, never catalog songs, now-playing statuses, or arrival timestamps.
use super::{Storage, snapshots::RepositoryRecord};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const COLLECTIONS: [&str; 3] = [
    "fm.teal.feed.play",
    "fm.teal.alpha.feed.play",
    "app.rocksky.scrobble",
];
const MAX_RECORDS: usize = 500_000;

#[derive(Debug, Clone)]
struct Play {
    title: String,
    artists: Vec<String>,
    album: String,
    artwork_url: Option<String>,
    listen_url: Option<String>,
    at: i64,
    actor: String,
    uri: String,
    source: String,
}
fn clean(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or("")
        .trim()
        .chars()
        .take(512)
        .collect()
}
fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn artwork(value: &Value) -> Option<String> {
    ["albumArtUrl", "albumCoverUrl", "artworkUrl"]
        .iter()
        .find_map(|key| {
            let raw = value[*key].as_str()?;
            if raw.len() > 2048 {
                return None;
            }
            let url = url::Url::parse(raw).ok()?;
            (url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none())
            .then(|| url.to_string())
        })
}
fn parse_play(record: &RepositoryRecord) -> Option<Play> {
    if record.operation == "delete" || !COLLECTIONS.contains(&record.collection.as_str()) {
        return None;
    }
    let value: Value = serde_json::from_str(&record.record_json).ok()?;
    let rocksky = record.collection == "app.rocksky.scrobble";
    let title = clean(&value, if rocksky { "title" } else { "trackName" });
    let artists = if rocksky {
        vec![clean(&value, "artist")]
    } else {
        value["artists"]
            .as_array()?
            .iter()
            .take(30)
            .map(|a| clean(a, "artistName"))
            .collect()
    };
    let artists: Vec<String> = artists.into_iter().filter(|s| !s.is_empty()).collect();
    let at = chrono::DateTime::parse_from_rfc3339(
        value[if rocksky { "createdAt" } else { "playedTime" }].as_str()?,
    )
    .ok()?
    .timestamp();
    if title.is_empty() || artists.is_empty() {
        return None;
    }
    Some(Play {
        title,
        artists,
        album: clean(&value, if rocksky { "album" } else { "releaseName" }),
        artwork_url: artwork(&value),
        listen_url: ["originUri", "originUrl", "spotifyLink"]
            .iter()
            .find_map(|key| {
                let raw = value[*key].as_str()?;
                let url = url::Url::parse(raw).ok()?;
                (raw.len() <= 2048
                    && url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none())
                .then(|| url.to_string())
            }),
        at,
        actor: record.actor_did.clone(),
        uri: record.at_uri.clone(),
        source: record.collection.clone(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Ranking {
    pub id: String,
    pub listen_url: Option<String>,
    pub artwork_url: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub plays: u64,
    pub listeners: usize,
    pub example_uri: String,
    pub sources: BTreeSet<String>,
}
#[derive(Default)]
struct Counter {
    listen_url: Option<String>,
    artwork_url: Option<String>,
    title: String,
    subtitle: String,
    plays: u64,
    listeners: HashSet<String>,
    example_uri: String,
    sources: BTreeSet<String>,
}
impl Counter {
    fn add(&mut self, play: &Play, title: &str, subtitle: &str) {
        self.title = title.to_owned();
        self.subtitle = subtitle.to_owned();
        self.plays += 1;
        self.listeners.insert(play.actor.clone());
        self.example_uri = play.uri.clone();
        self.sources.insert(play.source.clone());
        if play.listen_url.is_some() {
            self.listen_url = play.listen_url.clone();
        }
        if play.artwork_url.is_some() {
            self.artwork_url = play.artwork_url.clone();
        }
    }
}
fn entity_id(kind: &str, title: &str, subtitle: &str) -> String {
    format!(
        "{kind}-{:016x}",
        xxhash_rust::xxh3::xxh3_64(
            serde_json::to_string(&(normalized(title), normalized(subtitle)))
                .unwrap()
                .as_bytes()
        )
    )
}
fn ranked(counts: BTreeMap<String, Counter>, kind: &str) -> (Vec<Ranking>, Vec<Ranking>) {
    let mut rows: Vec<_> = counts
        .into_values()
        .map(|r| Ranking {
            id: entity_id(kind, &r.title, &r.subtitle),
            listen_url: r.listen_url,
            artwork_url: r.artwork_url,
            title: r.title,
            subtitle: r.subtitle,
            plays: r.plays,
            listeners: r.listeners.len(),
            example_uri: r.example_uri,
            sources: r.sources,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.plays
            .cmp(&a.plays)
            .then(b.listeners.cmp(&a.listeners))
            .then(a.title.cmp(&b.title))
            .then(a.subtitle.cmp(&b.subtitle))
    });
    let by_plays = rows.iter().take(30).cloned().collect();
    rows.sort_by(|a, b| {
        b.listeners
            .cmp(&a.listeners)
            .then(b.plays.cmp(&a.plays))
            .then(a.id.cmp(&b.id))
    });
    rows.truncate(30);
    (by_plays, rows)
}
#[derive(Debug, Serialize)]
pub struct MusicSnapshot {
    pub start: i64,
    pub end: i64,
    pub observed_since: i64,
    pub generated_at: i64,
    pub plays: usize,
    pub listeners: usize,
    pub tracks: Vec<Ranking>,
    pub artists: Vec<Ranking>,
    pub albums: Vec<Ranking>,
    pub tracks_by_listeners: Vec<Ranking>,
    pub artists_by_listeners: Vec<Ranking>,
    pub albums_by_listeners: Vec<Ranking>,
    pub source_records: BTreeMap<String, usize>,
    pub duplicate_plays: usize,
    pub unparseable_records: usize,
    pub scanned_records: usize,
    pub activity: Vec<(i64, usize)>,
}

fn aggregate(
    plays: &[Play],
    end: i64,
    seconds: i64,
    unparseable: usize,
    scanned: usize,
) -> MusicSnapshot {
    let start = end - seconds;
    let mut tracks: BTreeMap<String, Counter> = BTreeMap::new();
    let mut artists: BTreeMap<String, Counter> = BTreeMap::new();
    let mut albums: BTreeMap<String, Counter> = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut listeners = HashSet::new();
    let mut source_records = BTreeMap::new();
    let mut duplicate_plays = 0;
    let mut activity = BTreeMap::new();
    for play in plays.iter().filter(|p| p.at >= start && p.at < end) {
        *source_records.entry(play.source.clone()).or_insert(0) += 1;
        let credit = play.artists.join(" · ");
        // JSON tuple avoids delimiter collisions in user-controlled metadata.
        let track_key =
            serde_json::to_string(&(normalized(&play.title), normalized(&credit))).unwrap();
        if !seen.insert((play.actor.clone(), play.at, track_key.clone())) {
            duplicate_plays += 1;
            if let Some(track) = tracks.get_mut(&track_key) {
                track.sources.insert(play.source.clone());
                if play.artwork_url.is_some() {
                    track.artwork_url = play.artwork_url.clone();
                }
            }
            if let Some(album) = albums.get_mut(
                &serde_json::to_string(&(normalized(&play.album), normalized(&credit))).unwrap(),
            ) {
                if play.artwork_url.is_some() {
                    album.artwork_url = play.artwork_url.clone();
                }
            }
            continue;
        }
        listeners.insert(play.actor.clone());
        *activity
            .entry(start + (play.at - start) / (seconds / 24) * (seconds / 24))
            .or_insert(0) += 1;
        tracks
            .entry(track_key)
            .or_default()
            .add(play, &play.title, &credit);
        let mut credited = HashSet::new();
        for artist in &play.artists {
            if credited.insert(normalized(artist)) {
                artists
                    .entry(normalized(artist))
                    .or_default()
                    .add(play, artist, "Credited artist");
            }
        }
        if !play.album.is_empty() {
            let key =
                serde_json::to_string(&(normalized(&play.album), normalized(&credit))).unwrap();
            albums
                .entry(key)
                .or_default()
                .add(play, &play.album, &credit);
        }
    }
    let (tracks, tracks_by_listeners) = ranked(tracks, "song");
    let (artists, artists_by_listeners) = ranked(artists, "artist");
    let (albums, albums_by_listeners) = ranked(albums, "album");
    MusicSnapshot {
        start,
        end,
        observed_since: end - 604800,
        generated_at: chrono::Utc::now().timestamp(),
        plays: seen.len(),
        listeners: listeners.len(),
        tracks,
        artists,
        albums,
        tracks_by_listeners,
        artists_by_listeners,
        albums_by_listeners,
        source_records,
        duplicate_plays,
        unparseable_records: unparseable,
        scanned_records: scanned,
        activity: (0..24)
            .map(|i| {
                let at = start + i * (seconds / 24);
                (at, *activity.get(&at).unwrap_or(&0))
            })
            .collect(),
    }
}

impl Storage {
    pub async fn refresh_music(&self) -> Result<()> {
        let _permit = super::BACKGROUND_QUERY.acquire().await?;
        let end = chrono::Utc::now().timestamp();
        // Deduplicate event redelivery and record updates BEFORE reading payloads.
        // A bounded observation window means this is not a repository-state mirror.
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "90")
            .with_option("max_threads", "1")
            .with_option("max_memory_usage", "750000000")
            .with_option("max_bytes_before_external_group_by", "128000000");
        let mut cursor = client.query("SELECT at_uri, tupleElement(latest,1) AS actor_did, tupleElement(latest,2) AS collection, tupleElement(latest,3) AS operation, tupleElement(latest,4) AS timestamp_ms, tupleElement(latest,5) AS record_json FROM (SELECT at_uri, argMax(tuple(actor_did,collection,operation,toUnixTimestamp64Milli(timestamp),record_json),tuple(time_us,event_id)) AS latest FROM events WHERE collection IN ? AND timestamp >= fromUnixTimestamp(?) AND timestamp < fromUnixTimestamp(?) GROUP BY at_uri) ORDER BY at_uri LIMIT 500001")
            .bind(COLLECTIONS.as_slice()).bind(end - 604800).bind(end).fetch::<RepositoryRecord>()?;
        let mut plays = Vec::new();
        let mut scanned = 0;
        let mut unparseable = 0;
        while let Some(row) = cursor.next().await? {
            scanned += 1;
            ensure!(
                scanned <= MAX_RECORDS,
                "Music record budget exceeded; retaining previous complete charts"
            );
            if row.operation == "delete" {
                continue;
            }
            match parse_play(&row) {
                Some(play) => plays.push(play),
                None => unparseable += 1,
            }
        }
        let mut seeds = BTreeMap::new();
        let mut snapshots = Vec::new();
        for seconds in [3600, 86400, 604800] {
            let snapshot = aggregate(&plays, end, seconds, unparseable, scanned);
            for rows in [
                &snapshot.tracks,
                &snapshot.artists,
                &snapshot.albums,
                &snapshot.tracks_by_listeners,
                &snapshot.artists_by_listeners,
                &snapshot.albums_by_listeners,
            ] {
                for row in rows {
                    seeds.insert(row.id.clone(), row.clone());
                }
            }
            snapshots.push((seconds, snapshot));
        }
        let mut groups: BTreeMap<String, Vec<Play>> = BTreeMap::new();
        for play in plays.iter().filter(|p| p.at >= end - 604800 && p.at < end) {
            let credit = play.artists.join(" · ");
            let mut ids = BTreeSet::from([
                entity_id("song", &play.title, &credit),
                entity_id("album", &play.album, &credit),
            ]);
            for artist in &play.artists {
                ids.insert(entity_id("artist", artist, "Credited artist"));
            }
            for id in ids {
                if seeds.contains_key(&id) {
                    groups.entry(id).or_default().push(play.clone());
                }
            }
        }
        // Keep recently shared detail URLs useful when they drop out of a chart.
        // Old entries retain their timestamp and are visibly stale, never relabeled live.
        let mut entities: serde_json::Map<String, Value> = self
            .snapshot("music-entities")
            .await?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, v)| {
                v["generated_at"]
                    .as_i64()
                    .is_some_and(|at| at >= end - 604800)
            })
            .take(1000)
            .collect();
        for (id, row) in seeds {
            let matching = groups.remove(&id).unwrap_or_default();
            let mut windows = serde_json::Map::new();
            for (label, seconds) in [("1h", 3600), ("24h", 86400), ("7d", 604800)] {
                let summary = aggregate(&matching, end, seconds, 0, matching.len());
                windows.insert(label.into(), serde_json::json!({"plays":summary.plays,"listeners":summary.listeners,"activity":summary.activity,"start":summary.start,"end":summary.end,"songs":summary.tracks_by_listeners}));
            }
            let albums: BTreeSet<_> = matching
                .iter()
                .filter(|p| !p.album.is_empty())
                .map(|p| p.album.clone())
                .collect();
            let links: BTreeSet<_> = matching
                .iter()
                .filter_map(|p| p.listen_url.clone())
                .collect();
            entities.insert(id.clone(), serde_json::json!({"id":id,"kind":id.split('-').next(),"summary":row,"albums":albums.into_iter().take(12).collect::<Vec<_>>(),"links":links.into_iter().take(5).collect::<Vec<_>>(),"windows":windows,"generated_at":end}));
        }
        // Publish detail destinations before rankings that link to them.
        self.save_snapshot("music-entities", &Value::Object(entities).to_string())
            .await?;
        for (seconds, snapshot) in snapshots {
            self.save_snapshot(
                &format!("music:{seconds}"),
                &serde_json::to_string(&snapshot)?,
            )
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listener_ranking_considers_all_candidates_not_only_top_play_counts() {
        let mut counts = BTreeMap::new();
        for index in 0..35 {
            counts.insert(
                index.to_string(),
                Counter {
                    title: format!("Song {index}"),
                    plays: 1000 - index,
                    listeners: HashSet::from(["one".into()]),
                    ..Default::default()
                },
            );
        }
        counts.insert(
            "broad".into(),
            Counter {
                title: "Broad appeal".into(),
                plays: 3,
                listeners: HashSet::from(["one".into(), "two".into(), "three".into()]),
                ..Default::default()
            },
        );
        let (plays, listeners) = ranked(counts, "song");
        assert!(!plays.iter().any(|r| r.title == "Broad appeal"));
        assert_eq!(listeners[0].title, "Broad appeal");
        assert_eq!(
            entity_id("song", " SONG ", "Artist"),
            entity_id("song", "song", "artist")
        );
        assert_ne!(
            entity_id("song", "Song", "Artist"),
            entity_id("album", "Song", "Artist")
        );
    }
    #[test]
    fn artwork_survives_deduplication_without_changing_counts() {
        let first = parse_play(&teal()).unwrap();
        let end = first.at + 3600;
        let mut duplicate = parse_play(&teal()).unwrap();
        duplicate.artwork_url = Some("https://i.scdn.co/image/album".into());
        let data = aggregate(&[first, duplicate], end, 3600, 0, 2);
        assert_eq!(data.plays, 1);
        assert_eq!(
            data.tracks[0].artwork_url.as_deref(),
            Some("https://i.scdn.co/image/album")
        );
        assert_eq!(data.albums[0].artwork_url, data.tracks[0].artwork_url);
        assert_eq!(
            artwork(&serde_json::json!({"albumArtUrl":"javascript:alert(1)"})),
            None
        );
        assert_eq!(
            artwork(&serde_json::json!({"albumArtUrl":"https://user:pass@i.scdn.co/a"})),
            None
        );
        assert_eq!(
            artwork(&serde_json::json!({"albumArtUrl":"https://i.scdn.co/image/album"})),
            data.tracks[0].artwork_url
        );
    }
    #[tokio::test]
    #[ignore = "requires an isolated ClickHouse at CLICKHOUSE_TEST_URL"]
    async fn materializer_deduplicates_updates_deletes_and_persists_charts() {
        let url = std::env::var("CLICKHOUSE_TEST_URL").expect("isolated test server URL required");
        let db = format!("music_test_{}", std::process::id());
        let admin = clickhouse::Client::default().with_url(&url);
        for sql in include_str!("../../migrations/001_init.sql")
            .replace("observatory", &db)
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            admin.query(sql).execute().await.unwrap();
        }
        let storage = Storage::new(&url, &db, "default", "");
        storage.client.query("CREATE TABLE explorer_snapshots (key String, generated_at DateTime64(3), payload String) ENGINE=MergeTree ORDER BY (key,generated_at)").execute().await.unwrap();
        let at = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        let payload = serde_json::json!({"trackName":"Song","artists":[{"artistName":"Artist"}],"playedTime":at}).to_string();
        for (uri, version, operation, body) in [
            ("one", 1, "create", payload.as_str()),
            ("one", 1, "create", payload.as_str()),
            ("one", 2, "update", payload.as_str()),
            ("deleted", 1, "create", payload.as_str()),
            ("deleted", 2, "delete", ""),
            ("invalid", 1, "create", "{}"),
        ] {
            storage.client.query("INSERT INTO events (at_uri,event_id,time_us,timestamp,actor_did,collection,operation,record_json) VALUES (?,?,?,now64(3)-INTERVAL 1 SECOND,'did:plc:test','fm.teal.feed.play',?,?)")
                .bind(format!("at://did:plc:test/fm.teal.feed.play/{uri}")).bind(format!("{uri}-{version}")).bind(version).bind(operation).bind(body).execute().await.unwrap();
        }
        storage.refresh_music().await.unwrap();
        for seconds in [3600, 86400, 604800] {
            let value: Value = serde_json::from_str(
                &storage
                    .snapshot(&format!("music:{seconds}"))
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(value["plays"], 1);
            assert_eq!(value["scanned_records"], 3);
            assert_eq!(value["unparseable_records"], 1);
            assert_eq!(value["tracks"][0]["title"], "Song");
        }
        admin
            .query(&format!("DROP DATABASE {db}"))
            .execute()
            .await
            .unwrap();
    }
    fn record(collection: &str, payload: Value) -> RepositoryRecord {
        RepositoryRecord {
            at_uri: "at://did:plc:test/fm.teal.feed.play/one".into(),
            actor_did: "did:plc:test".into(),
            collection: collection.into(),
            operation: "create".into(),
            timestamp_ms: 999999999,
            record_json: payload.to_string(),
        }
    }
    fn teal() -> RepositoryRecord {
        record(
            COLLECTIONS[0],
            serde_json::json!({"trackName":" Song ","artists":[{"artistName":"Artist"}],"releaseName":"Album","playedTime":"2026-10-18T10:00:00Z"}),
        )
    }
    #[test]
    fn uses_play_time_not_import_time_and_rejects_invalid_records() {
        let mut row = teal();
        let play = parse_play(&row).unwrap();
        assert_eq!(play.title, "Song");
        assert_eq!(play.at, 1792317600);
        row.operation = "delete".into();
        assert!(parse_play(&row).is_none());
        row.operation = "create".into();
        row.collection = "app.rocksky.song".into();
        assert!(parse_play(&row).is_none());
        row.collection = COLLECTIONS[0].into();
        row.record_json = "not json".into();
        assert!(parse_play(&row).is_none());
        assert!(
            parse_play(&record(
                COLLECTIONS[0],
                serde_json::json!({"trackName":"Song","artists":[{"artistName":"Artist"}]})
            ))
            .is_none()
        );
    }
    #[test]
    fn deduplicates_cross_app_plays_but_preserves_repeated_listens_and_listeners() {
        let play = parse_play(&teal()).unwrap();
        let end = play.at + 3600;
        let mut rock = record(
            COLLECTIONS[2],
            serde_json::json!({"title":"song", "artist":"artist", "album":"Album", "createdAt":"2026-10-18T10:00:00Z"}),
        );
        rock.at_uri = "at://did:plc:test/app.rocksky.scrobble/two".into();
        let mut again = parse_play(&teal()).unwrap();
        again.at += 60;
        let mut other = parse_play(&teal()).unwrap();
        other.actor = "did:plc:other".into();
        let snapshot = aggregate(
            &[play, parse_play(&rock).unwrap(), again, other],
            end,
            3600,
            0,
            4,
        );
        assert_eq!(snapshot.plays, 3);
        assert_eq!(snapshot.listeners, 2);
        assert_eq!(snapshot.duplicate_plays, 1);
        assert_eq!(snapshot.tracks.len(), 1);
        assert_eq!(snapshot.tracks[0].sources.len(), 2);
        assert_eq!(snapshot.artists[0].plays, 3);
        assert_eq!(snapshot.albums[0].plays, 3);
    }
    #[test]
    fn half_open_windows_exclude_historical_imports_and_future_plays() {
        let play = parse_play(&teal()).unwrap();
        let end = play.at;
        assert_eq!(aggregate(&[play], end, 3600, 0, 1).plays, 0);
        let play = parse_play(&teal()).unwrap();
        assert_eq!(aggregate(&[play], end + 3601, 3600, 0, 1).plays, 0);
        assert_eq!(aggregate(&[], end, 3600, 0, 0).activity.len(), 24);
    }
}
