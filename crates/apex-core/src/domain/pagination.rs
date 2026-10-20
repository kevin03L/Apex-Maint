//! Pagination par curseur (keyset pagination) pour les grandes listes.
//!
//! Contrairement à une pagination par offset (`LIMIT / OFFSET`), la pagination par curseur
//! reste stable face aux insertions simultanées et ne dégrade pas les performances
//! sur les grands volumes. Le curseur est encodé sous forme opaque en URL (base64url).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::MaintError;

/// Curseur opaque pointant vers un élément de la liste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Identifiant de l'élément (ex. UUIDv7).
    pub id: String,
    /// Valeur de tri optionnelle (ex. horodatage RFC 3339 pour tri temporel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Valeur de tri textuelle (ex. code ou nom pour tri alphabétique).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_value: Option<String>,
}

impl Cursor {
    /// Crée un curseur simple basé sur l'identifiant.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            timestamp: None,
            sort_value: None,
        }
    }

    /// Crée un curseur composite avec identifiant et horodatage.
    pub fn with_timestamp(id: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            timestamp: Some(timestamp.into()),
            sort_value: None,
        }
    }

    /// Crée un curseur composite avec identifiant et valeur de tri textuelle.
    pub fn with_sort_value(id: impl Into<String>, sort_value: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            timestamp: None,
            sort_value: Some(sort_value.into()),
        }
    }

    /// Encode le curseur sous forme de chaîne opaque en base64url.
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("sérialisation du curseur");
        URL_SAFE_NO_PAD.encode(json)
    }

    /// Décode un curseur à partir de sa représentation opaque.
    pub fn decode(s: &str) -> Result<Self, MaintError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(MaintError::BadRequest(
                "Le curseur de pagination ne peut pas être vide".into(),
            ));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(trimmed)
            .map_err(|e| MaintError::BadRequest(format!("Curseur de pagination invalide : {e}")))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| MaintError::BadRequest(format!("Curseur de pagination corrompu : {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_encode_decode_simple() {
        let cursor = Cursor::new("018d9f52-1234-7890-abcd-ef0123456789");
        let encoded = cursor.encode();
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));

        let decoded = Cursor::decode(&encoded).expect("décodage réussi");
        assert_eq!(decoded, cursor);
    }

    #[test]
    fn cursor_encode_decode_with_timestamp() {
        let cursor = Cursor::with_timestamp(
            "018d9f52-1234-7890-abcd-ef0123456789",
            "2026-10-18T12:00:00.000Z",
        );
        let encoded = cursor.encode();
        let decoded = Cursor::decode(&encoded).expect("décodage réussi");
        assert_eq!(decoded, cursor);
    }

    #[test]
    fn cursor_encode_decode_with_sort_value() {
        let cursor = Cursor::with_sort_value("018d9f52-1234-7890-abcd-ef0123456789", "LIGNE-01");
        let encoded = cursor.encode();
        let decoded = Cursor::decode(&encoded).expect("décodage réussi");
        assert_eq!(decoded, cursor);
    }

    #[test]
    fn invalid_cursors_rejected() {
        assert!(Cursor::decode("").is_err());
        assert!(Cursor::decode("    ").is_err());
        assert!(Cursor::decode("!!!pas-du-base64???").is_err());
        // Base64 valide mais pas du JSON de curseur
        let bad_json = URL_SAFE_NO_PAD.encode(b"ce n'est pas du json");
        assert!(Cursor::decode(&bad_json).is_err());
    }
}
