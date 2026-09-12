ALTER TABLE observatory.events
    ADD COLUMN IF NOT EXISTS media_kind LowCardinality(String) DEFAULT '' AFTER record_json;

ALTER TABLE observatory.events
    ADD COLUMN IF NOT EXISTS media_count UInt8 DEFAULT 0 AFTER media_kind;
