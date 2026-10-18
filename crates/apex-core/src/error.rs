//! Erreurs métier du moteur Apex Maint.

use thiserror::Error;

/// Erreur métier d'Apex Maint, portant le code HTTP approprié.
#[derive(Debug, Error)]
pub enum MaintError {
    #[error("{0}")]
    PayloadTooLarge(String),
    #[error("{0}")]
    TooManyRequests(String),
    #[error("Ressource introuvable : {0}")]
    NotFound(String),

    #[error("Accès refusé : {0}")]
    Forbidden(String),

    #[error("Authentification requise : {0}")]
    Unauthorized(String),

    #[error("Requête invalide : {0}")]
    BadRequest(String),

    #[error("Conflit : {0}")]
    Conflict(String),

    #[error("Erreur de base de données : {0}")]
    DatabaseError(String),

    #[error("Service indisponible : {0}")]
    Unavailable(String),

    #[error("Erreur interne : {0}")]
    Internal(String),
}

impl MaintError {
    /// Code d'état HTTP correspondant à l'erreur.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::PayloadTooLarge(_) => 413,
            Self::TooManyRequests(_) => 429,
            Self::NotFound(_) => 404,
            Self::Forbidden(_) => 403,
            Self::Unauthorized(_) => 401,
            Self::BadRequest(_) => 400,
            Self::Conflict(_) => 409,
            Self::DatabaseError(_) => 500,
            Self::Unavailable(_) => 503,
            Self::Internal(_) => 500,
        }
    }
}

impl From<sqlx::Error> for MaintError {
    fn from(e: sqlx::Error) -> Self {
        if e.as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            return Self::Conflict("Cet identifiant existe déjà".into());
        }
        Self::DatabaseError(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_mapping() {
        assert_eq!(
            MaintError::PayloadTooLarge("test".into()).http_status(),
            413
        );
        assert_eq!(
            MaintError::TooManyRequests("test".into()).http_status(),
            429
        );
        assert_eq!(MaintError::NotFound("test".into()).http_status(), 404);
        assert_eq!(MaintError::Forbidden("test".into()).http_status(), 403);
        assert_eq!(MaintError::Unauthorized("test".into()).http_status(), 401);
        assert_eq!(MaintError::BadRequest("test".into()).http_status(), 400);
        assert_eq!(MaintError::Conflict("test".into()).http_status(), 409);
        assert_eq!(MaintError::DatabaseError("test".into()).http_status(), 500);
        assert_eq!(MaintError::Unavailable("test".into()).http_status(), 503);
        assert_eq!(MaintError::Internal("test".into()).http_status(), 500);
    }
}
