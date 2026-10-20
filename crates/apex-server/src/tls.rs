//! TLS intégré, vérification des certificats et renouvellement de l'autorité locale.
use apex_config::model::TlsConfig;
use axum_server::tls_rustls::RustlsConfig;
use chrono::{DateTime, NaiveDateTime, Utc};
use rustls::pki_types::CertificateDer;
use std::{
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
};

fn error(e: impl std::fmt::Display) -> String {
    format!("TLS : {e}")
}
fn path(base: &Path, value: &str) -> PathBuf {
    let p = Path::new(value);
    if p.is_absolute() {
        p.into()
    } else {
        base.join(p)
    }
}

/// Lecture DER bornée de la seule période de validité, après validation structurelle rustls.
fn tlv<'a>(input: &mut &'a [u8]) -> Result<(u8, &'a [u8]), String> {
    if input.len() < 2 {
        return Err(error("Certificat tronqué"));
    }
    let tag = input[0];
    let first = input[1];
    let mut offset = 2;
    let mut size = usize::from(first);
    if first & 128 != 0 {
        let n = usize::from(first & 127);
        if n == 0 || n > 4 || input.len() < 2 + n {
            return Err(error("Longueur DER invalide"));
        }
        size = 0;
        for b in &input[2..2 + n] {
            size = (size << 8) | usize::from(*b);
        }
        offset += n;
    }
    let end = offset
        .checked_add(size)
        .ok_or_else(|| error("Longueur DER invalide"))?;
    if end > input.len() {
        return Err(error("Certificat tronqué"));
    }
    let value = &input[offset..end];
    *input = &input[end..];
    Ok((tag, value))
}
fn date(tag: u8, raw: &[u8]) -> Result<DateTime<Utc>, String> {
    if !raw.is_ascii() {
        return Err(error("Date DER invalide"));
    }
    let value = std::str::from_utf8(raw).map_err(error)?;
    let expanded = if tag == 0x17 && value.len() == 13 {
        let year = value[..2].parse::<u32>().map_err(error)?;
        format!("{}{}", if year >= 50 { "19" } else { "20" }, value)
    } else if tag == 0x18 && value.len() == 15 {
        value.into()
    } else {
        return Err(error("Date DER invalide"));
    };
    Ok(NaiveDateTime::parse_from_str(&expanded, "%Y%m%d%H%M%SZ")
        .map_err(error)?
        .and_utc())
}
pub fn validity(cert: &CertificateDer<'_>) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let (_, mut outer) = tlv(&mut cert.as_ref())?;
    let (_, mut tbs) = tlv(&mut outer)?;
    if tbs.first() == Some(&0xa0) {
        tlv(&mut tbs)?;
    }
    for _ in 0..3 {
        tlv(&mut tbs)?;
    }
    let (tag, mut values) = tlv(&mut tbs)?;
    if tag != 0x30 {
        return Err(error("Validité absente"));
    }
    let (tag, start) = tlv(&mut values)?;
    let start = date(tag, start)?;
    let (tag, end) = tlv(&mut values)?;
    Ok((start, date(tag, end)?))
}

pub fn load(cert: &Path, key: &Path) -> Result<Arc<rustls::ServerConfig>, String> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let chain = rustls_pemfile::certs(&mut BufReader::new(
        std::fs::File::open(cert).map_err(error)?,
    ))
    .collect::<Result<Vec<_>, _>>()
    .map_err(error)?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(
        std::fs::File::open(key).map_err(error)?,
    ))
    .map_err(error)?
    .ok_or_else(|| error("Clé PEM absente"))?;
    for cert in &chain {
        let (start, end) = validity(cert)?;
        if Utc::now() < start || Utc::now() > end {
            return Err(error("Certificat expiré ou pas encore valide"));
        }
    }
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(error)?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

pub fn check(config: &TlsConfig, base: &Path) -> Result<(), String> {
    if config.mode == "files" {
        load(
            &path(base, &config.cert_file),
            &path(base, &config.key_file),
        )?;
    }
    if config.mode == "self_signed" && base.join("tls/server.pem").exists() {
        // Un certificat arrivé en fin de vie sera renouvelé au démarrage.
        let _ = std::fs::read(base.join("tls/ca.pem")).map_err(error)?;
        let _ = std::fs::read(base.join("tls/ca.key")).map_err(error)?;
    }
    Ok(())
}

