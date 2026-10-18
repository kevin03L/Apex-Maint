-- Phase 1 : authentification, permissions et journal d'audit.
CREATE TABLE users (
 id TEXT PRIMARY KEY NOT NULL,
 site_id TEXT REFERENCES sites(id),
 username TEXT NOT NULL,
 display_name TEXT NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('admin','responsable','production','technicien')),
 password_hash TEXT NOT NULL,
 must_change_password INTEGER NOT NULL DEFAULT 0 CHECK(must_change_password IN (0,1)),
 active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
 failed_attempts INTEGER NOT NULL DEFAULT 0,
 locked_until TEXT,
 last_login_at TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 deleted_at TEXT,
 CHECK(role = 'admin' OR site_id IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX users_username ON users(username COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX users_site ON users(site_id, deleted_at);
CREATE TABLE role_permissions (
 role TEXT NOT NULL CHECK(role IN ('admin','responsable','production','technicien')),
 permission TEXT NOT NULL,
 PRIMARY KEY(role, permission)
) STRICT;
CREATE TABLE sessions (
 token_hash TEXT PRIMARY KEY NOT NULL,
 user_id TEXT NOT NULL REFERENCES users(id),
 created_at TEXT NOT NULL,
 last_seen_at TEXT NOT NULL,
 ip TEXT,
 user_agent TEXT,
 revoked_at TEXT
) STRICT;
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE audit_log (
 id TEXT PRIMARY KEY NOT NULL,
 site_id TEXT,
 user_id TEXT,
 action TEXT NOT NULL,
 entity TEXT NOT NULL,
 entity_id TEXT,
 before_json TEXT,
 after_json TEXT,
 ip TEXT,
 created_at TEXT NOT NULL
) STRICT;
CREATE INDEX audit_site_date ON audit_log(site_id, created_at);
-- Les clés proviennent de Permission::ALL ; l'admin reste implicite.
INSERT INTO role_permissions(role, permission)
SELECT 'responsable', value FROM json_each('["dashboard.view","reporting.view","settings.view","audit.view","equipments.view","equipments.create","equipments.edit","equipments.delete","interventions.view","interventions.create","interventions.edit","interventions.delete","parts.view","parts.create","parts.edit","parts.delete","production_lines.view","production_lines.create","production_lines.edit","production_lines.delete","documents.view","documents.create","documents.delete","locations.view","locations.manage","maintenance.view","maintenance.manage","stock.view","stock.movements","suppliers.view","suppliers.manage","users.view","users.manage"]');
INSERT INTO role_permissions(role, permission)
SELECT 'technicien', value FROM json_each('["dashboard.view","equipments.view","production_lines.view","interventions.view","interventions.create","interventions.edit","parts.view","stock.view","stock.movements","documents.view","documents.create","locations.view","maintenance.view","suppliers.view"]');
INSERT INTO role_permissions(role, permission)
SELECT 'production', value FROM json_each('["dashboard.view","equipments.view","production_lines.view","interventions.view","interventions.create","documents.view","maintenance.view"]');
