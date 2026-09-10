export type Scores = { automation_score:number; coordination_score:number; shared_target_score:number; timing_similarity_score:number; content_similarity_score:number }
export type EvidenceEvent = { event_id:string; actor_did:string; kind:string; timestamp:string; target?:string; at_uri?:string; text?:string; reply_parent?:string; reply_root?:string; quote_target?:string }
export type Detection = { cluster_id:string; first_seen:string; last_seen:string; accounts:string[]; event_count:number; scores:Scores; shared_target_ratio:number; median_timing_difference_ms:number; repeated_content_count:number; common_urls:string[]; common_domains:string[]; common_targets:string[]; sample_events:EvidenceEvent[] }
export type ClusterDetail = Detection & { timeline:{timestamp:string;event_count:number}[]; graph_edges:{source:string;target:string;event_count:number}[] }
export type WindowRates = { seconds:number; events_per_sec:number; posts_per_sec:number; likes_per_sec:number; follows_per_sec:number; interactions_per_sec:number }
export type Rates = WindowRates & { windows:WindowRates[] }
export type TrendingItem = { key:string; count:number; recent_count:number; growth:number; sample?:string;record?:RecordItem|null;handle?:string|null }
export type AccountSummary = { did:string; handle?:string; events_24h:number; posts_24h:number; unique_targets:number; automation_score:number; [key:string]:unknown }
export type AccountDetail = { current_identity:Identity; identity_history:Identity[]; activity:AccountSummary & { first_seen?:string;last_seen?:string;windows:{seconds:number;events:number;posts:number;likes:number;reposts:number;follows:number;interactions:number;events_per_second:number}[];unique_domains:string[];unique_urls:string[];recent_events:FirehoseEvent[];cluster_ids:string[];scores:Scores }; stored?:{event_count:number;post_count:number;like_count:number;repost_count:number;follow_count:number;unique_targets:number;first_seen_ms:number;last_seen_ms:number;domains:string[];urls:string[]} }
export type AccountEvent = {event_id:string;time_us:number;actor_did:string;kind:string;at_uri:string;target:string;text:string}
export type Anomalies = { coordination_clusters:Detection[]; unusual_posting:AccountSummary[]; repeated_content_campaigns:Detection[] }
export type StorageStats = { raw_archive_bytes:number; clickhouse_bytes:number; derived_bytes:number; bytes_ingested_per_hour:number; bytes_ingested_per_day:number; projected_monthly_bytes:number; projected_yearly_bytes:number }
export type SystemStats = {
  scope_note:string
  runtime:{process_uptime_seconds:number;process_resident_memory_bytes:number;container_memory_used_bytes?:number;container_memory_limit_bytes?:number;process_cpu_utilization_percent?:number;container_cpu_utilization_percent?:number;host_cpu_utilization_percent?:number;host_load_average?:number[];host_uptime_seconds?:number}
  storage_array:{path:string;total_bytes:number;available_bytes:number;used_bytes:number}
  clickhouse?:{tables:{table:string;rows:number;active_parts:number;compressed_bytes:number;uncompressed_bytes:number}[];pending_mutations:number;errors_since_restart:number;active_queries:number}
  clickhouse_error?:string
}
export type SystemSample = { timestamp:number;events_per_second:number;posts_per_second:number;interactions_per_second:number;process_resident_memory_bytes:number;container_memory_used_bytes?:number;clickhouse_rows?:number;clickhouse_compressed_bytes?:number;process_cpu_utilization_percent?:number;container_cpu_utilization_percent?:number;host_cpu_utilization_percent?:number }
export type Identity = { did:string;handle?:string;observed_at:string;source:string }
export type RecordItem = { at_uri:string;actor_did:string;timestamp_ms:number;kind:string;text:string;reply_parent:string;reply_root:string;quote_target:string;urls:string[];domains:string[];languages:string[] }
export type RecordDetail = { record:RecordItem;interaction_counts:{likes:number;reposts:number;replies:number;quotes:number};recent_interactions:{event_id:string;actor_did:string;kind:string;timestamp_ms:number;at_uri:string;target:string}[];conversation:RecordItem[] }
export type RecordMedia = { kind:'image'|'video';url:string;thumbnail_url?:string;alt:string;width?:number;height?:number }
export type RecordMediaResponse = { media:RecordMedia[];source:'public_appview';ephemeral:true }
export type FirehoseEvent = { event_id:string;time_us:number;actor_did:string;kind:string;timestamp:string;at_uri:string;target?:string;target_did?:string;text?:string;urls:string[];domains:string[];reply_parent?:string;reply_root?:string;quote_target?:string }
export type FirehoseFeed = { events:FirehoseEvent[];next_after_time_us?:number;retained_event_count:number;has_more:boolean }
export type TrendDrilldown = { events:FirehoseEvent[];total:number;offset:number;limit:number;cluster_ids:string[] }
export type SearchKind = 'all'|'account'|'record'|'event'
export type SearchResult = {
  result_type:'account'|'record'|'event';did:string;handle:string|null;event_id:string|null
  at_uri:string|null;event_type:string|null;timestamp_ms:number|null;text:string|null
  target:string|null;urls:string[];domains:string[];collection:string|null;operation:string|null;record_json:string|null
}
export type SearchResponse = { query:string;type:SearchKind;offset:number;limit:number;has_more:boolean;results:SearchResult[] }
