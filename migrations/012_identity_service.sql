-- Durable, insertion-time ordered feed for the identity service. Identity
-- events remain in ClickHouse even while the resolver is unavailable.
CREATE TABLE IF NOT EXISTS identity_changes (
 received_at DateTime64(3, 'UTC') DEFAULT now64(3),
 did String, handle String, observed_at DateTime64(3, 'UTC'), source String
) ENGINE = MergeTree ORDER BY (received_at, did);

CREATE MATERIALIZED VIEW IF NOT EXISTS identity_changes_mv TO identity_changes AS
SELECT now64(3) AS received_at, toString(did) AS did, toString(handle) AS handle,
 observed_at, toString(source) AS source FROM identity_history;
