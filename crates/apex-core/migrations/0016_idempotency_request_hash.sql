-- Empreinte du corps ; les anciennes clés restent rejouables jusqu’à leur purge habituelle.
ALTER TABLE idempotency_keys ADD COLUMN request_hash TEXT;
