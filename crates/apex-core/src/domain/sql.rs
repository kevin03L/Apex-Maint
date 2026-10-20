//! Utilitaires SQL communs (limites, validation de site, échappement LIKE).

use sqlx::SqliteConnection;

use crate::error::MaintError;

/// Valide une limite de pagination (entre 1 et 200).
pub fn validate_limit(limit: u32) -> Result<u32, MaintError> {
    if (1..=200).contains(&limit) {
        Ok(limit)
    } else {
        Err(MaintError::BadRequest(
            "La limite de pagination doit être comprise entre 1 et 200".into(),
        ))
    }
}

/// Vérifie qu'un site existe et n'est pas supprimé logiquement.
pub async fn ensure_site_exists(
    conn: &mut SqliteConnection,
    site_id: &str,
) -> Result<(), MaintError> {
    let exists: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sites WHERE id = ? AND deleted_at IS NULL")
            .bind(site_id)
            .fetch_one(conn)
            .await?;

    if exists == 0 {
        return Err(MaintError::BadRequest("Site introuvable".into()));
    }
    Ok(())
}

/// Motif `LIKE` « contient » où `%`, `_` et `\` saisis sont pris littéralement
/// (à utiliser avec `ESCAPE '\'`).
pub fn like_pattern(q: &str) -> String {
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}
