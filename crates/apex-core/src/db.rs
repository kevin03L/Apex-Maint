//! Ouverture de la base SQLite, migrations embarquées et arrêt propre.

use std::{path::Path, time::Duration};

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::error::MaintError;
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
pub fn schema_version() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

/// Ouvre un pool de connexions SQLite (mode WAL, clés étrangères actives, délai d'attente),
/// puis exécute les migrations embarquées.
pub async fn open(path: Option<&Path>) -> Result<sqlx::SqlitePool, MaintError> {
    let pool = match path {
        Some(p) => {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| {
                    MaintError::DatabaseError(format!(
                        "création du dossier de base de données « {} » impossible : {e}",
                        dir.display()
                    ))
                })?;
            }
            let opts = SqliteConnectOptions::new()
                .filename(p)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5));
            SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(opts)
                .await
                .map_err(|e| {
                    MaintError::DatabaseError(format!(
                        "connexion à la base SQLite impossible : {e}"
                    ))
                })?
        }
        None => {
            // Base en mémoire pour les tests unitaires
            let opts = SqliteConnectOptions::new()
                .filename(":memory:")
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5));
            SqlitePoolOptions::new()
                .max_connections(1)
                .min_connections(1)
                .idle_timeout(None)
                .max_lifetime(None)
                .connect_with(opts)
                .await
                .map_err(|e| {
                    MaintError::DatabaseError(format!("base SQLite en mémoire impossible : {e}"))
                })?
        }
    };

    migrate(&pool).await?;

    Ok(pool)
}

/// Exécute les migrations SQL embarquées sur le pool.
pub async fn migrate(pool: &sqlx::SqlitePool) -> Result<(), MaintError> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|e| MaintError::DatabaseError(format!("échec des migrations : {e}")))?;
    Ok(())
}

/// Ferme proprement le pool SQLite en effectuant un checkpoint complet du journal WAL.
pub async fn close(pool: &sqlx::SqlitePool) {
    match sqlx::query_as::<_, (i64, i64, i64)>("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(pool)
        .await
    {
        Ok((0, _, _)) => {}
        Ok((_, log, done)) => {
            tracing::warn!(
                "Report du journal WAL incomplet à l'arrêt ({done}/{log} pages) : il sera repris au prochain démarrage"
            );
        }
        Err(e) => {
            tracing::warn!("Report du journal WAL dans la base impossible : {e}");
        }
    }
    pool.close().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrate_twice_is_idempotent_and_reads_default_settings() {
        let temp_dir =
            std::env::temp_dir().join(format!("apex-maint-db-test-{}", uuid::Uuid::now_v7()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("test-maint.db");

        // 1. Première ouverture + migrations
        let pool = open(Some(&db_path))
            .await
            .expect("première ouverture de la base");

        // 2. Deuxième passage de migrations (doit être idempotent)
        migrate(&pool)
            .await
            .expect("deuxième exécution des migrations");

        // 3. Lecture des paramètres par défaut
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM app_settings ORDER BY key ASC",
        )
        .fetch_all(&pool)
        .await
        .expect("lecture des paramètres de configuration");

        let settings: std::collections::HashMap<String, String> = rows.into_iter().collect();
        assert_eq!(
            settings.get("app_name").map(String::as_str),
            Some("Apex Maint")
        );
        assert_eq!(
            settings.get("app_subtitle").map(String::as_str),
            Some("GMAO")
        );
        assert_eq!(
            settings.get("stock_alert_threshold").map(String::as_str),
            Some("5")
        );
        assert_eq!(
            settings.get("session_timeout_min").map(String::as_str),
            Some("720")
        );

        // 4. Fermeture propre avec report WAL
        close(&pool).await;

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