fn authority_params() -> rcgen::CertificateParams {
    let mut p = rcgen::CertificateParams::default();
    p.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    p.distinguished_name
        .push(rcgen::DnType::CommonName, "Apex Maint - Autorité locale");
    p.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    p.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
    p.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(3650);
    p
}
/// Compare les SAN DNS/IP configurés avec l'extension du certificat existant.
fn same_names(cert: &CertificateDer<'_>, expected: &[rcgen::SanType]) -> Result<bool, String> {
    let (_, mut outer) = tlv(&mut cert.as_ref())?;
    let (_, mut tbs) = tlv(&mut outer)?;
    let mut actual = Vec::new();
    while !tbs.is_empty() {
        let (tag, mut value) = tlv(&mut tbs)?;
        if tag != 0xa3 {
            continue;
        }
        let (_, mut extensions) = tlv(&mut value)?;
        while !extensions.is_empty() {
            let (_, mut extension) = tlv(&mut extensions)?;
            let (_, oid) = tlv(&mut extension)?;
            if oid != [0x55, 0x1d, 0x11] {
                continue;
            }
            let (mut tag, mut value) = tlv(&mut extension)?;
            if tag == 1 {
                (tag, value) = tlv(&mut extension)?;
            }
            if tag != 4 {
                return Ok(false);
            }
            let (_, mut names) = tlv(&mut value)?;
            while !names.is_empty() {
                let (tag, name) = tlv(&mut names)?;
                actual.push((tag, name.to_vec()));
            }
        }
    }
    let mut wanted = Vec::new();
    for name in expected {
        wanted.push(match name {
            rcgen::SanType::DnsName(name) => (0x82, name.as_str().as_bytes().to_vec()),
            rcgen::SanType::IpAddress(std::net::IpAddr::V4(ip)) => (0x87, ip.octets().to_vec()),
            rcgen::SanType::IpAddress(std::net::IpAddr::V6(ip)) => (0x87, ip.octets().to_vec()),
            _ => return Ok(false),
        });
    }
    actual.sort();
    actual.dedup();
    wanted.sort();
    wanted.dedup();
    Ok(actual == wanted)
}

pub fn self_signed(
    config: &TlsConfig,
    base: &Path,
    bind: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let dir = base.join("tls");
    std::fs::create_dir_all(&dir).map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").map_err(error)?;
        let output = std::process::Command::new("icacls")
            .arg(&dir)
            .args([
                "/inheritance:r",
                "/grant:r",
                "*S-1-5-18:(OI)(CI)F",
                "*S-1-5-32-544:(OI)(CI)F",
                &format!("{user}:(OI)(CI)F"),
            ])
            .output()
            .map_err(error)?;
        if !output.status.success() {
            return Err(error("Restriction des droits du dossier TLS refusée"));
        }
    }
    let ca_key_path = dir.join("ca.key");
    let ca_key = if ca_key_path.exists() {
        rcgen::KeyPair::from_pem(&std::fs::read_to_string(&ca_key_path).map_err(error)?)
            .map_err(error)?
    } else {
        if dir.join("ca.pem").exists() {
            return Err(error(
                "Clé de l'autorité absente ; remplacement automatique refusé",
            ));
        }
        let key = rcgen::KeyPair::generate().map_err(error)?;
        crate::app::write_private_file(&ca_key_path, key.serialize_pem().as_bytes())
            .map_err(error)?;
        key
    };
    // Le sujet et la clé restent identiques à l'autorité distribuée aux appareils.
    let ca = authority_params().self_signed(&ca_key).map_err(error)?;
    if !dir.join("ca.pem").exists() {
        std::fs::write(dir.join("ca.pem"), ca.pem()).map_err(error)?;
    }
    load(&dir.join("ca.pem"), &ca_key_path)?;
    let cert = dir.join("server.pem");
    let key = dir.join("server.key");
    let mut names = config.self_signed_names.clone();
    if names.is_empty() {
        names = vec!["localhost".into(), "127.0.0.1".into(), "::1".into()];
        if let Ok(host) = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")) {
            use std::net::ToSocketAddrs;
            if let Ok(addresses) = (host.as_str(), 0).to_socket_addrs() {
                names.extend(addresses.map(|a| a.ip().to_string()));
            }
            names.push(host);
        }
        if bind
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| !ip.is_unspecified())
        {
            names.push(bind.into());
        }
    }
    names.sort();
    names.dedup();
    let mut params = rcgen::CertificateParams::new(names).map_err(error)?;

    let renew = load(&cert, &key)
        .ok()
        .and_then(|_| {
            let data = std::fs::read(&cert).ok()?;
            let der = rustls_pemfile::certs(&mut data.as_slice()).next()?.ok()?;
            validity(&der).ok().map(|(_, end)| {
                end <= Utc::now() + chrono::Duration::days(30)
                    || !same_names(&der, &params.subject_alt_names).unwrap_or(false)
            })
        })
        .unwrap_or(true);
    if renew {
        params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(730);
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = rcgen::KeyPair::generate().map_err(error)?;
        let server = params.signed_by(&server_key, &ca, &ca_key).map_err(error)?;
        crate::app::write_private_file(&key, server_key.serialize_pem().as_bytes())
            .map_err(error)?;
        std::fs::write(
            &cert,
            format!(
                "{}{}",
                server.pem(),
                std::fs::read_to_string(dir.join("ca.pem")).map_err(error)?
            ),
        )
        .map_err(error)?;
    }
    Ok((cert, key))
}

