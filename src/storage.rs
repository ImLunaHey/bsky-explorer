use crate::{
    community::CommunitySnapshot,
    identity::IdentityRecord,
    model::{Detection, NormalizedEvent},
};
use anyhow::{Context, Result};
use chrono::Utc;
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Storage {
    client: Client,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct TableStats {
    pub table: String,
    pub rows: u64,
    pub active_parts: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub compression_ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClickHouseStats {
    pub tables: Vec<TableStats>,
    pub pending_mutations: u64,
    pub errors_since_restart: u64,
    pub active_queries: u64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CommunitySnapshotRow {
    pub snapshot_id: String,
    pub status: String,
    pub window_seconds: u64,
    pub generated_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub community_count: u64,
    pub node_count: u64,
    pub edge_count: u64,
    pub truncated_nodes: u64,
    pub truncated_edges: u64,
    pub parameters_json: String,
    pub error: String,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CommunitySummaryRow {
    pub community_id: String,
    pub account_count: u64,
    pub internal_event_count: u64,
    pub internal_weight: f64,
    pub total_weight: f64,
    pub density: f64,
    pub score: f64,
    pub top_members: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CommunityMemberRow {
    pub did: String,
    pub weighted_degree: f64,
    pub internal_weight: f64,
    pub external_weight: f64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CommunityEdgeRow {
    pub source_community_id: String,
    pub target_community_id: String,
    pub weight: f64,
    pub event_count: u64,
    pub types: Vec<String>,
    pub last_seen_ms: i64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CommunityMemberEdgeRow {
    pub source_did: String,
    pub target_did: String,
    pub weight: f64,
    pub event_count: u64,
    pub types: Vec<String>,
    pub last_seen_ms: i64,
}
#[derive(Debug, Clone, Serialize)]
pub struct CommunityOverview {
    pub snapshot: CommunitySnapshotRow,
    pub communities: Vec<CommunitySummaryRow>,
    pub edges: Vec<CommunityEdgeRow>,
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
    pub has_more: bool,
    pub edges_truncated: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct CommunityDetail {
    pub snapshot: CommunitySnapshotRow,
    pub community: CommunitySummaryRow,
    pub members: Vec<CommunityMemberRow>,
    pub member_edges: Vec<CommunityMemberEdgeRow>,
    pub cross_edges: Vec<CommunityEdgeRow>,
    pub member_total: u64,
    pub member_has_more: bool,
    pub edges_truncated: bool,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CoverageCollection {
    pub collection: String,
    pub namespace: String,
    pub records: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub unique_accounts: u64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct CoverageDay {
    pub date: String,
    pub active_accounts: u64,
    pub new_accounts: u64,
    pub records: u64,
}
#[derive(Debug, Clone, Deserialize, Row)]
pub struct CoverageTotals {
    pub unique_observed_actors: u64,
    pub identities_resolved: u64,
    pub unique_records: u64,
    pub total_bytes: u64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct AccountStoredStats {
    pub event_count: u64,
    pub post_count: u64,
    pub like_count: u64,
    pub repost_count: u64,
    pub follow_count: u64,
    pub unique_targets: u64,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    pub domains: Vec<String>,
    pub urls: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct TrendingAccount {
    pub did: String,
    pub count: u64,
    pub previous_count: u64,
    pub change: i64,
    pub rate_per_hour: f64,
    pub evidence: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredRecord {
    pub at_uri: String,
    pub actor_did: String,
    pub timestamp_ms: i64,
    pub kind: String,
    pub text: String,
    pub reply_parent: String,
    pub reply_root: String,
    pub quote_target: String,
    pub urls: Vec<String>,
    pub domains: Vec<String>,
    pub languages: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredInteraction {
    pub event_id: String,
    pub actor_did: String,
    pub kind: String,
    pub timestamp_ms: i64,
    pub at_uri: String,
    pub target: String,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredAccountEvent {
    pub event_id: String,
    pub time_us: i64,
    pub actor_did: String,
    pub kind: String,
    pub at_uri: String,
    pub target: String,
    pub text: String,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct BackfillEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor_did: String,
    pub kind: String,
    pub collection: String,
    pub operation: String,
    pub at_uri: String,
    pub target: String,
}
#[derive(Debug, Clone, Default, Deserialize, Row, Serialize)]
pub struct FollowWindowStats {
    pub seconds: u64,
    pub outbound_follows: u64,
    pub inbound_follows: u64,
    pub outbound_unfollows_resolved: u64,
    pub inbound_unfollows_resolved: u64,
    pub outbound_unfollows_unresolved: u64,
    pub outbound_net: i64,
    pub inbound_net: i64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredFollowEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor_did: String,
    pub target_did: String,
    pub kind: String,
    pub direction: String,
    pub target_resolved: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccountFollowAnalytics {
    pub windows: Vec<FollowWindowStats>,
    pub recent_events: Vec<StoredFollowEvent>,
    pub unresolved_note: &'static str,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct AccountActivityBucket {
    pub timestamp_ms: i64,
    pub events: u64,
    pub posts: u64,
    pub interactions: u64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct AccountKindCount {
    pub kind: String,
    pub count: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccountAnalytics {
    pub activity: Vec<AccountActivityBucket>,
    pub event_kinds: Vec<AccountKindCount>,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccountCollectionStat {
    pub collection: String,
    pub namespace: String,
    pub family: String,
    pub event_count: u64,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
}
pub fn lexicon_family(collection: &str) -> &'static str {
    if collection.starts_with("app.bsky.graph.list")
        || collection.contains("starterpack")
        || collection.starts_with("tools.ozone.")
    {
        "lists_moderation"
    } else if collection.starts_with("com.whtwnd.") || collection.contains("whitewind") {
        "whitewind_blog"
    } else if collection.starts_with("fm.rocksky.")
        || collection.contains("rocksky")
        || collection.contains("scrobble")
    {
        "rocksky_music"
    } else if collection.starts_with("chat.")
        || collection.contains(".community")
        || collection.contains(".chat")
    {
        "chats_communities"
    } else if collection.contains("profile") || collection.contains("location") {
        "profile_location"
    } else if collection.contains(".rpg") || collection.contains(".game") {
        "rpg_games"
    } else if collection.starts_with("com.imlunahey.") {
        "com_imlunahey"
    } else if collection.starts_with("app.bsky.") {
        "bluesky_social"
    } else {
        "unknown"
    }
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct AccountCollectionSample {
    pub collection: String,
    pub at_uri: String,
    pub operation: String,
    pub timestamp_ms: i64,
    pub record_json: String,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct AccountListMembership {
    pub listitem_uri: String,
    pub list_uri: String,
    pub creator_did: String,
    pub observed_at_ms: i64,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccountLexiconInventory {
    pub collections: Vec<AccountCollectionStat>,
    pub samples: Vec<AccountCollectionSample>,
    pub inbound_list_memberships: Vec<AccountListMembership>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct SearchEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor_did: String,
    pub kind: String,
    pub collection: String,
    pub operation: String,
    pub at_uri: String,
    pub record_json: String,
    pub target: String,
    pub text: String,
    pub urls: Vec<String>,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct SearchIdentity {
    pub did: String,
    pub handle_value: String,
}
#[derive(Debug, Clone, Deserialize, Row)]
struct StoredUri {
    at_uri: String,
}
#[derive(Debug, Clone, Deserialize, Row)]
struct StoredEventId {
    event_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEventType {
    Records,
    Events,
    Both,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct RecordInteractionCounts {
    pub likes: u64,
    pub reposts: u64,
    pub replies: u64,
    pub quotes: u64,
}
#[derive(Row, Serialize)]
struct EventRow {
    event_id: String,
    time_us: i64,
    timestamp: i64,
    actor_did: String,
    kind: String,
    collection: String,
    rkey: String,
    operation: String,
    at_uri: String,
    record_json: String,
    target: String,
    target_did: String,
    text: String,
    reply_parent: String,
    reply_root: String,
    quote_target: String,
    urls: Vec<String>,
    domains: Vec<String>,
    languages: Vec<String>,
    text_hash: u64,
    normalized_text_hash: u64,
    simhash: u64,
}
#[derive(Row, Serialize)]
struct IdentityInsertRow {
    did: String,
    handle: String,
    observed_at: i64,
    source: String,
}
#[derive(Row, Serialize)]
struct CommunitySnapshotInsert {
    snapshot_id: String,
    status: String,
    window_seconds: u64,
    generated_at: i64,
    completed_at: Option<i64>,
    community_count: u64,
    node_count: u64,
    edge_count: u64,
    truncated_nodes: u64,
    truncated_edges: u64,
    parameters_json: String,
    error: String,
}
#[derive(Row, Serialize)]
struct CommunitySummaryInsert {
    snapshot_id: String,
    community_id: String,
    account_count: u64,
    internal_event_count: u64,
    internal_weight: f64,
    total_weight: f64,
    density: f64,
    score: f64,
    top_members: Vec<String>,
}
#[derive(Row, Serialize)]
struct CommunityMembershipInsert {
    snapshot_id: String,
    community_id: String,
    did: String,
    weighted_degree: f64,
    internal_weight: f64,
    external_weight: f64,
}
#[derive(Row, Serialize)]
struct CommunityEdgeInsert {
    snapshot_id: String,
    source_community_id: String,
    target_community_id: String,
    weight: f64,
    event_count: u64,
    types: Vec<String>,
    last_seen: i64,
}
#[derive(Row, Serialize)]
struct CommunityMemberEdgeInsert {
    snapshot_id: String,
    community_id: String,
    source_did: String,
    target_did: String,
    weight: f64,
    event_count: u64,
    types: Vec<String>,
    last_seen: i64,
}
#[derive(Row, Deserialize)]
struct CommunityInteractionRow {
    actor_did: String,
    target_did: String,
    kind: String,
    timestamp_ms: i64,
}
impl From<&NormalizedEvent> for EventRow {
    fn from(e: &NormalizedEvent) -> Self {
        Self {
            event_id: e.event_id.clone(),
            time_us: e.time_us,
            timestamp: e.timestamp.timestamp_millis(),
            actor_did: e.actor_did.clone(),
            kind: e.kind.as_str().into(),
            collection: e.collection.clone(),
            rkey: e.rkey.clone(),
            operation: e.operation.clone(),
            at_uri: e.at_uri.clone(),
            record_json: e.record_json.clone().unwrap_or_default(),
            target: e.target.clone().unwrap_or_default(),
            target_did: e.target_did.clone().unwrap_or_default(),
            text: e.text.clone().unwrap_or_default(),
            reply_parent: e.reply_parent.clone().unwrap_or_default(),
            reply_root: e.reply_root.clone().unwrap_or_default(),
            quote_target: e.quote_target.clone().unwrap_or_default(),
            urls: e.urls.clone(),
            domains: e.domains.clone(),
            languages: e.languages.clone(),
            text_hash: e.text_hash.unwrap_or_default(),
            normalized_text_hash: e.normalized_text_hash.unwrap_or_default(),
            simhash: e.simhash.unwrap_or_default(),
        }
    }
}
impl Storage {
    pub async fn ensure_community_schema(&self) -> Result<()> {
        for statement in include_str!("../migrations/006_communities.sql")
            .split(';')
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            self.client.query(statement).execute().await?
        }
        Ok(())
    }
    pub async fn community_interactions(
        &self,
        window_seconds: u64,
        event_types: &[String],
        limit: u64,
    ) -> Result<Vec<crate::community::CommunityInteraction>> {
        let allowed = event_types
            .iter()
            .filter(|v| matches!(v.as_str(), "like" | "repost" | "quote" | "reply" | "follow"))
            .collect::<Vec<_>>();
        if allowed.is_empty() {
            return Ok(vec![]);
        }
        let kinds = allowed
            .iter()
            .map(|v| format!("'{}'", v))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT actor_did,if(target_did!='',target_did,if(startsWith(target,'at://'),arrayElement(splitByChar('/',substring(target,6)),1),'')) target_did,kind,toUnixTimestamp64Milli(timestamp) timestamp_ms FROM events WHERE timestamp>=now()-INTERVAL ? SECOND AND kind IN ({kinds}) AND actor_did!='' AND (target_did!='' OR target!='') ORDER BY timestamp DESC LIMIT ?"
        );
        let rows = self
            .client
            .query(&sql)
            .bind(window_seconds.clamp(60, 604800))
            .bind(limit.min(2_000_000))
            .fetch_all::<CommunityInteractionRow>()
            .await?;
        Ok(rows
            .into_iter()
            .filter(|r| !r.target_did.is_empty() && r.target_did != r.actor_did)
            .filter_map(|r| {
                Some(crate::community::CommunityInteraction {
                    actor_did: r.actor_did,
                    target_did: r.target_did,
                    kind: r.kind,
                    timestamp: chrono::DateTime::from_timestamp_millis(r.timestamp_ms)?,
                    weight: 1.0,
                })
            })
            .collect())
    }
    pub async fn insert_community_snapshot(&self, s: &CommunitySnapshot) -> Result<()> {
        let start = CommunitySnapshotInsert {
            snapshot_id: s.snapshot_id.clone(),
            status: "building".into(),
            window_seconds: s.window_seconds,
            generated_at: s.generated_at.timestamp_millis(),
            completed_at: None,
            community_count: s.communities.len() as u64,
            node_count: s.node_count as u64,
            edge_count: s.edge_count as u64,
            truncated_nodes: s.truncated_nodes as u64,
            truncated_edges: s.truncated_edges as u64,
            parameters_json: serde_json::to_string(&s.parameters)?,
            error: String::new(),
        };
        let mut insert = self.client.insert("community_snapshots")?;
        insert.write(&start).await?;
        insert.end().await?;
        let mut summaries = self.client.insert("community_summaries")?;
        for c in &s.communities {
            let n = c.account_count as f64;
            let density = if n > 1.0 {
                (2.0 * c.internal_weight) / (n * (n - 1.0))
            } else {
                0.0
            };
            summaries
                .write(&CommunitySummaryInsert {
                    snapshot_id: s.snapshot_id.clone(),
                    community_id: c.community_id.clone(),
                    account_count: c.account_count as u64,
                    internal_event_count: c.internal_event_count,
                    internal_weight: c.internal_weight,
                    total_weight: c.total_weight,
                    density,
                    score: c.total_weight,
                    top_members: c.top_members.clone(),
                })
                .await?
        }
        summaries.end().await?;
        let mut members = self.client.insert("community_memberships")?;
        for m in &s.memberships {
            members
                .write(&CommunityMembershipInsert {
                    snapshot_id: s.snapshot_id.clone(),
                    community_id: m.community_id.clone(),
                    did: m.did.clone(),
                    weighted_degree: m.weighted_degree,
                    internal_weight: m.internal_weight,
                    external_weight: m.external_weight,
                })
                .await?
        }
        members.end().await?;
        let mut edges = self.client.insert("community_edges")?;
        for e in &s.cross_community_edges {
            edges
                .write(&CommunityEdgeInsert {
                    snapshot_id: s.snapshot_id.clone(),
                    source_community_id: e.source_community.clone(),
                    target_community_id: e.target_community.clone(),
                    weight: e.weight,
                    event_count: e.event_count,
                    types: e.event_types.clone(),
                    last_seen: e.last_seen.timestamp_millis(),
                })
                .await?
        }
        edges.end().await?;
        let mut member_edges = self.client.insert("community_member_edges")?;
        for e in &s.member_edges {
            member_edges
                .write(&CommunityMemberEdgeInsert {
                    snapshot_id: s.snapshot_id.clone(),
                    community_id: e.community_id.clone(),
                    source_did: e.source_did.clone(),
                    target_did: e.target_did.clone(),
                    weight: e.weight,
                    event_count: e.event_count,
                    types: e.event_types.clone(),
                    last_seen: e.last_seen.timestamp_millis(),
                })
                .await?
        }
        member_edges.end().await?;
        let done = CommunitySnapshotInsert {
            status: "completed".into(),
            completed_at: Some(Utc::now().timestamp_millis()),
            ..start
        };
        let mut insert = self.client.insert("community_snapshots")?;
        insert.write(&done).await?;
        insert.end().await?;
        Ok(())
    }
    pub async fn coverage(
        &self,
    ) -> Result<(CoverageTotals, Vec<CoverageCollection>, Vec<CoverageDay>)> {
        let totals=self.client.query("SELECT uniq(actor_did) AS unique_observed_actors,assumeNotNull((SELECT uniq(did) FROM identities FINAL WHERE handle!='')) AS identities_resolved,uniqIf(at_uri,at_uri!='') AS unique_records,assumeNotNull((SELECT coalesce(sum(bytes_on_disk),0) FROM system.parts WHERE active AND database=currentDatabase())) AS total_bytes FROM events").fetch_one::<CoverageTotals>();
        let collections=self.client.clone().with_option("max_execution_time","5").with_option("max_threads","2").query("WITH (SELECT assumeNotNull(coalesce(sum(bytes_on_disk),0)) FROM system.parts WHERE active AND database=currentDatabase() AND table='events') AS compressed_total,(SELECT assumeNotNull(coalesce(sum(data_uncompressed_bytes),0)) FROM system.parts WHERE active AND database=currentDatabase() AND table='events') AS uncompressed_total,(SELECT count() FROM events) AS total_rows SELECT collection,arrayStringConcat(arraySlice(splitByChar('.',collection),1,2),'.') AS namespace,count() AS records,assumeNotNull(toUInt64(compressed_total*records/greatest(total_rows,1))) AS compressed_bytes,assumeNotNull(toUInt64(uncompressed_total*records/greatest(total_rows,1))) AS uncompressed_bytes,uniq(actor_did) AS unique_accounts FROM events GROUP BY collection ORDER BY records DESC LIMIT 500").fetch_all::<CoverageCollection>();
        let daily=self.client.clone().with_option("max_execution_time","5").with_option("max_threads","2").query("WITH first_seen AS (SELECT actor_did,min(toDate(timestamp)) AS first_date FROM events GROUP BY actor_did), daily_events AS (SELECT toDate(timestamp) AS day,uniqExact(actor_did) AS active_accounts,count() AS records FROM events WHERE timestamp>=today()-30 GROUP BY day), daily_new AS (SELECT first_date AS day,count() AS new_accounts FROM first_seen WHERE first_date>=today()-30 GROUP BY day) SELECT toString(e.day) AS date,e.active_accounts,coalesce(n.new_accounts,0) AS new_accounts,e.records FROM daily_events e LEFT JOIN daily_new n USING day ORDER BY e.day").fetch_all::<CoverageDay>();
        tokio::try_join!(totals, collections, daily).map_err(Into::into)
    }
    pub async fn trending_accounts(
        &self,
        metric: &str,
        window_seconds: u64,
        limit: u64,
    ) -> Result<Vec<TrendingAccount>> {
        let (did, predicate, evidence) = match metric {
            "followers" => (
                "target_did",
                "kind = 'follow' AND target_did != ''",
                "at_uri",
            ),
            "reposts" => (
                "target_did",
                "kind = 'repost' AND target_did != ''",
                "target",
            ),
            "likes" => ("target_did", "kind = 'like' AND target_did != ''", "target"),
            "replies" => (
                "target_did",
                "kind = 'reply' AND target_did != ''",
                "reply_parent",
            ),
            "posts" => ("actor_did", "kind = 'post'", "at_uri"),
            _ => anyhow::bail!("invalid trending account metric"),
        };
        anyhow::ensure!(
            matches!(window_seconds, 3600 | 86400 | 604800),
            "invalid trending account window"
        );
        let query = format!(
            "WITH now64(3) AS measured_at, measured_at - toIntervalSecond(?) AS current_start, current_start - toIntervalSecond(?) AS previous_start
             SELECT {did} AS did,
               uniqExactIf(event_id, timestamp >= current_start) AS count,
               uniqExactIf(event_id, timestamp < current_start) AS previous_count,
               toInt64(count) - toInt64(previous_count) AS change,
               count / (? / 3600.0) AS rate_per_hour,
               arraySlice(groupUniqArrayIf(20)({evidence}, timestamp >= current_start AND {evidence} != ''), 1, 10) AS evidence
             FROM events PREWHERE timestamp >= previous_start
             WHERE {predicate}
             GROUP BY did HAVING count > 0
             ORDER BY count DESC, did ASC LIMIT ?
             SETTINGS max_execution_time=3, max_threads=4"
        );
        Ok(self
            .client
            .query(&query)
            .bind(window_seconds)
            .bind(window_seconds)
            .bind(window_seconds)
            .bind(limit.clamp(1, 100))
            .fetch_all::<TrendingAccount>()
            .await?)
    }
    pub fn new(url: &str, database: &str, user: &str, password: &str) -> Self {
        Self {
            client: Client::default()
                .with_url(url)
                .with_database(database)
                .with_user(user)
                .with_password(password),
        }
    }
    pub async fn insert_events(&self, events: &[NormalizedEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut insert = self.client.insert("events")?;
        for e in events {
            insert.write(&EventRow::from(e)).await?;
        }
        insert.end().await.context("commit ClickHouse event batch")
    }
    pub async fn ping(&self) -> Result<()> {
        self.client
            .query("SELECT 1")
            .execute()
            .await
            .context("ClickHouse readiness query")
    }
    pub async fn insert_detection(&self, d: &Detection) -> Result<()> {
        let body = serde_json::to_string(d)?;
        self.client.query("INSERT INTO detections (cluster_id, first_seen, last_seen, account_count, event_count, coordination_score, evidence_json) VALUES (?, fromUnixTimestamp64Milli(?), fromUnixTimestamp64Milli(?), ?, ?, ?, ?)").bind(&d.cluster_id).bind(d.first_seen.timestamp_millis()).bind(d.last_seen.timestamp_millis()).bind(d.accounts.len() as u32).bind(d.event_count as u64).bind(d.scores.coordination_score).bind(body).execute().await?;
        Ok(())
    }
    /// Loads the newest persisted version of a coordination detection.
    ///
    /// `cluster_id` is the leading sorting-key column, so this remains a
    /// narrow lookup even as the append-only detections table grows. Avoiding
    /// `FINAL` also prevents ClickHouse from merging every matching part on
    /// the request path; ordering by the replacement version gives us the
    /// same latest-row semantics.
    pub async fn detection(&self, cluster_id: &str) -> Result<Option<Detection>> {
        #[derive(Row, Deserialize)]
        struct DetectionJson {
            evidence_json: String,
        }

        let row = self
            .client
            .query(
                "SELECT evidence_json FROM detections WHERE cluster_id = ? ORDER BY last_seen DESC LIMIT 1",
            )
            .bind(cluster_id)
            .fetch_optional::<DetectionJson>()
            .await?;
        row.map(|row| serde_json::from_str(&row.evidence_json))
            .transpose()
            .context("decode persisted detection evidence")
    }
    pub async fn insert_identity(&self, identity: &IdentityRecord) -> Result<()> {
        self.insert_identities(std::slice::from_ref(identity)).await
    }
    pub async fn insert_identities(&self, identities: &[IdentityRecord]) -> Result<()> {
        if identities.is_empty() {
            return Ok(());
        }
        for table in ["identity_history", "identities"] {
            let mut insert = self.client.insert(table)?;
            for identity in identities {
                insert
                    .write(&IdentityInsertRow {
                        did: identity.did.clone(),
                        handle: identity.handle.clone().unwrap_or_default(),
                        observed_at: identity.observed_at.timestamp_millis(),
                        source: identity.source.clone(),
                    })
                    .await?;
            }
            insert
                .end()
                .await
                .with_context(|| format!("insert identity batch into {table}"))?;
        }
        Ok(())
    }
    pub async fn identity(&self, did: &str) -> Result<Option<IdentityRecord>> {
        #[derive(Row, Deserialize)]
        struct IdentityResult {
            did: String,
            handle: String,
            observed_at_ms: i64,
            source: String,
        }
        let row = self.client
            .query("SELECT did, handle, toUnixTimestamp64Milli(observed_at) AS observed_at_ms, source FROM identities FINAL WHERE did = ? LIMIT 1")
            .bind(did)
            .fetch_optional::<IdentityResult>()
            .await?;
        Ok(row.and_then(|row| {
            Some(IdentityRecord {
                did: row.did,
                handle: (!row.handle.is_empty()).then_some(row.handle),
                observed_at: chrono::DateTime::from_timestamp_millis(row.observed_at_ms)?,
                source: row.source,
            })
        }))
    }
    pub async fn identity_history(&self, did: &str, limit: u64) -> Result<Vec<IdentityRecord>> {
        #[derive(Row, Deserialize)]
        struct IdentityResult {
            did: String,
            handle: String,
            observed_at_ms: i64,
            source: String,
        }
        let rows = self.client
            .query("SELECT did, handle, toUnixTimestamp64Milli(observed_at) AS observed_at_ms, source FROM identity_history WHERE did = ? ORDER BY observed_at DESC LIMIT ?")
            .bind(did).bind(limit)
            .fetch_all::<IdentityResult>().await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(IdentityRecord {
                    did: row.did,
                    handle: (!row.handle.is_empty()).then_some(row.handle),
                    observed_at: chrono::DateTime::from_timestamp_millis(row.observed_at_ms)?,
                    source: row.source,
                })
            })
            .collect())
    }

    /// A bounded sweep catches DIDs produced by repository/association backfills,
    /// which intentionally run in separate processes and cannot enqueue in memory.
    pub async fn unresolved_identity_dids(&self, limit: u64) -> Result<Vec<String>> {
        #[derive(Row, Deserialize)]
        struct DidRow {
            did: String,
        }
        let rows = self.client.query(
            "SELECT actor_did AS did FROM events WHERE operation = 'association_backfill' AND actor_did NOT IN (SELECT did FROM identities FINAL) GROUP BY actor_did LIMIT ? SETTINGS max_execution_time=2"
        ).bind(limit.min(1000)).fetch_all::<DidRow>().await?;
        Ok(rows.into_iter().map(|r| r.did).collect())
    }
    pub async fn account_stats(&self, did: &str) -> Result<Option<AccountStoredStats>> {
        let row = self.client.query(
            "SELECT count() AS event_count, countIf(kind IN ('post','reply','quote')) AS post_count, countIf(kind='like') AS like_count, countIf(kind='repost') AS repost_count, countIf(kind='follow') AS follow_count, uniqExactIf(target, target != '') AS unique_targets, toUnixTimestamp64Milli(min(timestamp)) AS first_seen_ms, toUnixTimestamp64Milli(max(timestamp)) AS last_seen_ms, arraySlice(groupUniqArrayArray(100)(domains), 1, 50) AS domains, arraySlice(groupUniqArrayArray(100)(urls), 1, 50) AS urls FROM events WHERE actor_did = ? HAVING event_count > 0"
        ).bind(did).fetch_optional::<AccountStoredStats>().await?;
        Ok(row)
    }
    pub async fn account_recent_events(
        &self,
        did: &str,
        limit: u64,
    ) -> Result<Vec<StoredAccountEvent>> {
        Ok(self.client.query(
            "SELECT event_id, time_us, actor_did, kind, at_uri, target, text FROM events WHERE actor_did = ? ORDER BY time_us DESC LIMIT ?"
        ).bind(did).bind(limit.min(200))
        .fetch_all::<StoredAccountEvent>().await?)
    }
    pub async fn account_post_count(&self, did: &str) -> Result<u64> {
        Ok(self
            .client
            .query("SELECT uniqExact(at_uri) FROM events WHERE actor_did = ? AND at_uri != '' AND kind IN ('post','reply','quote')")
            .bind(did)
            .fetch_one::<u64>()
            .await?)
    }
    pub async fn association_participants(
        &self,
        did: &str,
        kinds: &[String],
        limit: u64,
    ) -> Result<Vec<String>> {
        if kinds.is_empty() {
            return Ok(vec![]);
        }
        let allowed = kinds
            .iter()
            .filter_map(|v| match v.as_str() {
                "likes" => Some("like"),
                "reposts" => Some("repost"),
                "quotes" => Some("quote"),
                "replies" => Some("reply"),
                _ => None,
            })
            .collect::<Vec<_>>();
        if allowed.is_empty() {
            return Ok(vec![]);
        }
        let quoted = allowed
            .iter()
            .map(|v| format!("'{v}'"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT actor_did FROM events WHERE operation='association_backfill' AND startsWith(target, ?) AND kind IN ({quoted}) AND actor_did != ? LIMIT ?"
        );
        Ok(self
            .client
            .query(&sql)
            .bind(format!("at://{did}/"))
            .bind(did)
            .bind(limit.min(10_000))
            .fetch_all::<String>()
            .await?)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn community_overview(
        &self,
        window: u64,
        min_size: u64,
        sort: &str,
        descending: bool,
        limit: u64,
        offset: u64,
        edge_limit: u64,
        min_edge: f64,
    ) -> Result<Option<CommunityOverview>> {
        let snapshot=self.client.query("SELECT snapshot_id,status,window_seconds,toUnixTimestamp64Milli(generated_at) generated_at_ms,toUnixTimestamp64Milli(completed_at) completed_at_ms,community_count,node_count,edge_count,truncated_nodes,truncated_edges,parameters_json,error FROM community_snapshots FINAL WHERE status='completed' AND window_seconds=? ORDER BY generated_at DESC LIMIT 1").bind(window).fetch_optional::<CommunitySnapshotRow>().await?;
        let Some(snapshot) = snapshot else {
            return Ok(None);
        };
        let sort = match sort {
            "account_count" => "account_count",
            "event_count" => "internal_event_count",
            "density" => "density",
            _ => "score",
        };
        let order = if descending { "DESC" } else { "ASC" };
        let total = self
            .client
            .query(
                "SELECT count() FROM community_summaries WHERE snapshot_id=? AND account_count>=?",
            )
            .bind(&snapshot.snapshot_id)
            .bind(min_size)
            .fetch_one::<u64>()
            .await?;
        let sql = format!(
            "SELECT community_id,account_count,internal_event_count,internal_weight,total_weight,density,score,top_members FROM community_summaries WHERE snapshot_id=? AND account_count>=? ORDER BY {sort} {order},community_id LIMIT ? OFFSET ?"
        );
        let communities = self
            .client
            .query(&sql)
            .bind(&snapshot.snapshot_id)
            .bind(min_size)
            .bind(limit.min(100))
            .bind(offset)
            .fetch_all::<CommunitySummaryRow>()
            .await?;
        let edges=self.client.query("SELECT source_community_id,target_community_id,weight,event_count,types,toUnixTimestamp64Milli(last_seen) last_seen_ms FROM community_edges WHERE snapshot_id=? AND weight>=? ORDER BY weight DESC LIMIT ?").bind(&snapshot.snapshot_id).bind(min_edge).bind(edge_limit.min(500)+1).fetch_all::<CommunityEdgeRow>().await?;
        let truncated = edges.len() > edge_limit.min(500) as usize;
        Ok(Some(CommunityOverview {
            snapshot,
            communities,
            edges: edges
                .into_iter()
                .take(edge_limit.min(500) as usize)
                .collect(),
            total,
            offset,
            limit: limit.min(100),
            has_more: offset + limit.min(100) < total,
            edges_truncated: truncated,
        }))
    }
    pub async fn community_detail(
        &self,
        community_id: &str,
        snapshot_id: Option<&str>,
        member_limit: u64,
        member_offset: u64,
        edge_limit: u64,
        min_edge: f64,
    ) -> Result<Option<CommunityDetail>> {
        let snapshot = if let Some(id) = snapshot_id {
            self.client.query("SELECT snapshot_id,status,window_seconds,toUnixTimestamp64Milli(generated_at) generated_at_ms,toUnixTimestamp64Milli(completed_at) completed_at_ms,community_count,node_count,edge_count,truncated_nodes,truncated_edges,parameters_json,error FROM community_snapshots FINAL WHERE snapshot_id=? AND status='completed' LIMIT 1").bind(id).fetch_optional::<CommunitySnapshotRow>().await?
        } else {
            self.client.query("SELECT snapshot_id,status,window_seconds,toUnixTimestamp64Milli(generated_at) generated_at_ms,toUnixTimestamp64Milli(completed_at) completed_at_ms,community_count,node_count,edge_count,truncated_nodes,truncated_edges,parameters_json,error FROM community_snapshots FINAL WHERE status='completed' ORDER BY generated_at DESC LIMIT 1").fetch_optional::<CommunitySnapshotRow>().await?
        };
        let Some(snapshot) = snapshot else {
            return Ok(None);
        };
        let community=self.client.query("SELECT community_id,account_count,internal_event_count,internal_weight,total_weight,density,score,top_members FROM community_summaries WHERE snapshot_id=? AND community_id=? LIMIT 1").bind(&snapshot.snapshot_id).bind(community_id).fetch_optional::<CommunitySummaryRow>().await?;
        let Some(community) = community else {
            return Ok(None);
        };
        let member_total = self
            .client
            .query(
                "SELECT count() FROM community_memberships WHERE snapshot_id=? AND community_id=?",
            )
            .bind(&snapshot.snapshot_id)
            .bind(community_id)
            .fetch_one::<u64>()
            .await?;
        let members=self.client.query("SELECT did,weighted_degree,internal_weight,external_weight FROM community_memberships WHERE snapshot_id=? AND community_id=? ORDER BY weighted_degree DESC,did LIMIT ? OFFSET ?").bind(&snapshot.snapshot_id).bind(community_id).bind(member_limit.min(200)).bind(member_offset).fetch_all::<CommunityMemberRow>().await?;
        let member_edges=self.client.query("SELECT source_did,target_did,weight,event_count,types,toUnixTimestamp64Milli(last_seen) last_seen_ms FROM community_member_edges WHERE snapshot_id=? AND community_id=? AND weight>=? ORDER BY weight DESC LIMIT ?").bind(&snapshot.snapshot_id).bind(community_id).bind(min_edge).bind(edge_limit.min(500)+1).fetch_all::<CommunityMemberEdgeRow>().await?;
        let truncated = member_edges.len() > edge_limit.min(500) as usize;
        let cross_edges=self.client.query("SELECT source_community_id,target_community_id,weight,event_count,types,toUnixTimestamp64Milli(last_seen) last_seen_ms FROM community_edges WHERE snapshot_id=? AND (source_community_id=? OR target_community_id=?) AND weight>=? ORDER BY weight DESC LIMIT ?").bind(&snapshot.snapshot_id).bind(community_id).bind(community_id).bind(min_edge).bind(edge_limit.min(500)).fetch_all::<CommunityEdgeRow>().await?;
        Ok(Some(CommunityDetail {
            snapshot,
            community,
            members,
            member_edges: member_edges
                .into_iter()
                .take(edge_limit.min(500) as usize)
                .collect(),
            cross_edges,
            member_total,
            member_has_more: member_offset + member_limit.min(200) < member_total,
            edges_truncated: truncated,
        }))
    }
    pub async fn backfill_events(
        &self,
        did: &str,
        associations: bool,
        limit: u64,
        offset: u64,
    ) -> Result<(Vec<BackfillEvent>, u64)> {
        let (operation, ownership) = if associations {
            ("association_backfill", "startsWith(target, ?)")
        } else {
            ("pds_backfill", "actor_did = ?")
        };
        let owner = if associations {
            format!("at://{did}/")
        } else {
            did.to_owned()
        };
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "2")
            .with_option("max_threads", "2")
            .with_option("max_result_rows", limit.clamp(1, 100).to_string());
        let predicate = format!("operation = ? AND {ownership}");
        let count_sql = format!("SELECT count() FROM events WHERE {predicate}");
        let rows_sql = format!(
            "SELECT event_id,toUnixTimestamp64Milli(timestamp) AS timestamp_ms,actor_did,kind,collection,operation,at_uri,target FROM events WHERE {predicate} ORDER BY timestamp DESC,event_id DESC LIMIT ? OFFSET ?"
        );
        let total = client
            .query(&count_sql)
            .bind(operation)
            .bind(&owner)
            .fetch_one::<u64>()
            .await?;
        let rows = client
            .query(&rows_sql)
            .bind(operation)
            .bind(&owner)
            .bind(limit.clamp(1, 100))
            .bind(offset.min(10_000))
            .fetch_all::<BackfillEvent>()
            .await?;
        Ok((rows, total))
    }
    /// Returns bounded follow analytics from normalized events. Jetstream delete
    /// commits normally omit the deleted follow record, so target-less outbound
    /// unfollows are reported separately instead of being misattributed.
    pub async fn account_follow_analytics(&self, did: &str) -> Result<AccountFollowAnalytics> {
        let window_query = self.client.query(
            "WITH ? AS account_did
             SELECT toUInt64(window_seconds) AS seconds,
               countIf(kind='follow' AND actor_did=account_did AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS outbound_follows,
               countIf(kind='follow' AND target_did=account_did AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS inbound_follows,
               countIf(kind='unfollow' AND actor_did=account_did AND target_did!='' AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS outbound_unfollows_resolved,
               countIf(kind='unfollow' AND target_did=account_did AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS inbound_unfollows_resolved,
               countIf(kind='unfollow' AND actor_did=account_did AND target_did='' AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS outbound_unfollows_unresolved,
               toInt64(outbound_follows)-toInt64(outbound_unfollows_resolved)-toInt64(outbound_unfollows_unresolved) AS outbound_net,
               toInt64(inbound_follows)-toInt64(inbound_unfollows_resolved) AS inbound_net
             FROM events ARRAY JOIN [60,300,3600,86400] AS window_seconds
             WHERE collection='app.bsky.graph.follow'
               AND timestamp >= now64(3)-INTERVAL 1 DAY
               AND (actor_did=account_did OR target_did=account_did)
             GROUP BY window_seconds ORDER BY window_seconds"
        ).bind(did).fetch_all::<FollowWindowStats>();
        let events_query = self
            .client
            .query(
                "WITH ? AS account_did
             SELECT event_id, toUnixTimestamp64Milli(timestamp) AS timestamp_ms,
               actor_did, target_did, kind,
               if(actor_did=account_did,'outbound','inbound') AS direction,
               toBool(target_did!='') AS target_resolved
             FROM events
             WHERE collection='app.bsky.graph.follow'
               AND timestamp >= now64(3)-INTERVAL 30 DAY
               AND (actor_did=account_did OR target_did=account_did)
             ORDER BY timestamp DESC LIMIT 100",
            )
            .bind(did)
            .fetch_all::<StoredFollowEvent>();
        let (mut windows, recent_events) = tokio::try_join!(window_query, events_query)?;
        for seconds in [60, 300, 3_600, 86_400] {
            if !windows.iter().any(|window| window.seconds == seconds) {
                windows.push(FollowWindowStats {
                    seconds,
                    ..FollowWindowStats::default()
                });
            }
        }
        windows.sort_by_key(|window| window.seconds);
        Ok(AccountFollowAnalytics {
            windows,
            recent_events,
            unresolved_note: "Jetstream delete events may omit the original follow target. Those outbound unfollows are counted as unknown-target removals; they reduce outbound net change but cannot be attributed to a specific account. Inbound net change can therefore overstate growth when an incoming unfollow target was unavailable.",
        })
    }
    /// Bounded 24-hour activity series for an interactive account page. Both
    /// queries use event time and return at most 24 buckets / a handful of kinds.
    pub async fn account_analytics(&self, did: &str) -> Result<AccountAnalytics> {
        let activity = self
            .client
            .query(
                "SELECT toInt64(toUnixTimestamp(toStartOfHour(timestamp)))*1000 AS timestamp_ms,
               count() AS events,
               countIf(kind IN ('post','reply','quote')) AS posts,
               countIf(kind NOT IN ('post','reply','quote')) AS interactions
             FROM events
             WHERE actor_did=? AND timestamp >= now64(3)-INTERVAL 24 HOUR
             GROUP BY toStartOfHour(timestamp) ORDER BY timestamp_ms",
            )
            .bind(did)
            .fetch_all::<AccountActivityBucket>();
        let event_kinds = self
            .client
            .query(
                "SELECT kind, count() AS count FROM events
             WHERE actor_did=? AND timestamp >= now64(3)-INTERVAL 24 HOUR
             GROUP BY kind ORDER BY count DESC",
            )
            .bind(did)
            .fetch_all::<AccountKindCount>();
        let (activity, event_kinds) = tokio::try_join!(activity, event_kinds)?;
        Ok(AccountAnalytics {
            activity,
            event_kinds,
        })
    }

    /// A bounded repository inventory for account/profile exploration. The
    /// aggregate and samples are separate so large record JSON is never read
    /// while computing counts.
    pub async fn account_lexicon_inventory(&self, did: &str) -> Result<AccountLexiconInventory> {
        #[derive(Debug, Deserialize, Row)]
        struct CollectionRow {
            collection: String,
            event_count: u64,
            first_seen_ms: i64,
            last_seen_ms: i64,
        }
        #[derive(Debug, Deserialize, Row)]
        struct MembershipRow {
            listitem_uri: String,
            list_uri: String,
            observed_at_ms: i64,
        }
        let stats = self.client.query(
            "SELECT collection, count() AS event_count, toUnixTimestamp64Milli(min(timestamp)) AS first_seen_ms, toUnixTimestamp64Milli(max(timestamp)) AS last_seen_ms FROM events WHERE actor_did=? GROUP BY collection ORDER BY event_count DESC, collection LIMIT 200"
        ).bind(did).fetch_all::<CollectionRow>();
        let samples = self.client.query(
            "SELECT collection, at_uri, operation, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, record_json FROM events WHERE actor_did=? AND record_json!='' ORDER BY timestamp DESC LIMIT 3 BY collection LIMIT 600"
        ).bind(did).fetch_all::<AccountCollectionSample>();
        // A listitem delete has no record body. Collapse each listitem URI to
        // its latest operation, but retain the latest non-empty record so an
        // observed membership can be attributed without inventing history.
        let memberships = self.client.query(
            "SELECT listitem_uri, JSONExtractString(record_json, 'list') AS list_uri, toUnixTimestamp64Milli(observed_at) AS observed_at_ms FROM (SELECT at_uri AS listitem_uri, argMax(operation,time_us) AS latest_operation, argMaxIf(record_json,time_us,record_json!='') AS record_json, max(timestamp) AS observed_at FROM events WHERE collection='app.bsky.graph.listitem' GROUP BY at_uri HAVING latest_operation!='delete') WHERE JSONExtractString(record_json,'subject')=? AND list_uri!='' ORDER BY observed_at DESC LIMIT 200"
        ).bind(did).fetch_all::<MembershipRow>();
        let (collection_rows, samples, memberships) =
            tokio::try_join!(stats, samples, memberships)?;
        let collections = collection_rows
            .into_iter()
            .map(|row| {
                let mut parts = row.collection.split('.');
                let namespace = match (parts.next(), parts.next()) {
                    (Some(a), Some(b)) => format!("{a}.{b}"),
                    _ => row.collection.clone(),
                };
                AccountCollectionStat {
                    family: lexicon_family(&row.collection).into(),
                    namespace,
                    collection: row.collection,
                    event_count: row.event_count,
                    first_seen_ms: row.first_seen_ms,
                    last_seen_ms: row.last_seen_ms,
                }
            })
            .collect();
        let inbound_list_memberships = memberships
            .into_iter()
            .map(|row| {
                let creator_did = row
                    .list_uri
                    .strip_prefix("at://")
                    .and_then(|value| value.split('/').next())
                    .unwrap_or_default()
                    .to_owned();
                AccountListMembership {
                    listitem_uri: row.listitem_uri,
                    list_uri: row.list_uri,
                    creator_did,
                    observed_at_ms: row.observed_at_ms,
                }
            })
            .collect();
        Ok(AccountLexiconInventory {
            collections,
            samples,
            inbound_list_memberships,
        })
    }
    /// Searches retained normalized events. Exact identifiers search the full
    /// retention period; free-text searches are deliberately bounded to the
    /// last 30 days so an interactive request cannot scan every partition.
    pub async fn search_events(
        &self,
        term: &str,
        event_type: SearchEventType,
        limit: u64,
    ) -> Result<Vec<SearchEvent>> {
        let is_did = term.starts_with("did:");
        let is_at_uri = term.starts_with("at://");
        let is_url = term.starts_with("http://") || term.starts_with("https://");
        let exact_identifier = is_did || is_at_uri || is_url;
        let type_predicate = match event_type {
            SearchEventType::Records => "kind IN ('post', 'reply', 'quote')",
            SearchEventType::Events => "kind NOT IN ('post', 'reply', 'quote')",
            SearchEventType::Both => "1",
        };
        let time_predicate = if exact_identifier {
            "1"
        } else {
            "timestamp >= now() - INTERVAL 30 DAY"
        };
        let (search_predicate, bind_count) = if is_did {
            ("(actor_did = ? OR target_did = ?)", 2)
        } else if is_at_uri {
            (
                "(at_uri = ? OR target = ? OR reply_parent = ? OR reply_root = ? OR quote_target = ?)",
                5,
            )
        } else if is_url {
            ("(target = ? OR has(urls, ?))", 2)
        } else if term.contains('.') && !term.chars().any(char::is_whitespace) {
            (
                "(has(domains, ?) OR has(urls, ?) OR positionCaseInsensitiveUTF8(text, ?) > 0)",
                3,
            )
        } else {
            (
                "(target = ? OR positionCaseInsensitiveUTF8(text, ?) > 0 OR positionCaseInsensitiveUTF8(record_json, ?) > 0)",
                3,
            )
        };
        let sql = format!(
            "SELECT event_id, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, actor_did, kind, collection, operation, at_uri, record_json, target, text, urls, domains FROM events WHERE {type_predicate} AND {time_predicate} AND {search_predicate} ORDER BY timestamp DESC LIMIT ?"
        );
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "2")
            .with_option("max_threads", "2")
            .with_option("max_result_rows", (limit + 1).to_string());
        let mut query = client.query(&sql);
        for _ in 0..bind_count {
            query = query.bind(term);
        }
        Ok(query
            .bind(limit.min(10_001))
            .fetch_all::<SearchEvent>()
            .await?)
    }

    pub async fn search_identities(&self, term: &str, limit: u64) -> Result<Vec<SearchIdentity>> {
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "1")
            .with_option("max_threads", "2")
            .with_option("max_result_rows", (limit + 1).to_string());
        Ok(client
            .query(
                "SELECT did, argMax(handle, observed_at) AS handle_value FROM identities WHERE did = ? OR positionCaseInsensitiveUTF8(handle, ?) > 0 GROUP BY did ORDER BY (did = ?) DESC, handle_value, did LIMIT ?",
            )
            .bind(term)
            .bind(term)
            .bind(term)
            .bind(limit.min(10_001))
            .fetch_all::<SearchIdentity>()
            .await?)
    }
    pub async fn identities_for_dids(&self, dids: &[String]) -> Result<Vec<SearchIdentity>> {
        if dids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .client
            .query("SELECT did, argMax(handle, observed_at) AS handle_value FROM identities WHERE has(?, did) GROUP BY did")
            .bind(dids)
            .fetch_all::<SearchIdentity>()
            .await?)
    }
    pub async fn account_cluster_ids(&self, did: &str, limit: u64) -> Result<Vec<String>> {
        #[derive(Row, Deserialize)]
        struct ClusterResult {
            cluster_id: String,
        }
        // DIDs cannot contain JSON quote characters, so matching their encoded
        // account-array value avoids substring matches against evidence text.
        let needle = format!("%\"{}\"%", did);
        Ok(self.client
            .query("SELECT cluster_id FROM detections FINAL WHERE evidence_json LIKE ? ORDER BY last_seen DESC LIMIT ?")
            .bind(needle).bind(limit)
            .fetch_all::<ClusterResult>().await?
            .into_iter().map(|row| row.cluster_id).collect())
    }
    pub async fn record(&self, uri: &str) -> Result<Option<StoredRecord>> {
        Ok(self.client.query(
            "SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE at_uri = ? AND kind IN ('post','reply','quote') ORDER BY timestamp DESC LIMIT 1"
        ).bind(uri).fetch_optional::<StoredRecord>().await?)
    }
    pub async fn records(&self, uris: &[String]) -> Result<Vec<StoredRecord>> {
        if uris.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.client.query(
            "SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE has(?, at_uri) AND kind IN ('post','reply','quote') ORDER BY timestamp DESC LIMIT ?"
        ).bind(uris).bind(uris.len() as u64 * 2)
        .fetch_all::<StoredRecord>().await?)
    }
    pub async fn existing_uris(
        &self,
        uris: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        if uris.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        Ok(self
            .client
            .query("SELECT DISTINCT at_uri FROM events WHERE has(?, at_uri)")
            .bind(uris)
            .fetch_all::<StoredUri>()
            .await?
            .into_iter()
            .map(|row| row.at_uri)
            .collect())
    }
    pub async fn account_post_uris(&self, did: &str) -> Result<Vec<String>> {
        Ok(self
            .client
            .query("SELECT DISTINCT at_uri FROM events WHERE actor_did = ? AND collection = 'app.bsky.feed.post' AND operation != 'delete' ORDER BY at_uri")
            .bind(did)
            .fetch_all::<StoredUri>()
            .await?
            .into_iter()
            .map(|row| row.at_uri)
            .collect())
    }
    pub async fn existing_event_ids(
        &self,
        event_ids: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        if event_ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        Ok(self
            .client
            .query("SELECT DISTINCT event_id FROM events WHERE has(?, event_id)")
            .bind(event_ids)
            .fetch_all::<StoredEventId>()
            .await?
            .into_iter()
            .map(|row| row.event_id)
            .collect())
    }
    pub async fn record_interaction_counts(&self, uri: &str) -> Result<RecordInteractionCounts> {
        Ok(self.client.query(
            "SELECT countIf(kind='like' AND target=?) AS likes, countIf(kind='repost' AND target=?) AS reposts, countIf(kind='reply' AND reply_parent=?) AS replies, countIf(kind='quote' AND quote_target=?) AS quotes FROM events"
        ).bind(uri).bind(uri).bind(uri).bind(uri)
        .fetch_one::<RecordInteractionCounts>().await?)
    }
    pub async fn record_interactions(
        &self,
        uri: &str,
        limit: u64,
    ) -> Result<Vec<StoredInteraction>> {
        Ok(self.client.query(
            "SELECT event_id, actor_did, kind, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, at_uri, target FROM events WHERE (kind IN ('like','repost') AND target=?) OR (kind='reply' AND reply_parent=?) OR (kind='quote' AND quote_target=?) ORDER BY timestamp DESC LIMIT ?"
        ).bind(uri).bind(uri).bind(uri).bind(limit)
        .fetch_all::<StoredInteraction>().await?)
    }
    pub async fn conversation_context(&self, root: &str, limit: u64) -> Result<Vec<StoredRecord>> {
        Ok(self.client.query(
            "SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE at_uri=? OR reply_root=? ORDER BY timestamp ASC LIMIT ?"
        ).bind(root).bind(root).bind(limit)
        .fetch_all::<StoredRecord>().await?)
    }
    pub async fn bytes(&self) -> Result<u64> {
        self.table_bytes(None).await
    }
    pub async fn table_bytes(&self, table: Option<&str>) -> Result<u64> {
        let table = table.unwrap_or("");
        Ok(self.client.query("SELECT coalesce(sum(bytes_on_disk), 0) FROM system.parts WHERE active AND database=currentDatabase() AND (? = '' OR table = ?)").bind(table).bind(table).fetch_one::<u64>().await?)
    }
    pub async fn stats(&self) -> Result<ClickHouseStats> {
        let tables = self.client.query(
            "SELECT table, sum(rows) AS rows, count() AS active_parts, sum(bytes_on_disk) AS compressed_bytes, sum(data_uncompressed_bytes) AS uncompressed_bytes, if(compressed_bytes=0, 0., uncompressed_bytes/compressed_bytes) AS compression_ratio FROM system.parts WHERE active AND database=currentDatabase() GROUP BY table ORDER BY compressed_bytes DESC"
        ).fetch_all::<TableStats>().await?;
        let pending_mutations = self.client.query(
            "SELECT count() FROM system.mutations WHERE database=currentDatabase() AND NOT is_done"
        ).fetch_one::<u64>().await.unwrap_or(0);
        let errors_since_restart = self
            .client
            .query("SELECT coalesce(sum(value), 0) FROM system.errors")
            .fetch_one::<u64>()
            .await
            .unwrap_or(0);
        let active_queries = self
            .client
            .query("SELECT count() FROM system.processes")
            .fetch_one::<u64>()
            .await
            .unwrap_or(0);
        Ok(ClickHouseStats {
            tables,
            pending_mutations,
            errors_since_restart,
            active_queries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trending_accounts_rejects_dynamic_sql_inputs_before_querying() {
        let storage = Storage::new("http://127.0.0.1:1", "observatory", "default", "");
        assert!(
            storage
                .trending_accounts("likes OR 1=1", 3600, 20)
                .await
                .is_err()
        );
        assert!(storage.trending_accounts("likes", 123, 20).await.is_err());
    }
    #[tokio::test]
    #[ignore = "requires a local ClickHouse observatory database"]
    async fn coverage_rows_decode_from_clickhouse() {
        let storage = Storage::new("http://127.0.0.1:8123", "observatory", "default", "");
        let (totals, collections, daily) = storage.coverage().await.unwrap();
        assert!(totals.unique_observed_actors > 0);
        assert!(!collections.is_empty());
        assert!(!daily.is_empty());
    }
    use crate::model::EventKind;
    #[test]
    fn classifies_known_and_unknown_lexicon_families_without_guessing() {
        assert_eq!(
            lexicon_family("app.bsky.graph.listitem"),
            "lists_moderation"
        );
        assert_eq!(lexicon_family("com.whtwnd.blog.entry"), "whitewind_blog");
        assert_eq!(lexicon_family("fm.rocksky.scrobble"), "rocksky_music");
        assert_eq!(lexicon_family("com.imlunahey.status"), "com_imlunahey");
        assert_eq!(lexicon_family("net.example.unknown"), "unknown");
    }
    #[test]
    fn serializes_without_options() {
        let e = NormalizedEvent {
            event_id: "e".into(),
            time_us: 0,
            timestamp: chrono::Utc::now(),
            actor_did: "d".into(),
            kind: EventKind::Like,
            collection: "c".into(),
            rkey: "r".into(),
            operation: "create".into(),
            at_uri: "u".into(),
            record_json: None,
            target: None,
            target_did: None,
            text: None,
            reply_parent: None,
            reply_root: None,
            quote_target: None,
            urls: vec![],
            domains: vec![],
            languages: vec![],
            text_hash: None,
            normalized_text_hash: None,
            simhash: None,
        };
        let r = EventRow::from(&e);
        assert_eq!(r.target, "");
        assert_eq!(r.kind, "like")
    }

    #[test]
    fn schema_is_append_optimized_bounded_and_blob_free() {
        let schema = include_str!("../migrations/001_init.sql");
        assert!(schema.contains("ENGINE = MergeTree"));
        assert!(schema.contains("PARTITION BY toYYYYMM(timestamp)"));
        assert!(!schema.contains("TTL timestamp"));
        assert!(schema.contains("CODEC(ZSTD(3))"));

        let lowercase = schema.to_ascii_lowercase();
        for forbidden in [" image ", " avatar ", " banner ", " video ", " blob "] {
            assert!(
                !lowercase.contains(forbidden),
                "schema must not retain media column {forbidden:?}"
            );
        }
    }

    #[test]
    fn schema_matches_serialized_event_columns_in_order() {
        let schema = include_str!("../migrations/001_init.sql");
        let expected = [
            "event_id",
            "time_us",
            "timestamp",
            "actor_did",
            "kind",
            "collection",
            "rkey",
            "at_uri",
            "target",
            "target_did",
            "text",
            "reply_parent",
            "reply_root",
            "quote_target",
            "urls",
            "domains",
            "languages",
            "text_hash",
            "normalized_text_hash",
            "simhash",
        ];
        let event_table = schema
            .split("CREATE TABLE IF NOT EXISTS observatory.events")
            .nth(1)
            .unwrap()
            .split("ENGINE")
            .next()
            .unwrap();
        let mut previous = 0;
        for column in expected {
            let position = event_table[previous..]
                .find(column)
                .unwrap_or_else(|| panic!("missing or out-of-order ClickHouse column {column}"));
            previous += position + column.len();
        }
    }

    #[test]
    fn identity_schema_keeps_current_and_replayable_history() {
        let schema = include_str!("../migrations/002_identities.sql");
        assert!(schema.contains("identity_history"));
        assert!(schema.contains("ENGINE = ReplacingMergeTree(observed_at)"));
        assert!(!schema.contains("TTL observed_at"));
    }
    #[test]
    fn community_schema_retains_immutable_bounded_graph_snapshots() {
        let schema = include_str!("../migrations/006_communities.sql");
        for table in [
            "community_snapshots",
            "community_summaries",
            "community_memberships",
            "community_edges",
            "community_member_edges",
        ] {
            assert!(schema.contains(table))
        }
        assert!(schema.contains("MergeTree"));
        assert!(!schema.contains("TTL "));
        assert!(schema.contains("snapshot_id"));
    }
}
