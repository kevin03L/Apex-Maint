//! Outillage des tests d'intégration « boîte noire » du moteur `apex-maint`.
//!
//! Chaque test lance le binaire compilé (`CARGO_BIN_EXE_apex-maint`) :
//! - dans un dossier temporaire unique (sous `CARGO_TARGET_TMPDIR`), en mode portable ;
//! - avec un fichier `--config` écrit par le test ;
//! - sur des ports libres choisis dynamiquement (jamais 3000 ni 3100) ;
//! - avec un environnement nettoyé (`RUST_LOG`, `APEX_MAINT_CONFIG`...).
//!
//! Le dialogue passe par un client HTTP/1.1 minimal écrit sur `std::net` :
//! aucune dépendance supplémentaire. Les gardes `Drop` tuent le processus et suppriment
//! le dossier, y compris en cas de panique.

use std::{
    collections::HashSet,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use serde_json::Value;

/// Binaire du moteur compilé par Cargo pour ces tests.
pub const EXE: &str = env!("CARGO_BIN_EXE_apex-maint");

/// Délai maximal de démarrage.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
/// Délai maximal d'arrêt d'un processus.
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Délai d'attente d'une réponse réseau.
const IO_TIMEOUT: Duration = Duration::from_secs(15);

// ============================================================================
// DOSSIER TEMPORAIRE
// ============================================================================

pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(name: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("apex-it")
            .join(format!("{name}-{}-{n}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("création du dossier temporaire");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.path).is_ok() || !self.path.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

// ============================================================================
// PORTS LIBRES (JAMAIS 3000 NI 3100)
// ============================================================================

static ALLOCATED_PORTS: Mutex<Option<HashSet<u16>>> = Mutex::new(None);

pub fn free_port() -> u16 {
    let mut guard = ALLOCATED_PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let used = guard.get_or_insert_with(HashSet::new);
    loop {
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("attribution d'un port libre")
            .port();
        if port != 3000 && port != 3100 && used.insert(port) {
            return port;
        }
    }
}

// ============================================================================
// CONFIGURATION
// ============================================================================

#[derive(Debug, Clone)]
pub struct Settings {
    pub port: u16,
    pub demo: bool,
}

impl Settings {
    pub fn new() -> Self {
        Self {
            port: free_port(),
            demo: false,
        }
    }

    pub fn render(&self) -> String {
        let web_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("static");
        format!(
            "config_version = 1\n\
             \n\
             [instance]\n\
             name = \"Essai d'intégration\"\n\
             \n\
             [server]\n\
             bind = \"127.0.0.1\"\n\
             port = {port}\n\
             \n\
             [paths]\n\
             web_dir = '{web}'\n\
             \n\
             [logging]\n\
             level = \"info\"\n\
             file_enabled = true\n\
             retention_files = 2\n\
             \n\
             [demo]\n\
             enabled = {demo}\n",
            port = self.port,
            web = web_dir.display(),
            demo = self.demo,
        )
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

pub fn write_config(dir: &Path, content: &str) -> PathBuf {
    let file = dir.join("apex-maint.toml");
    std::fs::write(&file, content).expect("écriture du fichier de configuration");
    file
}

// ============================================================================
// PROCESSUS
// ============================================================================

pub fn spawn_engine(dir: &Path, config: &Path, prefix: &str) -> Child {
    let stdout = std::fs::File::create(dir.join(format!("{prefix}.stdout.log")))
        .expect("fichier de sortie standard");
    let stderr = std::fs::File::create(dir.join(format!("{prefix}.stderr.log")))
        .expect("fichier de sortie d'erreur");
    let mut cmd = Command::new(EXE);
    cmd.arg("run")
        .arg("--config")
        .arg(config)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    for (key, _) in std::env::vars_os() {
        let k = key.to_string_lossy().to_uppercase();
        if k.starts_with("APEX_") || k == "RUST_LOG" || k == "INVOCATION_ID" {
            cmd.env_remove(&key);
        }
    }
    cmd.spawn().expect("lancement du binaire apex-maint")
}

pub fn wait_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => return None,
        }
    }
}

pub fn read_text(path: &Path) -> String {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

pub fn read_log_dir(dir: &Path) -> String {
    let mut out = String::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("logs")) {
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            out.push_str(&read_text(&f));
        }
    }
    out
}

pub struct Outcome {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_to_exit(dir: &Path, config: &Path, prefix: &str) -> Outcome {
    let mut child = ChildGuard(Some(spawn_engine(dir, config, prefix)));
    let status = wait_timeout(child.get(), EXIT_TIMEOUT);
    child.kill();
    let outcome = Outcome {
        code: status.and_then(|s| s.code()),
        stdout: read_text(&dir.join(format!("{prefix}.stdout.log"))),
        stderr: read_text(&dir.join(format!("{prefix}.stderr.log"))),
    };
    assert!(
        status.is_some(),
        "le moteur ne s'est pas arrêté en {EXIT_TIMEOUT:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        outcome.stdout,
        outcome.stderr
    );
    outcome
}

pub struct ChildGuard(pub Option<Child>);

impl ChildGuard {
    pub fn get(&mut self) -> &mut Child {
        self.0.as_mut().expect("processus présent")
    }

    pub fn kill(&mut self) {
        if let Some(mut c) = self.0.take() {
            if let Ok(None) = c.try_wait() {
                let _ = c.kill();
            }
            let _ = c.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

// ============================================================================
// INSTANCE EN MARCHE
// ============================================================================

pub struct Server {
    child: ChildGuard,
    pub settings: Settings,
    pub config: PathBuf,
    dir: TempDir,
}

impl Server {
    pub fn start(name: &str) -> Self {
        Self::start_with(name, Settings::new())
    }

    pub fn start_with(name: &str, settings: Settings) -> Self {
        let dir = TempDir::new(name);
        let config = write_config(&dir.path, &settings.render());
        let child = ChildGuard(Some(spawn_engine(&dir.path, &config, "engine")));
        let mut server = Server {
            child,
            settings,
            config,
            dir,
        };
        server.wait_ready();
        server
    }

    pub fn dir(&self) -> &Path {
        &self.dir.path
    }

    pub fn port(&self) -> u16 {
        self.settings.port
    }

    pub fn pid(&mut self) -> u32 {
        self.child.get().id()
    }

    fn wait_ready(&mut self) {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Ok(Some(status)) = self.child.get().try_wait() {
                panic!(
                    "le moteur s'est arrêté au démarrage ({status})\n{}",
                    self.logs()
                );
            }
            if let Ok(r) = request(self.port(), "GET", "/api/health", None, None) {
                if r.status == 200 {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "le moteur n'a pas répondu en {STARTUP_TIMEOUT:?}\n{}",
                self.logs()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn logs(&self) -> String {
        format!(
            "--- stdout ---\n{}\n--- stderr ---\n{}",
            read_text(&self.dir.path.join("engine.stdout.log")),
            read_text(&self.dir.path.join("engine.stderr.log")),
        )
    }

    pub fn control_token(&self) -> String {
        let file = self.dir.path.join("run").join("control.token");
        let token = read_text(&file).trim().to_string();
        assert!(
            !token.is_empty(),
            "jeton de contrôle absent : {}",
            file.display()
        );
        token
    }

    pub fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Response {
        request(self.port(), method, path, token, body).unwrap_or_else(|e| {
            panic!("{method} {path} : erreur réseau {e}\n{}", self.logs());
        })
    }

    pub fn call_with_headers(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Response {
        request_with_headers(self.port(), method, path, token, None, headers).unwrap_or_else(|e| {
            panic!("{method} {path} : erreur réseau {e}\n{}", self.logs());
        })
    }

    pub fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        wait_timeout(self.child.get(), timeout)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill();
        if std::thread::panicking() {
            eprintln!(
                "=== journaux du moteur ({}) ===\n{}\n--- logs/ ---\n{}",
                self.dir.path.display(),
                self.logs(),
                read_log_dir(&self.dir.path)
            );
        }
    }
}

// ============================================================================
// CLIENT HTTP/1.1 MINIMAL
// ============================================================================

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("réponse JSON attendue ({e}) : {}", self.text()))
    }
}

fn connect(port: u16) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), IO_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

pub fn request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> io::Result<Response> {
    request_with_headers(port, method, path, token, body, &[])
}

pub fn request_with_headers(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
    headers: &[(&str, &str)],
) -> io::Result<Response> {
    let mut stream = connect(port)?;
    let payload = body.map(|b| b.to_string()).unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if let Some(t) = token {
        head.push_str(&format!("X-Maint-Token: {t}\r\n"));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if body.is_some() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if body.is_some() || method != "GET" {
        head.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(payload.as_bytes())?;
    stream.flush()?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

type Head = (u16, Vec<(String, String)>, usize);

fn parse_head(raw: &[u8]) -> io::Result<Head> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| invalid("en-têtes HTTP incomplets"))?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| invalid("en-têtes non UTF-8"))?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| invalid("ligne d'état HTTP invalide"))?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok((status, headers, end + 4))
}

fn parse_response(raw: &[u8]) -> io::Result<Response> {
    let (status, headers, body_start) = parse_head(raw)?;
    let body_raw = &raw[body_start..];
    let is_chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked"));
    let body = if is_chunked {
        decode_chunked(body_raw)?
    } else {
        body_raw.to_vec()
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn decode_chunked(mut raw: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = raw
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| invalid("fin de taille de tronçon introuvable"))?;
        let size_str = std::str::from_utf8(&raw[..line_end])
            .map_err(|_| invalid("taille de tronçon non UTF-8"))?
            .trim();
        let size = usize::from_str_radix(size_str, 16)
            .map_err(|_| invalid("taille de tronçon hexadécimale invalide"))?;
        raw = &raw[line_end + 2..];
        if size == 0 {
            break;
        }
        if raw.len() < size + 2 {
            return Err(invalid("tronçon incomplet"));
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
    Ok(out)
}
