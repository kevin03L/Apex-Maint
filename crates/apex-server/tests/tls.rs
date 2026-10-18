//! Démarrage HTTPS auto-signé sur instance portable et confiance dans l'autorité publiée.
#[allow(dead_code)]
mod common;
use std::{
    io::{Read, Write},
    sync::Arc,
    time::{Duration, Instant},
};
struct Process(std::process::Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(
    port: u16,
    ca: &std::path::Path,
    path: &str,
    method: &str,
    token: Option<&str>,
) -> Result<String, String> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let pem = std::fs::read(ca).map_err(|e| e.to_string())?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
        roots
            .add(cert.map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    let client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connection =
        rustls::ClientConnection::new(Arc::new(client), "localhost".try_into().unwrap())
            .map_err(|e| e.to_string())?;
    let stream = std::net::TcpStream::connect_timeout(
        &([127, 0, 0, 1], port).into(),
        Duration::from_secs(2),
    )
    .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut stream = rustls::StreamOwned::new(connection, stream);
    let header = token
        .map(|v| format!("X-Maint-Token: {v}\r\n"))
        .unwrap_or_default();
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\n{header}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).map_err(|e|e.to_string())?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
#[test]
fn autorite_generee_connexion_verifiee_et_telechargement() {
    let dir = common::TempDir::new("tls");
    let settings = common::Settings::new();
    let config=common::write_config(&dir.path,&format!("{}\n[server.tls]\nmode = 'self_signed'\nself_signed_names = ['localhost','127.0.0.1']\n",settings.render()));
    let mut child = Process(common::spawn_engine(&dir.path, &config, "tls"));
    let ca = dir.path.join("data/tls/ca.pem");
    let deadline = Instant::now() + Duration::from_secs(40);
    let result = loop {
        match request(settings.port, &ca, "/api/health", "GET", None) {
            Ok(response) => break response,
            Err(e) if Instant::now() >= deadline => {
                let _ = child.0.kill();
                panic!("Démarrage TLS : {e}");
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    assert!(result.starts_with("HTTP/1.1 200"), "{result}");
    assert!(result
        .to_lowercase()
        .contains("strict-transport-security: max-age=86400"));
    let pem = request(settings.port, &ca, "/api/tls/ca.pem", "GET", None).unwrap();
    assert!(pem.contains("BEGIN CERTIFICATE"));
    assert!(!pem.contains("PRIVATE KEY"));
    assert!(dir.path.join("data/tls/ca.key").is_file());
    let token = std::fs::read_to_string(dir.path.join("run/control.token")).unwrap();
    assert!(request(
        settings.port,
        &ca,
        "/api/system/shutdown",
        "POST",
        Some(token.trim())
    )
    .unwrap()
    .starts_with("HTTP/1.1 200"));
    let _ = child.0.kill();
    let _ = child.0.wait();
}
