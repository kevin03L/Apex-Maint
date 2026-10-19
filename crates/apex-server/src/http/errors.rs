//! Gestion unifiée des erreurs HTTP et du décodage des corps JSON.

use apex_core::MaintError;
use axum::{
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub struct ApiError(pub MaintError);

impl From<MaintError> for ApiError {
    fn from(e: MaintError) -> Self {
        Self(e)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self(e.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = if status.is_server_error() {
            tracing::error!("API : {}", self.0);
            "Erreur interne du serveur".to_string()
        } else {
            match &self.0 {
                MaintError::Unauthorized(message) => message.clone(),
                _ => self.0.to_string(),
            }
        };
        (status, Json(json!({"error": true, "message": message}))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;
pub type Body<T> = std::result::Result<Json<T>, JsonRejection>;

pub fn body<T>(input: Body<T>) -> Result<T> {
    input.map(|Json(v)| v).map_err(|e| {
        ApiError(if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            MaintError::PayloadTooLarge("Corps JSON trop volumineux (64 Kio maximum)".into())
        } else {
            MaintError::BadRequest("Corps JSON invalide ou champs inconnus".into())
        })
    })
}
