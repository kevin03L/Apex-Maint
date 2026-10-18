-- Migration 0001 : Socle initial (sites et paramètres de l'application)

CREATE TABLE IF NOT EXISTS sites (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CONSTRAINT chk_sites_id CHECK (length(id) > 0),
    CONSTRAINT chk_sites_name CHECK (length(name) > 0)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_sites_deleted_at ON sites(deleted_at);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT chk_app_settings_key CHECK (length(key) > 0)
) STRICT;

INSERT OR IGNORE INTO app_settings (key, value, updated_at) VALUES
    ('app_name', 'Apex Maint', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('app_subtitle', 'GMAO', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('stock_alert_threshold', '5', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('session_timeout_min', '720', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
