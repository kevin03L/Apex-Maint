-- Migration 0006 : Maintenance préventive, plannings, compteurs et occurrences

CREATE TABLE equipment_counters (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    equipment_id TEXT NOT NULL REFERENCES equipments(id),
    name TEXT NOT NULL,
    unit TEXT NOT NULL,
    current_value INTEGER NOT NULL DEFAULT 0 CHECK (current_value >= 0),
    last_reading_at TEXT,
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE UNIQUE INDEX idx_equipment_counters_name ON equipment_counters(equipment_id, name COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_equipment_counters_site ON equipment_counters(site_id, deleted_at);
CREATE INDEX idx_equipment_counters_equipment ON equipment_counters(site_id, equipment_id, deleted_at);

CREATE TABLE counter_readings (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    counter_id TEXT NOT NULL REFERENCES equipment_counters(id),
    value INTEGER NOT NULL CHECK (value >= 0),
    read_at TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('manual','supervision')),
    is_reset INTEGER NOT NULL DEFAULT 0 CHECK (is_reset IN (0,1)),
    recorded_by_id TEXT REFERENCES users(id),
    note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

CREATE INDEX idx_counter_readings_counter ON counter_readings(counter_id, read_at DESC);
CREATE INDEX idx_counter_readings_site ON counter_readings(site_id, deleted_at);

CREATE TABLE maintenance_plans (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    equipment_id TEXT NOT NULL REFERENCES equipments(id),
    description TEXT,
    instructions TEXT,
    priority TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low','medium','high','critical')),
    frequency TEXT NOT NULL CHECK (frequency IN ('daily','weekly','monthly','quarterly','yearly','interval_days','counter')),
    interval_days INTEGER CHECK (interval_days IS NULL OR interval_days BETWEEN 1 AND 3650),
    counter_id TEXT REFERENCES equipment_counters(id),
    counter_interval INTEGER CHECK (counter_interval IS NULL OR counter_interval > 0),
    anchor_date TEXT,
    next_due_date TEXT,
    anchor_counter INTEGER,
    next_due_counter INTEGER,
    lead_days INTEGER NOT NULL DEFAULT 7 CHECK (lead_days BETWEEN 0 AND 365),
    lead_counter INTEGER NOT NULL DEFAULT 0 CHECK (lead_counter >= 0),
    auto_generate INTEGER NOT NULL DEFAULT 1 CHECK (auto_generate IN (0,1)),
    active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0,1)),
    estimated_minutes INTEGER CHECK (estimated_minutes IS NULL OR estimated_minutes >= 0),
    default_technician_id TEXT REFERENCES users(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CHECK (
        (frequency = 'counter' AND counter_id IS NOT NULL AND counter_interval IS NOT NULL AND interval_days IS NULL AND anchor_date IS NULL)
        OR (frequency = 'interval_days' AND interval_days IS NOT NULL AND counter_id IS NULL AND counter_interval IS NULL AND anchor_counter IS NULL)
        OR (frequency IN ('daily','weekly','monthly','quarterly','yearly') AND interval_days IS NULL AND counter_id IS NULL AND counter_interval IS NULL AND anchor_counter IS NULL)
    )
) STRICT;

CREATE UNIQUE INDEX idx_maintenance_plans_site_code ON maintenance_plans(site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_maintenance_plans_site_equipment ON maintenance_plans(site_id, equipment_id, deleted_at);
CREATE INDEX idx_maintenance_plans_site_counter ON maintenance_plans(site_id, counter_id, deleted_at);
CREATE INDEX idx_maintenance_plans_site_status ON maintenance_plans(site_id, active, deleted_at);

CREATE TABLE maintenance_occurrences (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    plan_id TEXT NOT NULL REFERENCES maintenance_plans(id),
    due_date TEXT,
    due_counter INTEGER,
    status TEXT NOT NULL CHECK (status IN ('generated','done','skipped','missed')),
    intervention_id TEXT REFERENCES interventions(id),
    generated_at TEXT,
    closed_at TEXT,
    generation TEXT CHECK (generation IS NULL OR generation IN ('auto','manual')),
    skip_reason TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CHECK (status <> 'skipped' OR (skip_reason IS NOT NULL AND length(trim(skip_reason)) > 0))
) STRICT;

CREATE UNIQUE INDEX idx_maintenance_occurrences_plan_date ON maintenance_occurrences(plan_id, due_date) WHERE due_date IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX idx_maintenance_occurrences_plan_counter ON maintenance_occurrences(plan_id, due_counter) WHERE due_counter IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX idx_maintenance_occurrences_single_generated ON maintenance_occurrences(plan_id) WHERE status = 'generated' AND deleted_at IS NULL;
CREATE INDEX idx_maintenance_occurrences_site_status ON maintenance_occurrences(site_id, status, deleted_at);
CREATE INDEX idx_maintenance_occurrences_intervention ON maintenance_occurrences(intervention_id);

ALTER TABLE interventions ADD COLUMN maintenance_occurrence_id TEXT REFERENCES maintenance_occurrences(id);
CREATE INDEX idx_interventions_occurrence ON interventions(maintenance_occurrence_id);

INSERT OR IGNORE INTO role_permissions(role, permission) VALUES
    ('responsable', 'counters.record'),
    ('technicien', 'counters.record'),
    ('production', 'counters.record');
