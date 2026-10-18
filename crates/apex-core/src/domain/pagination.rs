//! Pagination par curseur (keyset pagination) pour les grandes listes.
//!
//! Contrairement à une pagination par offset (`LIMIT / OFFSET`), la pagination par curseur
//! reste stable face aux insertions simultanées et ne dégrade pas les performances
//! sur les grands volumes. Le curseur est encodé sous forme opaque en URL (base64url).

use serde::{Deserialize, Serialize};

use crate::error::MaintError;

/// Curseur opaque pointant vers un élément de la liste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Identifiant de l'élément (ex. UUIDv7).
    pub id: String,
    /// Valeur de tri optionnelle (ex. horodatage RFC 3339 pour tri temporel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl Cursor {
    /// Crée un curseur simple basé sur l'identifiant.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            timestamp: None,
        }
    }

    /// Crée un curseur composite avec identifiant et horodatage.
    pub fn with_timestamp(id: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            timestamp: Some(timestamp.into()),
        }
    }

    /// Encode le curseur sous forme de chaîne opaque en base64url.
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("sérialisation du curseur");
        b64_encode(&json)
    }

    /// Décode un curseur à partir de sa représentation opaque.
    pub fn decode(s: &str) -> Result<Self, MaintError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(MaintError::BadRequest(
                "Le curseur de pagination ne peut pas être vide".into(),
            ));
        }
        let bytes = b64_decode(trimmed)
            .map_err(|e| MaintError::BadRequest(format!("Curseur de pagination invalide : {e}")))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| MaintError::BadRequest(format!("Curseur de pagination corrompu : {e}")))
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn b64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[(triple >> 18) & 0x3F] as char);
        out.push(ALPHABET[(triple >> 12) & 0x3F] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(triple >> 6) & 0x3F] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[triple & 0x3F] as char);
        }
    }
    out
}

fn b64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn decode_char(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'-' => Ok(62),
            b'_' => Ok(63),
            other => Err(format!("caractère non autorisé « {} »", other as char)),
        }
    }

    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity((bytes.len() * 3) / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() == 1 {
            return Err("longueur de bloc invalide".into());
        }
        let c0 = decode_char(chunk[0])? as u32;
        let c1 = decode_char(chunk[1])? as u32;
        let c2 = if chunk.len() > 2 {
            decode_char(chunk[2])? as u32
        } else {
            0
        };
        let c3 = if chunk.len() > 3 {
            decode_char(chunk[3])? as u32
        } else {
            0
        };

        let triple = (c0 << 18) | (c1 << 12) | (c2 << 6) | c3;

        out.push(((triple >> 16) & 0xFF) as u8);
        if chunk.len() > 2 {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk.len() > 3 {
            out.push((triple & 0xFF) as u8);
        }
    }
    Ok(out)
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
    fn invalid_cursors_rejected() {
        assert!(Cursor::decode("").is_err());
        assert!(Cursor::decode("    ").is_err());
        assert!(Cursor::decode("!!!pas-du-base64???").is_err());
        // Base64 valide mais pas du JSON de curseur
        let bad_json = b64_encode(b"ce n'est pas du json");
        assert!(Cursor::decode(&bad_json).is_err());
    }
}
