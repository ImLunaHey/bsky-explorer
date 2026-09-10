ALTER TABLE observatory.events
    MODIFY TTL timestamp + INTERVAL 365 DAY
    DELETE WHERE operation NOT IN ('pds_backfill', 'association_backfill');
