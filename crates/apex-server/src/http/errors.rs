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

/// Seul un corps réellement vide autorise les valeurs par défaut.
pub async fn optional_body<T: serde::de::DeserializeOwned>(
    req: axum::extract::Request,
) -> Result<Option<T>> {
    use axum::extract::FromRequest;
    let (parts, stream) = req.into_parts();
    let bytes = axum::body::Bytes::from_request(
        axum::extract::Request::from_parts(parts.clone(), stream),
        &(),
    )
    .await
    .map_err(|e| {
        ApiError(if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            MaintError::PayloadTooLarge("Corps JSON trop volumineux".into())
        } else {
            MaintError::BadRequest("Corps JSON illisible".into())
        })
    })?;
    if bytes.is_empty() {
        return Ok(None);
    }
    body(
        Json::<T>::from_request(
            axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes)),
            &(),
        )
        .await,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn corps_facultatif_invalide_refuse_avant_traitement() {
        use axum::{
            body::Body,
            extract::DefaultBodyLimit,
            http::{Request, StatusCode},
            routing::post,
            Router,
        };
        use tower::ServiceExt;
        let router = Router::new()
            .route(
                "/consume",
                post(|req| async {
                    super::optional_body::<apex_core::stock::ConsumeReservationInput>(req).await?;
                    Ok::<_, super::ApiError>(StatusCode::NO_CONTENT)
                }),
            )
            .route(
                "/take",
                post(|req| async {
                    super::optional_body::<apex_core::interventions::TakeIntervention>(req).await?;
                    Ok::<_, super::ApiError>(StatusCode::NO_CONTENT)
                }),
            )
            .layer(DefaultBodyLimit::max(65536));
        for path in ["/consume", "/take"] {
            for input in [
                "{".to_string(),
                "null".into(),
                if path == "/consume" {
                    r#"{"quantity":"2"}"#.into()
                } else {
                    r#"{"technician_ids":2}"#.into()
                },
                "x".repeat(65537),
            ] {
                let expected = if input.len() > 65536 {
                    StatusCode::PAYLOAD_TOO_LARGE
                } else {
                    StatusCode::BAD_REQUEST
                };
                let request = Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(input))
                    .unwrap();
                assert_eq!(
                    router.clone().oneshot(request).await.unwrap().status(),
                    expected
                );
            }
        }
    }

    #[tokio::test]
    async fn corps_facultatif_vide_et_objet_vide_conservent_les_defauts() {
        use axum::{body::Body, http::Request};
        for input in ["", "{}"] {
            let req = Request::builder()
                .header("content-type", "application/json")
                .body(Body::from(input))
                .unwrap();
            let value = super::optional_body::<apex_core::stock::ConsumeReservationInput>(req)
                .await
                .unwrap_or_else(|_| panic!("Corps valide refusé"));
            assert_eq!(value.and_then(|v| v.quantity), None);
        }
    }
}
