-- Suppression logique des pièces consommées sur intervention (§C12).
ALTER TABLE intervention_parts ADD COLUMN deleted_at TEXT;
CREATE INDEX IF NOT EXISTS idx_intervention_parts_deleted_at ON intervention_parts(deleted_at);
