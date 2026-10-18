//! Hachage Argon2id et jetons opaques de session.
use std::sync::{Arc, LazyLock};

use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::MaintError;

static DUMMY: LazyLock<String> =
    LazyLock::new(|| hash_password(&generate_token()).expect("hachage factice"));
static CHECKS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));

pub fn prepare_dummy() {
    LazyLock::force(&DUMMY);
}

pub fn hash_password(plain: &str) -> Result<String, MaintError> {
    Argon2::default()
        .hash_password(plain.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(|_| MaintError::Internal("Hachage impossible".into()))
}

pub fn verify_password(plain: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(plain.as_bytes(), &h)
            .is_ok()
    })
}

pub async fn verify(plain: String, hash: Option<String>) -> Result<bool, MaintError> {
    let permit = Arc::clone(&CHECKS)
        .acquire_owned()
        .await
        .map_err(|_| MaintError::Unavailable("Vérification indisponible".into()))?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let valid = hash.as_deref().and_then(|h| PasswordHash::new(h).ok());
        match valid {
            Some(h) => Argon2::default()
                .verify_password(plain.as_bytes(), &h)
                .is_ok(),
            None => {
                let _ = verify_password(&plain, &DUMMY);
                false
            }
        }
    })
    .await
    .map_err(|_| MaintError::Internal("Vérification interrompue".into()));
    result
}

pub async fn hash(plain: String) -> Result<String, MaintError> {
    let permit = Arc::clone(&CHECKS)
        .acquire_owned()
        .await
        .map_err(|_| MaintError::Unavailable("Hachage indisponible".into()))?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password(&plain)
    })
    .await
    .map_err(|_| MaintError::Internal("Hachage interrompu".into()))?;
    result
}

pub fn validate_new_password(new: &str, old: Option<&str>) -> Result<(), MaintError> {
    if new.chars().count() < 8 || new.len() > 256 {
        return Err(MaintError::BadRequest(
            "Le mot de passe doit contenir au moins 8 caractères et au plus 256 octets".into(),
        ));
    }
    if new == "admin123" || old == Some(new) {
        return Err(MaintError::BadRequest(
            "Le mot de passe doit différer de l'ancien et du mot de passe d'usine".into(),
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn generate_token() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn passwords_and_tokens() {
        let h = hash_password("secret123").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("secret123".into(), Some(h.clone())).await.unwrap());
        assert!(!verify("faux".into(), Some(h)).await.unwrap());
        assert!(!verify("faux".into(), None).await.unwrap());
        assert!(validate_new_password("admin123", None).is_err());
        assert!(validate_new_password("secret123", Some("secret123")).is_err());
        assert_eq!(generate_token().len(), 64);
        assert_ne!(generate_token(), generate_token());
    }
}
