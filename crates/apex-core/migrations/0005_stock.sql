-- Migration 0005 : Pièces détachées, stocks, mouvements, interventions et réservations (Phase 4)

CREATE TABLE parts (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    reference TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    category TEXT,
    unit TEXT NOT NULL DEFAULT 'piece' CHECK (unit IN ('piece','metre','litre','kg','lot','boite')),
    unit_price_cents INTEGER NOT NULL DEFAULT 0 CHECK (unit_price_cents >= 0),
    supplier_id TEXT REFERENCES suppliers(id),
    supplier_reference TEXT,
    manufacturer TEXT,
    manufacturer_reference TEXT,
    barcode TEXT,
    alert_threshold INTEGER CHECK (alert_threshold IS NULL OR alert_threshold >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE UNIQUE INDEX idx_parts_site_ref ON parts(site_id, reference COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_parts_site_supplier ON parts(site_id, supplier_id, deleted_at);
CREATE INDEX idx_parts_site_category ON parts(site_id, category, deleted_at);
CREATE INDEX idx_parts_site_deleted ON parts(site_id, deleted_at);

CREATE TABLE stock (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    part_id TEXT NOT NULL REFERENCES parts(id),
    location_id TEXT NOT NULL REFERENCES locations(id),
    quantity INTEGER NOT NULL DEFAULT 0 CHECK (quantity >= 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX idx_stock_part_location ON stock(part_id, location_id);
CREATE INDEX idx_stock_site_part ON stock(site_id, part_id);
CREATE INDEX idx_stock_site_location ON stock(site_id, location_id);

CREATE TABLE stock_movements (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    part_id TEXT NOT NULL REFERENCES parts(id),
    movement_type TEXT NOT NULL CHECK (movement_type IN ('in','out','transfer','adjustment')),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    from_location_id TEXT REFERENCES locations(id),
    to_location_id TEXT REFERENCES locations(id),
    intervention_id TEXT REFERENCES interventions(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    unit_price_cents INTEGER CHECK (unit_price_cents IS NULL OR unit_price_cents >= 0),
    reason TEXT,
    adjustment_kind TEXT CHECK (adjustment_kind IS NULL OR adjustment_kind IN ('inventaire','rebut','perte','casse')),
    delta INTEGER,
    created_at TEXT NOT NULL,
    CHECK (movement_type != 'out' OR intervention_id IS NOT NULL),
    CHECK (movement_type != 'in' OR (from_location_id IS NULL AND to_location_id IS NOT NULL)),
    CHECK (movement_type != 'out' OR (from_location_id IS NOT NULL AND to_location_id IS NULL)),
    CHECK (movement_type != 'transfer' OR (from_location_id IS NOT NULL AND to_location_id IS NOT NULL AND from_location_id != to_location_id)),
    CHECK (movement_type <> 'adjustment' OR (reason IS NOT NULL AND adjustment_kind IS NOT NULL AND delta IS NOT NULL AND delta != 0)),
    CHECK (movement_type <> 'adjustment' OR ((delta > 0 AND from_location_id IS NULL AND to_location_id IS NOT NULL) OR (delta < 0 AND from_location_id IS NOT NULL AND to_location_id IS NULL)))
) STRICT;

CREATE INDEX idx_stock_movements_site_part_created ON stock_movements(site_id, part_id, created_at DESC);
CREATE INDEX idx_stock_movements_intervention ON stock_movements(intervention_id) WHERE intervention_id IS NOT NULL;
CREATE INDEX idx_stock_movements_site_created ON stock_movements(site_id, created_at DESC);

CREATE TABLE intervention_parts (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    intervention_id TEXT NOT NULL REFERENCES interventions(id),
    part_id TEXT NOT NULL REFERENCES parts(id),
    location_id TEXT NOT NULL REFERENCES locations(id),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    unit_price_cents INTEGER NOT NULL CHECK (unit_price_cents >= 0),
    movement_id TEXT NOT NULL REFERENCES stock_movements(id),
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX idx_intervention_parts_intervention ON intervention_parts(intervention_id);
CREATE INDEX idx_intervention_parts_part ON intervention_parts(part_id);

CREATE TABLE equipment_parts (
    equipment_id TEXT NOT NULL REFERENCES equipments(id),
    part_id TEXT NOT NULL REFERENCES parts(id),
    quantity_recommended INTEGER NOT NULL DEFAULT 1 CHECK (quantity_recommended > 0),
    notes TEXT,
    PRIMARY KEY (equipment_id, part_id)
) STRICT;

CREATE INDEX idx_equipment_parts_part ON equipment_parts(part_id);

CREATE TABLE stock_reservations (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    intervention_id TEXT NOT NULL REFERENCES interventions(id),
    part_id TEXT NOT NULL REFERENCES parts(id),
    location_id TEXT NOT NULL REFERENCES locations(id),
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    status TEXT NOT NULL CHECK (status IN ('prepared','consumed','released')),
    prepared_by_id TEXT NOT NULL REFERENCES users(id),
    prepared_at TEXT NOT NULL,
    closed_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX idx_stock_reservations_site_part_loc_status ON stock_reservations(site_id, part_id, location_id, status);
CREATE INDEX idx_stock_reservations_intervention ON stock_reservations(intervention_id, status);

INSERT OR IGNORE INTO role_permissions(role, permission) VALUES ('responsable', 'stock.adjust');