pub fn prepare(
    config: &TlsConfig,
    base: &Path,
    bind: &str,
) -> Result<Option<(RustlsConfig, PathBuf, PathBuf)>, String> {
    if config.mode == "off" {
        return Ok(None);
    }
    let (cert, key) = if config.mode == "self_signed" {
        self_signed(config, base, bind)?
    } else {
        (path(base, &config.cert_file), path(base, &config.key_file))
    };
    Ok(Some((
        RustlsConfig::from_config(load(&cert, &key)?),
        cert,
        key,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparaison_san_dns_et_ip_sans_acces_disque() {
        let key = rcgen::KeyPair::generate().unwrap();
        let params = rcgen::CertificateParams::new(vec![
            "atelier.local".into(),
            "127.0.0.1".into(),
            "::1".into(),
        ])
        .unwrap();
        let mut names = params.subject_alt_names.clone();
        let cert = params.self_signed(&key).unwrap();
        names.reverse();
        assert!(same_names(cert.der(), &names).unwrap());
        names.pop();
        assert!(!same_names(cert.der(), &names).unwrap());
        names.push(rcgen::SanType::DnsName("nouveau.local".try_into().unwrap()));
        assert!(!same_names(cert.der(), &names).unwrap());
    }
    #[test]
    fn noms_modifies_renouvellent_le_serveur_sans_changer_autorite() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = TlsConfig {
            self_signed_names: vec!["localhost".into(), "127.0.0.1".into()],
            ..Default::default()
        };
        let (cert, _) = self_signed(&config, dir.path(), "127.0.0.1").unwrap();
        let original = std::fs::read(&cert).unwrap();
        let ca = std::fs::read(dir.path().join("tls/ca.pem")).unwrap();
        config.self_signed_names.reverse();
        self_signed(&config, dir.path(), "127.0.0.1").unwrap();
        assert_eq!(std::fs::read(&cert).unwrap(), original);
        config.self_signed_names.push("atelier.local".into());
        self_signed(&config, dir.path(), "127.0.0.1").unwrap();
        assert_ne!(std::fs::read(&cert).unwrap(), original);
        assert_eq!(std::fs::read(dir.path().join("tls/ca.pem")).unwrap(), ca);
        config.self_signed_names.retain(|n| n != "atelier.local");
        let changed = std::fs::read(&cert).unwrap();
        self_signed(&config, dir.path(), "127.0.0.1").unwrap();
        assert_ne!(std::fs::read(&cert).unwrap(), changed);
    }
    #[test]
    fn certificats_absents_expires_et_cle_incompatible() {
        let dir = tempfile::tempdir().unwrap();
        let cert = dir.path().join("cert.pem");
        let key = dir.path().join("key.pem");
        assert!(load(&cert, &key).is_err());
        let pair = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap();
        params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(365);
        params.not_after = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        std::fs::write(&cert, params.self_signed(&pair).unwrap().pem()).unwrap();
        std::fs::write(&key, pair.serialize_pem()).unwrap();
        assert!(load(&cert, &key).unwrap_err().contains("expiré"));
        let params = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap();
        std::fs::write(&cert, params.self_signed(&pair).unwrap().pem()).unwrap();
        assert!(load(&cert, &key).is_ok());
        let config = RustlsConfig::from_config(load(&cert, &key).unwrap());
        let old = config.get_inner();
        std::fs::write(&key, rcgen::KeyPair::generate().unwrap().serialize_pem()).unwrap();
        assert!(load(&cert, &key).is_err());
        assert!(Arc::ptr_eq(&old, &config.get_inner()));
        std::fs::write(&key, pair.serialize_pem()).unwrap();
        config.reload_from_config(load(&cert, &key).unwrap());
        assert!(!Arc::ptr_eq(&old, &config.get_inner()));
    }
}
