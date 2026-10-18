-- Migration 0003 : Référentiel technique (lignes, équipements, emplacements, fournisseurs)

CREATE TABLE production_lines (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE UNIQUE INDEX idx_production_lines_site_code ON production_lines(site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_production_lines_site_deleted ON production_lines(site_id, deleted_at);

CREATE TABLE equipments (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    production_line_id TEXT REFERENCES production_lines(id),
    parent_id TEXT REFERENCES equipments(id),
    status TEXT NOT NULL DEFAULT 'operational' CHECK (status IN ('operational','degraded','out_of_service')),
    manufacturer TEXT,
    model TEXT,
    serial_number TEXT,
    commissioned_on TEXT,
    description TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE UNIQUE INDEX idx_equipments_site_code ON equipments(site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_equipments_site_line ON equipments(site_id, production_line_id, deleted_at);
CREATE INDEX idx_equipments_site_parent ON equipments(site_id, parent_id, deleted_at);
CREATE INDEX idx_equipments_site_status ON equipments(site_id, status, deleted_at);
CREATE INDEX idx_equipments_site_deleted ON equipments(site_id, deleted_at);

CREATE TABLE locations (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('magasin','allee','rack','bac')),
    parent_id TEXT REFERENCES locations(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE UNIQUE INDEX idx_locations_site_code ON locations(site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_locations_site_kind ON locations(site_id, kind, deleted_at);
CREATE INDEX idx_locations_site_parent ON locations(site_id, parent_id, deleted_at);
CREATE INDEX idx_locations_site_deleted ON locations(site_id, deleted_at);

CREATE TABLE suppliers (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    name TEXT NOT NULL,
    contact_name TEXT,
    email TEXT,
    phone TEXT,
    address TEXT,
    website TEXT,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE UNIQUE INDEX idx_suppliers_site_name ON suppliers(site_id, name COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_suppliers_site_deleted ON suppliers(site_id, deleted_at);
