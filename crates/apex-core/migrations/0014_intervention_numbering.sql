-- La numérotation consulte aussi les interventions supprimées logiquement.
CREATE INDEX idx_interventions_site_code_history ON interventions(site_id, code);
