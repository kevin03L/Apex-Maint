-- Chaque remise à zéro ouvre un nouveau cycle sans supprimer les occurrences historiques.
ALTER TABLE maintenance_plans ADD COLUMN counter_cycle INTEGER NOT NULL DEFAULT 0;
ALTER TABLE maintenance_occurrences ADD COLUMN counter_cycle INTEGER;
UPDATE maintenance_occurrences SET counter_cycle = 0;
DROP INDEX idx_maintenance_occurrences_plan_counter;
CREATE UNIQUE INDEX idx_maintenance_occurrences_plan_counter
ON maintenance_occurrences(plan_id, counter_cycle, due_counter)
WHERE due_counter IS NOT NULL AND deleted_at IS NULL;
CREATE TRIGGER maintenance_occurrence_cycle AFTER INSERT ON maintenance_occurrences
WHEN NEW.counter_cycle IS NULL
BEGIN
    UPDATE maintenance_occurrences SET counter_cycle = (
        SELECT counter_cycle FROM maintenance_plans WHERE id = NEW.plan_id AND site_id = NEW.site_id
    ) WHERE id = NEW.id AND site_id = NEW.site_id;
END;

-- La date affichée reste celle du début d'état ; l'ordre utilise le dernier événement accepté.
ALTER TABLE equipment_external_status ADD COLUMN last_event_at TEXT;
UPDATE equipment_external_status SET last_event_at = since;
