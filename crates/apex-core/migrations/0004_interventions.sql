-- Migration 0004 : Interventions de maintenance et techniciens

CREATE TABLE interventions (
    id TEXT PRIMARY KEY NOT NULL,
    site_id TEXT NOT NULL REFERENCES sites(id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    type TEXT NOT NULL CHECK (type IN ('corrective','preventive','a_posteriori','hors_equipement')),
    priority TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low','medium','high','critical')),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','in_progress','completed','cancelled')),
    machine_state TEXT NOT NULL DEFAULT 'FONCTIONNEL' CHECK (machine_state IN ('ARRET','DEGRADE','FONCTIONNEL')),
    equipment_id TEXT REFERENCES equipments(id),
    parent_id TEXT REFERENCES interventions(id),
    reported_by_id TEXT NOT NULL REFERENCES users(id),
    reported_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    duration_minutes INTEGER CHECK (duration_minutes IS NULL OR duration_minutes >= 0),
    solution_report TEXT,
    cancelled_at TEXT,
    cancelled_by_id TEXT REFERENCES users(id),
    cancel_reason TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CHECK ((type = 'hors_equipement' AND equipment_id IS NULL) OR (type != 'hors_equipement' AND equipment_id IS NOT NULL)),
    CHECK (completed_at IS NULL OR started_at IS NULL OR completed_at >= started_at),
    CHECK (status <> 'cancelled' OR (cancel_reason IS NOT NULL AND length(trim(cancel_reason)) > 0))
) STRICT;

CREATE UNIQUE INDEX idx_interventions_site_code ON interventions(site_id, code COLLATE NOCASE) WHERE deleted_at IS NULL;
CREATE INDEX idx_interventions_site_status ON interventions(site_id, status, deleted_at);
CREATE INDEX idx_interventions_site_equipment ON interventions(site_id, equipment_id, deleted_at);
CREATE INDEX idx_interventions_site_parent ON interventions(site_id, parent_id, deleted_at);
CREATE INDEX idx_interventions_site_created ON interventions(site_id, created_at DESC);

CREATE TABLE intervention_technicians (
    intervention_id TEXT NOT NULL REFERENCES interventions(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    is_lead INTEGER NOT NULL DEFAULT 0 CHECK (is_lead IN (0,1)),
    assigned_at TEXT NOT NULL,
    duration_minutes INTEGER CHECK (duration_minutes IS NULL OR duration_minutes >= 0),
    PRIMARY KEY (intervention_id, user_id)
) STRICT;

CREATE INDEX idx_intervention_technicians_user ON intervention_technicians(user_id, intervention_id);
