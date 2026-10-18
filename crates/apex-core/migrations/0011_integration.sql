-- Phase 10 : liaison facultative, sans accès partagé aux bases.
CREATE TABLE integration_keys (
 id TEXT PRIMARY KEY NOT NULL,
 site_id TEXT NOT NULL REFERENCES sites(id),
 label TEXT NOT NULL,
 key_prefix TEXT NOT NULL,
 key_hash TEXT NOT NULL UNIQUE,
 scopes TEXT NOT NULL CHECK(json_valid(scopes)),
 created_by_id TEXT NOT NULL REFERENCES users(id),
 created_at TEXT NOT NULL,
 last_used_at TEXT,
 last_used_ip TEXT,
 revoked_at TEXT,
 expires_at TEXT
) STRICT;
CREATE INDEX integration_keys_site ON integration_keys(site_id,id);
ALTER TABLE equipment_counters ADD COLUMN external_ref TEXT;
CREATE UNIQUE INDEX counter_external_ref ON equipment_counters(site_id,external_ref) WHERE deleted_at IS NULL AND external_ref IS NOT NULL;
ALTER TABLE equipments ADD COLUMN external_ref TEXT;
CREATE UNIQUE INDEX equipment_external_ref ON equipments(site_id,external_ref) WHERE deleted_at IS NULL AND external_ref IS NOT NULL;
CREATE TABLE equipment_external_status (
 equipment_id TEXT PRIMARY KEY NOT NULL REFERENCES equipments(id),
 site_id TEXT NOT NULL REFERENCES sites(id),
 source TEXT NOT NULL DEFAULT 'supervision' CHECK(source='supervision'),
 state TEXT NOT NULL CHECK(state IN ('running','stopped','degraded','alarm','unknown')),
 since TEXT NOT NULL,
 message TEXT,
 received_at TEXT NOT NULL
) STRICT;
CREATE INDEX external_status_site ON equipment_external_status(site_id,state,equipment_id);
CREATE TABLE equipment_external_status_history (
 id TEXT PRIMARY KEY NOT NULL,
 equipment_id TEXT NOT NULL REFERENCES equipments(id),
 site_id TEXT NOT NULL REFERENCES sites(id),
 source TEXT NOT NULL DEFAULT 'supervision' CHECK(source='supervision'),
 state TEXT NOT NULL CHECK(state IN ('running','stopped','degraded','alarm','unknown')),
 since TEXT NOT NULL,
 message TEXT,
 received_at TEXT NOT NULL
) STRICT;
CREATE INDEX external_history_site ON equipment_external_status_history(site_id,equipment_id,received_at,id);
CREATE TABLE integration_events (
 id TEXT PRIMARY KEY NOT NULL,
 site_id TEXT NOT NULL REFERENCES sites(id),
 key_id TEXT NOT NULL REFERENCES integration_keys(id),
 kind TEXT NOT NULL CHECK(kind IN ('counters','status')),
 received_at TEXT NOT NULL,
 items_count INTEGER NOT NULL,
 accepted INTEGER NOT NULL,
 rejected INTEGER NOT NULL,
 errors TEXT NOT NULL CHECK(json_valid(errors)),
 duration_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX integration_events_site ON integration_events(site_id,id);
INSERT INTO app_settings(key,value,updated_at) VALUES
 ('integration.supervision.enabled','false',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('integration.supervision.counters_enabled','true',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('integration.supervision.status_enabled','true',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('integration.supervision.counter_min_interval_min','15',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('integration.supervision.status_stale_after_min','30',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('integration.supervision.status_history_days','90',strftime('%Y-%m-%dT%H:%M:%fZ','now'));
