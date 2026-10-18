-- Phase 6 : documents versionnés, stockages et formats d'étiquettes.
CREATE TABLE storage_backends (
 id TEXT PRIMARY KEY NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('folder','s3')),
 label TEXT NOT NULL CHECK(length(trim(label)) > 0),
 config TEXT NOT NULL CHECK(json_valid(config)),
 secret_enc TEXT,
 is_active INTEGER NOT NULL DEFAULT 0 CHECK(is_active IN (0,1)),
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 deleted_at TEXT
) STRICT;
CREATE UNIQUE INDEX storage_active ON storage_backends(is_active) WHERE is_active=1;
INSERT INTO storage_backends(id,kind,label,config,is_active,created_at,updated_at)
VALUES('0199f000-0000-7000-8000-000000000007','folder','Dossier par défaut','{"path":""}',1,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'));
CREATE TABLE documents (
 id TEXT PRIMARY KEY NOT NULL,
 site_id TEXT NOT NULL REFERENCES sites(id),
 entity_type TEXT NOT NULL CHECK(entity_type IN ('equipment','intervention','part','maintenance_plan')),
 entity_id TEXT NOT NULL,
 title TEXT NOT NULL CHECK(length(trim(title)) > 0),
 category TEXT,
 description TEXT,
 current_version_id TEXT REFERENCES document_versions(id),
 versions_count INTEGER NOT NULL DEFAULT 0 CHECK(versions_count >= 0),
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 deleted_at TEXT
) STRICT;
CREATE INDEX documents_entity ON documents(site_id,entity_type,entity_id,deleted_at);
CREATE INDEX documents_search ON documents(site_id,id,deleted_at);
CREATE TABLE document_versions (
 id TEXT PRIMARY KEY NOT NULL,
 site_id TEXT NOT NULL REFERENCES sites(id),
 document_id TEXT NOT NULL REFERENCES documents(id),
 version_no INTEGER NOT NULL CHECK(version_no > 0),
 storage_id TEXT NOT NULL REFERENCES storage_backends(id),
 storage_key TEXT NOT NULL,
 original_name TEXT NOT NULL,
 mime_type TEXT NOT NULL,
 extension TEXT NOT NULL,
 size_bytes INTEGER NOT NULL CHECK(size_bytes > 0),
 sha256 TEXT NOT NULL,
 comment TEXT,
 uploaded_by_id TEXT NOT NULL REFERENCES users(id),
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL,
 deleted_at TEXT,
 UNIQUE(document_id,version_no)
) STRICT;
CREATE INDEX versions_document ON document_versions(site_id,document_id,version_no);
INSERT INTO app_settings(key,value,updated_at) VALUES
 ('documents.allowed_types','["pdf","jpeg","png","webp","docx","doc","xlsx","xls","dwg","dxf","step"]',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('documents.max_size_mb','50',strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 ('labels.format','{"mode":"sheet","sheet":{"page":"A4","columns":3,"rows":8,"margin_top_mm":5,"margin_left_mm":5,"gap_horizontal_mm":0,"gap_vertical_mm":0},"single":{"width_mm":70,"height_mm":37},"qr_size_mm":20,"show_code":true,"show_name":true,"show_site":false,"show_production_line":false}',strftime('%Y-%m-%dT%H:%M:%fZ','now'));
