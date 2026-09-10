-- Observatory retention is indefinite. Capacity is managed through measured
-- growth and explicit operator action, never automatic record expiry.
ALTER TABLE observatory.events REMOVE TTL;
ALTER TABLE observatory.identity_history REMOVE TTL;
