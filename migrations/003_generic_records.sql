ALTER TABLE observatory.events
    ADD COLUMN IF NOT EXISTS operation LowCardinality(String) AFTER rkey,
    ADD COLUMN IF NOT EXISTS record_json String CODEC(ZSTD(3)) AFTER at_uri;
