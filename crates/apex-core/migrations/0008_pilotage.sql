-- Pilotage, préférences, file transactionnelle et réinitialisation par e-mail.
CREATE TABLE dashboard_layouts (
 scope TEXT NOT NULL CHECK(scope IN ('role','user')),
 role TEXT CHECK(role IN ('admin','responsable','production','technicien')),
 user_id TEXT REFERENCES users(id), layout TEXT NOT NULL CHECK(json_valid(layout)),
 updated_at TEXT NOT NULL, updated_by_id TEXT REFERENCES users(id), deleted_at TEXT,
 CHECK((scope='role' AND role IS NOT NULL AND user_id IS NULL) OR
       (scope='user' AND user_id IS NOT NULL AND role IS NULL))
) STRICT;
CREATE UNIQUE INDEX dashboard_role ON dashboard_layouts(role) WHERE scope='role';
CREATE UNIQUE INDEX dashboard_user ON dashboard_layouts(user_id) WHERE scope='user';
CREATE TABLE notification_preferences (
 user_id TEXT NOT NULL REFERENCES users(id),
 event TEXT NOT NULL CHECK(event IN ('stock_low','maintenance_generated','maintenance_overdue','intervention_critical','intervention_assigned')),
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), PRIMARY KEY(user_id,event)
) STRICT;
CREATE TABLE outbox_emails (
 id TEXT PRIMARY KEY NOT NULL, site_id TEXT REFERENCES sites(id),
 to_address TEXT NOT NULL, subject TEXT NOT NULL, body_text TEXT NOT NULL, body_html TEXT NOT NULL,
 event TEXT NOT NULL, dedup_key TEXT, status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','sent','failed')),
 attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_at TEXT NOT NULL, sent_at TEXT, next_attempt_at TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX outbox_dedup ON outbox_emails(dedup_key) WHERE status IN ('pending','sent') AND dedup_key IS NOT NULL;
CREATE INDEX outbox_pending ON outbox_emails(status,next_attempt_at);
CREATE INDEX outbox_site ON outbox_emails(site_id,created_at DESC,id DESC);
CREATE TABLE password_reset_tokens (
 id TEXT PRIMARY KEY NOT NULL, user_id TEXT NOT NULL REFERENCES users(id), token_hash TEXT NOT NULL UNIQUE,
 expires_at TEXT NOT NULL, used_at TEXT, created_at TEXT NOT NULL, requested_ip TEXT
) STRICT;
CREATE INDEX reset_user ON password_reset_tokens(user_id,created_at);
CREATE TABLE password_reset_requests (id TEXT PRIMARY KEY NOT NULL, requested_ip TEXT NOT NULL, created_at TEXT NOT NULL) STRICT;
CREATE INDEX reset_ip ON password_reset_requests(requested_ip,created_at);
ALTER TABLE users ADD COLUMN email TEXT;
CREATE UNIQUE INDEX users_email ON users(email COLLATE NOCASE) WHERE deleted_at IS NULL AND email IS NOT NULL;
CREATE INDEX audit_site_date_id ON audit_log(site_id,created_at DESC,id DESC);
CREATE INDEX audit_date_id ON audit_log(created_at DESC,id DESC);
CREATE INDEX interventions_site_reported ON interventions(site_id,reported_at) WHERE deleted_at IS NULL;
UPDATE app_settings SET value='480' WHERE key='session_timeout_min';
INSERT INTO app_settings(key,value,updated_at) VALUES
 ('public_base_url','',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.host','',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.port','587',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.security','starttls',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.username','',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.password_enc','',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.from_address','',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.from_name','Apex Maint',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('smtp.enabled','false',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('alerts.overdue_digest_hour','7',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('alerts.defaults','{}',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('audit.retention_days','1095',strftime('%Y-%m-%dT%H:%M:%fZ','now'));
INSERT INTO role_permissions(role,permission) VALUES ('responsable','dashboard.customize'),('responsable','exports.run');
