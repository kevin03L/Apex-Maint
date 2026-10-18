-- Phase 9 : réponses des opérations hors ligne, conservées trente jours.
CREATE TABLE idempotency_keys (
    key TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id),
    site_id TEXT REFERENCES sites(id),
    route TEXT NOT NULL,
    response_code INTEGER NOT NULL,
    response_body TEXT NOT NULL CHECK(json_valid(response_body)),
    created_at TEXT NOT NULL
) STRICT;
CREATE INDEX idempotency_created ON idempotency_keys(created_at);
