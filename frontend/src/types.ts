export type Scores = { automation_score:number; coordination_score:number; shared_target_score:number; timing_similarity_score:number; content_similarity_score:number }
export type EvidenceEvent = { event_id:string; actor_did:string; kind:string; timestamp:string; target?:string; at_uri?:string; text?:string; reply_parent?:string; reply_root?:string; quote_target?:string }
export type Detection = { cluster_id:string; first_seen:string; last_seen:string; accounts:string[]; event_count:number; scores:Scores; shared_target_ratio:number; median_timing_difference_ms:number; repeated_content_count:number; common_urls:string[]; common_domains:string[]; common_targets:string[]; sample_events:EvidenceEvent[] }
export type ClusterDetail = Detection & { timeline:{timestamp:string;event_count:number}[]; graph_edges:{source:string;target:string;event_count:number}[] }
export type WindowRates = { seconds:number; events_per_sec:number; posts_per_sec:number; likes_per_sec:number; follows_per_sec:number; interactions_per_sec:number }
export type Rates = WindowRates & { windows:WindowRates[] }
export type TrendingItem = { key:string; count:number; recent_count:number; growth:number; sample?:string;record?:RecordItem|null;handle?:string|null }
export type TopAccountMetric = 'followers'|'reposts'|'likes'|'replies'|'posts'
export type TopAccount = {did:string;count:number;previous_count:number;change:number;rate_per_hour:number;evidence:string[]}
export type TopAccountsResponse = {metric:TopAccountMetric;window:'1h'|'24h'|'7d';window_seconds:number;accounts:TopAccount[]}
export type AccountSummary = { did:string; handle?:string; events_24h:number; posts_24h:number; unique_targets:number; automation_score:number; [key:string]:unknown }
export type AccountDetail = { current_identity:Identity; identity_history:Identity[]; activity:AccountSummary & { first_seen?:string;last_seen?:string;windows:{seconds:number;events:number;posts:number;likes:number;reposts:number;follows:number;interactions:number;events_per_second:number}[];unique_domains:string[];unique_urls:string[];recent_events:FirehoseEvent[];cluster_ids:string[];scores:Scores }; stored?:{event_count:number;post_count:number;like_count:number;repost_count:number;follow_count:number;unique_targets:number;first_seen_ms:number;last_seen_ms:number;domains:string[];urls:string[]} }
export type AccountEvent = {event_id:string;time_us:number;actor_did:string;kind:string;at_uri:string;target:string;text:string}
export type AccountProfile = {did:string;handle:string;display_name?:string|null;description?:string|null;avatar_url?:string|null;labels:string[];followers_count?:number|null;follows_count?:number|null;posts_count?:number|null;indexed_at?:string|null;observed_at:string}
export type Anomalies = { coordination_clusters:Detection[]; unusual_posting:AccountSummary[]; repeated_content_campaigns:Detection[] }
export type StorageStats = { raw_archive_bytes:number; clickhouse_bytes:number; derived_bytes:number; bytes_ingested_per_hour:number; bytes_ingested_per_day:number; projected_monthly_bytes:number; projected_yearly_bytes:number;storage_budget_bytes:number;used_footprint_bytes:number;remaining_allocation_bytes:number;physical_available_bytes:number;allocation_runway_days?:number;allocation_runway_months?:number;allocation_runway_years?:number;physical_runway_days?:number;physical_runway_months?:number;physical_runway_years?:number;limiting_factor:'observatory_allocation'|'physical_array'|'growth_unavailable' }
export type SystemStats = {
  scope_note:string
  runtime:{process_uptime_seconds:number;process_resident_memory_bytes:number;container_memory_used_bytes?:number;container_memory_limit_bytes?:number;process_cpu_utilization_percent?:number;container_cpu_utilization_percent?:number;host_cpu_utilization_percent?:number;host_load_average?:number[];host_uptime_seconds?:number}
  storage_array:{path:string;total_bytes:number;available_bytes:number;used_bytes:number}
  clickhouse?:{tables:{table:string;rows:number;active_parts:number;compressed_bytes:number;uncompressed_bytes:number}[];pending_mutations:number;errors_since_restart:number;active_queries:number}
  clickhouse_error?:string
}
export type SystemSample = { timestamp:number;events_per_second:number;posts_per_second:number;interactions_per_second:number;process_resident_memory_bytes:number;container_memory_used_bytes?:number;clickhouse_rows?:number;clickhouse_compressed_bytes?:number;process_cpu_utilization_percent?:number;container_cpu_utilization_percent?:number;host_cpu_utilization_percent?:number }
export type Identity = { did:string;handle?:string|null;observed_at:string;source:string }
export type RecordItem = { at_uri:string;actor_did:string;timestamp_ms:number;kind:string;text:string;reply_parent:string;reply_root:string;quote_target:string;urls:string[];domains:string[];languages:string[] }
export type RecordDetail = { record:RecordItem;interaction_counts:{likes:number;reposts:number;replies:number;quotes:number};recent_interactions:{event_id:string;actor_did:string;kind:string;timestamp_ms:number;at_uri:string;target:string}[];conversation:RecordItem[] }
export type RecordMedia = { kind:'image'|'video';url:string;thumbnail_url?:string;alt:string;width?:number;height?:number }
export type RecordMediaResponse = { media:RecordMedia[];source:'public_appview';ephemeral:true }
export type FirehoseEvent = { event_id:string;time_us:number;actor_did:string;kind:string;collection:string;timestamp:string;at_uri:string;target?:string;target_did?:string;text?:string;urls:string[];domains:string[];reply_parent?:string;reply_root?:string;quote_target?:string;media_kind?:'images'|'video'|'external'|'record_with_media'|'unknown'|'none'|string;media_count?:number }
export type FirehoseFeed = { events:FirehoseEvent[];next_after_time_us?:number;retained_event_count:number;has_more:boolean }
export type TrendDrilldown = { events:FirehoseEvent[];total:number;offset:number;limit:number;cluster_ids:string[] }
export type SearchKind = 'all'|'account'|'record'|'event'
export type SearchResult = {
  result_type:'account'|'record'|'event';did:string;handle:string|null;event_id:string|null
  at_uri:string|null;event_type:string|null;timestamp_ms:number|null;text:string|null
  target:string|null;urls:string[];domains:string[];collection:string|null;operation:string|null;record_json:string|null
}
export type BackfillStatus = 'queued'|'resolving'|'running'|'retrying'|'paused'|'completed'|'dead_letter'|'cancelled'
export type BackfillProgress={kind:string;updated_at:string;collection_index?:number|null;current_collection?:string|null;cursor?:string|null;records_written:number;posts_processed?:number|null;posts_completed?:number|null;posts_total?:number|null;posts_remaining?:number|null;requests?:number|null;likes?:number|null;reposts?:number|null;quotes?:number|null;replies?:number|null;errors?:number|null;rate_per_second?:number|null;eta_seconds?:number|null}
export type BackfillJob = { id:string;query:string;did?:string|null;handle?:string|null;status:BackfillStatus;job_type:'repository'|'associations';depends_on_job_id:string|null;parent_job_id?:string|null;crawl_id?:string|null;include_associations:boolean;scope?:string;crawl_depth?:number;max_depth?:number;created_at:string;updated_at:string;collection?:string|null;cursor?:string|null;records_written:number;retry_count:number;max_retries:number;last_error?:string|null;next_attempt_at?:string|null;lease_token?:string|null;lease_until?:string|null;progress?:BackfillProgress|null }
export type CrawlStatus='queued'|'running'|'paused'|'completed'|'cancelled'|'failed'
export type CrawlRelation='posts'|'likes'|'reposts'|'quotes'|'replies'|'profiles'
export type CrawlAction='pause'|'resume'|'cancel'
export type CrawlLimitKey='max_accounts'|'max_jobs'|'max_records'|'max_requests'
export type CrawlLimits=Partial<Record<CrawlLimitKey,number>>
export type Crawl={id:string;root_query:string;root_did?:string|null;status:CrawlStatus;max_depth:number;relations:CrawlRelation[];limits:CrawlLimits;created_at:string;updated_at:string}
export type CrawlsResponse={crawls:Crawl[];total:number;offset:number;limit:number;has_more:boolean}
export type CrawlDetailResponse={crawl:Crawl;jobs:BackfillJob[]}
export type CreateCrawlResponse={crawl:Crawl;root_job:BackfillJob}
export type CoverageCollection={collection:string;namespace:string;records:number;unique_accounts:number;compressed_bytes:number;uncompressed_bytes:number}
export type CoverageDaily={date:string;active_accounts:number;new_accounts:number;records:number}
export type CoverageResponse={summary:{unique_observed_actors:number;identities_resolved:number;repository_accounts_completed:number;repository_jobs_queued:number;repository_jobs_running:number;repository_jobs_partial:number;repository_jobs_failed:number;unique_records:number;total_bytes:number;coverage_estimate?:number|null;coverage_denominator?:number|null;coverage_qualification:string};collections:CoverageCollection[];daily:CoverageDaily[]}
export type CommunitySnapshot={snapshot_id:string;status:string;window_seconds:number;generated_at_ms:number;completed_at_ms?:number|null;community_count:number;node_count:number;edge_count:number;error?:string|null}
export type Community={community_id:string;account_count:number;internal_event_count:number;internal_weight:number;total_weight:number;density:number;score:number;top_members:string[]}
export type CommunityEdge={source_community_id:string;target_community_id:string;weight:number;event_count:number;types:string[];last_seen_ms:number}
export type CommunityMember={did:string;weighted_degree:number;internal_weight:number;external_weight:number}
export type CommunityMemberEdge={source_did:string;target_did:string;weight:number;event_count:number;types:string[];last_seen_ms:number}
export type CommunityTimelinePoint={bucket:string;events:number;accounts:number}
export type CommunitiesResponse={snapshot:CommunitySnapshot;communities:Community[];edges?:CommunityEdge[];edges_truncated?:boolean;total:number;offset:number;limit:number;has_more:boolean}
export type CommunityDetailResponse={snapshot:CommunitySnapshot;community:Community;members:CommunityMember[];member_total:number;member_has_more:boolean;cross_edges?:CommunityEdge[];edges_truncated:boolean;member_edges?:CommunityMemberEdge[];timeline?:CommunityTimelinePoint[]}
export type BackfillEvent={event_id:string;timestamp_ms:number;actor_did:string;kind:string;collection:string;operation:string;at_uri:string;target:string}
export type BackfillEventsResponse={events:BackfillEvent[];total:number;offset:number}
export type SearchResponse = { query:string;type:SearchKind;offset:number;limit:number;has_more:boolean;results:SearchResult[];backfill_job?:BackfillJob|null }
export type BackfillJobsResponse = {jobs:BackfillJob[];total?:number;offset?:number;limit?:number;has_more?:boolean}
export type IdentityQueueStatus = 'queued'|'running'|'retrying'|'resolved'|'negative'|'dead_letter'
export type IdentityQueueJob = {id:string;did:string;handle?:string|null;status:IdentityQueueStatus;created_at:string;updated_at:string;started_at?:string|null;completed_at?:string|null;duration_ms?:number|null;next_attempt_at?:string|null;attempts:number;max_attempts:number;last_error?:string|null}
export type IdentityQueueSeries = {bucket:string;resolved:number;negative:number;failures:number;completed:number;avg_duration_ms?:number|null}
export type IdentityQueueAnalytics = {avg_duration_ms?:number|null;p50_duration_ms?:number|null;p95_duration_ms?:number|null;p99_duration_ms?:number|null;success_rate:number;negative_rate:number;failure_rate:number;throughput_1m:number;throughput_5m:number;throughput_1h:number;throughput_24h:number;backlog_eta_seconds?:number|null;oldest_queued_age_seconds?:number|null;avg_attempts:number;max_attempts_observed:number;series:IdentityQueueSeries[]}
export type IdentityQueueResponse = {summary:{total:number;queued:number;running:number;retrying:number;resolved:number;negative:number;dead_letter:number};jobs:IdentityQueueJob[];total:number;offset:number;limit:number;has_more:boolean;analytics:IdentityQueueAnalytics}
