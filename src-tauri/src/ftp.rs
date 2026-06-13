use crate::connections::{Connection, ConnectionKind, ConnectionSecret};
use crate::error::{AppError, Error, Result};
use crate::logging;
use crate::remote::{Caps, Remote};
use crate::s3::{emit_transfer_error, ObjectEntry, TransferEvent};
use crate::taxonomy::{
    category_for_ftp, ftp_retryable, Connector, ErrorCategory, Level, Phase, StatusCode,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;
use crate::connections::FtpMode;
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream};
use suppaftp::{FtpError, Mode};
use tauri::{AppHandle, Emitter};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

/// Project a `suppaftp::FtpError` onto the [`AppError`] IR. The FTP reply code
/// (e.g. 530, 550) drives the category; `phase` says where we were.
fn ftp_err(connector: Connector, phase: Phase, e: &FtpError) -> Error {
    let (category, code) = match e {
        FtpError::ConnectionError(_) => (ErrorCategory::Network, None),
        FtpError::SecureError(_) => (ErrorCategory::Tls, None),
        FtpError::UnexpectedResponse(resp) => {
            let c = resp.status.code() as u16;
            (category_for_ftp(c), Some(c))
        }
        FtpError::BadResponse => (ErrorCategory::Protocol, None),
        FtpError::InvalidAddress(_) => (ErrorCategory::Config, None),
        FtpError::DataConnectionAlreadyOpen => (ErrorCategory::Protocol, None),
    };

    let retryable = code.map(ftp_retryable).unwrap_or_else(|| category.retryable());
    let mut err = AppError::new(category, format!("FTP {phase:?} failed: {e}"))
        .connector(connector)
        .phase(phase)
        .retryable(retryable)
        .detail(format!("{e:?}"));
    if let Some(c) = code {
        err = err.code(StatusCode::Ftp(c));
    }
    Error::App(err)
}

/// FTP / FTPS backend over a single live control connection, reused across
/// operations. FTP is a stateful, single-stream protocol, so the stream is
/// serialized behind a `Mutex` — operations on one connection never overlap.
pub struct FtpBackend {
    c: Connection,
    ftp: Mutex<AsyncRustlsFtpStream>,
}

impl FtpBackend {
    /// Connect, authenticate, and set the data-connection mode — done once; the
    /// control connection then stays open until `disconnect()` or a drop.
    pub async fn connect(c: &Connection, s: &ConnectionSecret) -> Result<Self> {
        let ftp = open(c, s).await?;
        Ok(Self { c: c.clone(), ftp: Mutex::new(ftp) })
    }
}

#[async_trait]
impl Remote for FtpBackend {
    fn caps(&self) -> Caps {
        Caps { multipart: false, resume: false, virtual_buckets: false }
    }

    async fn test(&self) -> Result<String> {
        check(&mut *self.ftp.lock().await, &self.c).await
    }

    async fn list(&self, path: &str) -> Result<Vec<ObjectEntry>> {
        list(&mut *self.ftp.lock().await, &self.c, path).await
    }

    async fn search(
        &self,
        path: &str,
        query: &str,
        limit: usize,
        max_depth: usize,
    ) -> Result<Vec<ObjectEntry>> {
        search(&mut *self.ftp.lock().await, &self.c, path, query, limit, max_depth).await
    }

    async fn download(
        &self,
        app: &AppHandle,
        remote: &str,
        dest: &Path,
        transfer_id: String,
    ) -> Result<()> {
        download(&mut *self.ftp.lock().await, app, &self.c, remote, dest, transfer_id).await
    }

    async fn upload_file(
        &self,
        app: &AppHandle,
        src: &Path,
        remote: &str,
        transfer_id: String,
    ) -> Result<()> {
        upload_file(&mut *self.ftp.lock().await, app, &self.c, src, remote, transfer_id).await
    }

    async fn upload_dir(
        &self,
        app: &AppHandle,
        remote_base: &str,
        files: &[(PathBuf, String)],
    ) -> Result<usize> {
        upload_dir(&mut *self.ftp.lock().await, app, &self.c, remote_base, files).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        delete(&mut *self.ftp.lock().await, &self.c, path).await
    }

    async fn disconnect(&self) {
        // Best-effort graceful QUIT; the socket closes on drop regardless.
        let _ = self.ftp.lock().await.quit().await;
    }
}

/// Log which data-connection mode the next transfer/listing will use. Called
/// before each operation that opens a data connection (LIST / RETR / STOR);
/// the raw `227`/`229` reply with the resolved address comes through the
/// protocol log bridge.
fn log_data_mode(c: &Connection) {
    let mode = match c.ftp_mode.unwrap_or_default() {
        FtpMode::Active => "active",
        FtpMode::Passive => "passive",
        FtpMode::ExtendedPassive => "extended passive",
    };
    logging::emit_global(
        Level::Debug,
        c.kind.into(),
        Phase::Passive,
        Some(&c.name),
        format!("Using {mode} mode for data connection"),
    );
}

fn rustls_config() -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn open(c: &Connection, s: &ConnectionSecret) -> Result<AsyncRustlsFtpStream> {
    let connector: Connector = c.kind.into();
    // The UI stores unset optional fields as "" (not null); treat empty as missing.
    let host = c.host.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
        Error::App(
            AppError::new(ErrorCategory::Config, "FTP: missing host")
                .connector(connector)
                .phase(Phase::Config),
        )
    })?;
    // Implicit FTPS wraps the control channel in TLS from the first byte and
    // conventionally listens on 990; explicit FTP/FTPS connects in the clear on 21.
    let implicit = matches!(c.kind, ConnectionKind::Ftps) && c.ftps_implicit;
    let port = c.port.unwrap_or(if implicit { 990 } else { 21 });
    let user = c
        .username
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anonymous".into());
    let pass = s.password.clone().unwrap_or_default();

    let tls_desc = if implicit {
        "implicit TLS"
    } else if matches!(c.kind, ConnectionKind::Ftps) {
        "explicit TLS"
    } else {
        "plain FTP"
    };
    logging::emit_global(
        Level::Debug,
        connector,
        Phase::Connect,
        Some(&c.name),
        format!("Connecting to {host}:{port} ({tls_desc})"),
    );

    let addr = format!("{host}:{port}");
    let mut ftp = if implicit {
        let tls = tokio_rustls::TlsConnector::from(rustls_config());
        let ftp = AsyncRustlsFtpStream::connect_secure_implicit(&addr, AsyncRustlsConnector::from(tls), &host)
            .await
            .map_err(|e| ftp_err(connector, Phase::Tls, &e))?;
        logging::emit_global(
            Level::Debug,
            connector,
            Phase::Tls,
            Some(&c.name),
            "TLS channel established",
        );
        ftp
    } else {
        let ftp = AsyncRustlsFtpStream::connect(&addr)
            .await
            .map_err(|e| ftp_err(connector, Phase::Connect, &e))?;
        // Explicit FTPS: upgrade the existing plaintext channel via AUTH TLS.
        if matches!(c.kind, ConnectionKind::Ftps) {
            logging::emit_global(
                Level::Debug,
                connector,
                Phase::Tls,
                Some(&c.name),
                "Negotiating TLS (AUTH TLS)",
            );
            let tls = tokio_rustls::TlsConnector::from(rustls_config());
            let ftp = ftp
                .into_secure(AsyncRustlsConnector::from(tls), &host)
                .await
                .map_err(|e| ftp_err(connector, Phase::Tls, &e))?;
            logging::emit_global(
                Level::Debug,
                connector,
                Phase::Tls,
                Some(&c.name),
                "TLS channel established",
            );
            ftp
        } else {
            ftp
        }
    };

    if let Some(banner) = ftp.get_welcome_msg() {
        logging::emit_global(
            Level::Debug,
            connector,
            Phase::Connect,
            Some(&c.name),
            format!("Server banner: {}", banner.trim()),
        );
    }

    logging::emit_global(
        Level::Debug,
        connector,
        Phase::Auth,
        Some(&c.name),
        format!("Logging in as {user}"),
    );
    ftp.login(&user, &pass)
        .await
        .map_err(|e| ftp_err(connector, Phase::Auth, &e))?;
    logging::emit_global(
        Level::Info,
        connector,
        Phase::Auth,
        Some(&c.name),
        format!("Logged in as {user}"),
    );
    ftp.set_mode(match c.ftp_mode.unwrap_or_default() {
        FtpMode::Active => Mode::Active,
        FtpMode::Passive => Mode::Passive,
        FtpMode::ExtendedPassive => Mode::ExtendedPassive,
    });
    Ok(ftp)
}

/// Probe a live connection (used by the Test button and the pooled `test()`).
pub async fn check(ftp: &mut AsyncRustlsFtpStream, c: &Connection) -> Result<String> {
    let connector: Connector = c.kind.into();
    let pwd = ftp
        .pwd()
        .await
        .map_err(|e| ftp_err(connector, Phase::List, &e))?;
    Ok(format!("OK — connected, cwd={pwd}"))
}

pub async fn list(
    ftp: &mut AsyncRustlsFtpStream,
    c: &Connection,
    path: &str,
) -> Result<Vec<ObjectEntry>> {
    let connector: Connector = c.kind.into();
    let target = if path.is_empty() { None } else { Some(path) };
    log_data_mode(c);
    let lines = ftp
        .list(target)
        .await
        .map_err(|e| ftp_err(connector, Phase::List, &e))?;

    let mut out = Vec::new();
    for line in lines {
        if let Ok(f) = suppaftp::list::ListParser::parse_posix(&line) {
            let name = f.name().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let is_dir = f.is_directory();
            let size = if is_dir { None } else { Some(f.size() as i64) };
            let modified: Option<DateTime<Utc>> = {
                let st: SystemTime = f.modified();
                st.duration_since(SystemTime::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0))
            };
            let full = if path.is_empty() || path == "/" {
                name.clone()
            } else {
                format!("{}/{}", path.trim_end_matches('/'), name)
            };
            out.push(ObjectEntry {
                key: full,
                name,
                kind: if is_dir { "folder".into() } else { "file".into() },
                size,
                modified,
                etag: None,
            });
        }
    }
    out.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
        ("folder", "file") => std::cmp::Ordering::Less,
        ("file", "folder") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

/// Recursively walk from `root` via repeated LIST, returning entries whose name
/// contains `query` (case-insensitive). `name` is the path relative to `root`.
/// Bounded by `max_depth` and `limit` — FTP is chatty, one round-trip per dir.
pub async fn search(
    ftp: &mut AsyncRustlsFtpStream,
    c: &Connection,
    root: &str,
    query: &str,
    limit: usize,
    max_depth: usize,
) -> Result<Vec<ObjectEntry>> {
    let needle = query.to_lowercase();
    let start = if root.is_empty() { "/".to_string() } else { root.to_string() };

    let mut out = Vec::new();
    // (absolute dir, relative dir, depth)
    let mut stack: Vec<(String, String, usize)> = vec![(start, String::new(), 0)];

    log_data_mode(c);
    while let Some((abs_dir, rel_dir, depth)) = stack.pop() {
        let lines = match ftp.list(Some(&abs_dir)).await {
            Ok(l) => l,
            Err(_) => continue, // skip unreadable dirs rather than aborting
        };
        for line in lines {
            let Ok(f) = suppaftp::list::ListParser::parse_posix(&line) else { continue };
            let name = f.name().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let is_dir = f.is_directory();
            let abs_child = if abs_dir == "/" {
                format!("/{name}")
            } else {
                format!("{}/{}", abs_dir.trim_end_matches('/'), name)
            };
            let rel_child = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            if name.to_lowercase().contains(&needle) {
                let modified: Option<DateTime<Utc>> = {
                    let st: SystemTime = f.modified();
                    st.duration_since(SystemTime::UNIX_EPOCH)
                        .ok()
                        .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0))
                };
                out.push(ObjectEntry {
                    key: abs_child.clone(),
                    name: rel_child.clone(),
                    kind: if is_dir { "folder".into() } else { "file".into() },
                    size: if is_dir { None } else { Some(f.size() as i64) },
                    modified,
                    etag: None,
                });
                if out.len() >= limit {
                    return Ok(out);
                }
            }
            if is_dir && depth + 1 < max_depth {
                stack.push((abs_child, rel_child, depth + 1));
            }
        }
    }
    Ok(out)
}

pub async fn download(
    ftp: &mut AsyncRustlsFtpStream,
    app: &AppHandle,
    c: &Connection,
    remote: &str,
    dest: &Path,
    transfer_id: String,
) -> Result<()> {
    let connector: Connector = c.kind.into();
    let res: Result<()> = async {
        let total = ftp.size(remote).await.unwrap_or(0) as u64;

        let _ = app.emit(
            "transfer",
            TransferEvent::Start {
                id: transfer_id.clone(),
                name: remote.rsplit('/').next().unwrap_or(remote).to_string(),
                total,
                direction: "down".into(),
            },
        );

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut local = File::create(dest).await?;
        log_data_mode(c);
        let mut stream = ftp
            .retr_as_stream(remote)
            .await
            .map_err(|e| ftp_err(connector, Phase::Transfer, &e))?;
        let mut transferred: u64 = 0;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = stream
                .read(&mut buf)
                .await
                .map_err(Error::from)?;
            if n == 0 {
                break;
            }
            local.write_all(&buf[..n]).await?;
            transferred += n as u64;
            let _ = app.emit(
                "transfer",
                TransferEvent::Progress {
                    id: transfer_id.clone(),
                    transferred,
                    total: if total == 0 { transferred } else { total },
                },
            );
        }
        local.flush().await?;
        ftp.finalize_retr_stream(stream)
            .await
            .map_err(|e| ftp_err(connector, Phase::Transfer, &e))?;
        let _ = app.emit("transfer", TransferEvent::Done { id: transfer_id.clone() });
        Ok(())
    }
    .await;

    if let Err(e) = &res {
        emit_transfer_error(app, &transfer_id, e);
    }
    res
}

pub async fn upload_file(
    ftp: &mut AsyncRustlsFtpStream,
    app: &AppHandle,
    c: &Connection,
    src: &Path,
    remote: &str,
    transfer_id: String,
) -> Result<()> {
    let connector: Connector = c.kind.into();
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("upload").to_string();
    log_data_mode(c);
    put_file(ftp, app, connector, src, remote, &name, transfer_id).await
}

/// Upload every file under `files` (abs path, path relative to the upload root)
/// into `remote_base`, reusing one connection and creating remote dirs as needed.
pub async fn upload_dir(
    ftp: &mut AsyncRustlsFtpStream,
    app: &AppHandle,
    c: &Connection,
    remote_base: &str,
    files: &[(PathBuf, String)],
) -> Result<usize> {
    let connector: Connector = c.kind.into();
    let mut created: HashSet<String> = HashSet::new();
    ensure_dir(ftp, &mut created, remote_base).await;

    log_data_mode(c);
    let mut count = 0;
    for (abs, rel) in files {
        let remote = join_remote(remote_base, rel);
        if let Some((parent, _)) = remote.rsplit_once('/') {
            if !parent.is_empty() {
                ensure_dir(ftp, &mut created, parent).await;
            }
        }
        put_file(ftp, app, connector, abs, &remote, rel, Uuid::new_v4().to_string()).await?;
        count += 1;
    }
    Ok(count)
}

/// Create `dir` and all its ancestors (mkdir -p). Errors are ignored — an
/// already-existing dir is fine; a truly unwritable path surfaces on STOR.
async fn ensure_dir(ftp: &mut AsyncRustlsFtpStream, created: &mut HashSet<String>, dir: &str) {
    let absolute = dir.starts_with('/');
    let mut cur = String::new();
    for comp in dir.split('/').filter(|c| !c.is_empty()) {
        cur = if cur.is_empty() {
            if absolute { format!("/{comp}") } else { comp.to_string() }
        } else {
            format!("{cur}/{comp}")
        };
        if !created.insert(cur.clone()) {
            continue;
        }
        let _ = ftp.mkdir(&cur).await;
    }
}

fn join_remote(base: &str, rel: &str) -> String {
    if base.is_empty() || base == "/" {
        format!("/{rel}")
    } else {
        format!("{}/{}", base.trim_end_matches('/'), rel)
    }
}

async fn put_file(
    ftp: &mut AsyncRustlsFtpStream,
    app: &AppHandle,
    connector: Connector,
    src: &Path,
    remote: &str,
    name: &str,
    transfer_id: String,
) -> Result<()> {
    let res: Result<()> = async {
        let meta = tokio::fs::metadata(src).await?;
        let total = meta.len();

        let _ = app.emit(
            "transfer",
            TransferEvent::Start {
                id: transfer_id.clone(),
                name: name.to_string(),
                total,
                direction: "up".into(),
            },
        );

        let mut local = File::open(src).await?;
        let mut stream = ftp
            .put_with_stream(remote)
            .await
            .map_err(|e| ftp_err(connector, Phase::Transfer, &e))?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut transferred: u64 = 0;
        loop {
            let n = local.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            stream
                .write_all(&buf[..n])
                .await
                .map_err(Error::from)?;
            transferred += n as u64;
            let _ = app.emit(
                "transfer",
                TransferEvent::Progress {
                    id: transfer_id.clone(),
                    transferred,
                    total,
                },
            );
        }
        stream.flush().await.ok();
        ftp.finalize_put_stream(stream)
            .await
            .map_err(|e| ftp_err(connector, Phase::Transfer, &e))?;
        let _ = app.emit("transfer", TransferEvent::Done { id: transfer_id.clone() });
        Ok(())
    }
    .await;

    if let Err(e) = &res {
        emit_transfer_error(app, &transfer_id, e);
    }
    res
}

pub async fn delete(ftp: &mut AsyncRustlsFtpStream, c: &Connection, path: &str) -> Result<()> {
    let connector: Connector = c.kind.into();
    if ftp.rm(path).await.is_err() {
        ftp.rmdir(path)
            .await
            .map_err(|e| ftp_err(connector, Phase::Delete, &e))?;
    }
    Ok(())
}

#[cfg(test)]
mod live {
    //! Live, read-only integration tests against public test.rebex.net (FTP +
    //! explicit FTPS). Ignored by default (network); run with:
    //!   cargo test --lib ftp::live -- --ignored --nocapture
    use super::*;
    use crate::connections::ConnectionKind;

    /// The app installs the rustls CryptoProvider at startup; tests must too,
    /// or FTPS TLS setup panics. Idempotent (ignores "already installed").
    fn ensure_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn conn(kind: ConnectionKind, user: &str) -> Connection {
        Connection {
            id: "t".into(),
            name: "rebex".into(),
            kind,
            host: Some("test.rebex.net".into()),
            port: Some(21),
            region: None,
            endpoint: None,
            bucket: None,
            default_path: None,
            username: Some(user.into()),
            use_path_style: false,
            ftp_mode: None,
            ftps_implicit: false,
            color: "#fff".into(),
            created_at: None,
        }
    }
    fn pw(p: &str) -> ConnectionSecret {
        ConnectionSecret { password: Some(p.into()), ..Default::default() }
    }

    #[tokio::test]
    #[ignore = "hits public test.rebex.net"]
    async fn ftps_connect_and_list() {
        ensure_crypto();
        let c = conn(ConnectionKind::Ftps, "demo");
        let s = pw("password");
        // Exercises the explicit-FTPS AUTH TLS handshake against a real CA cert.
        let b = FtpBackend::connect(&c, &s).await.expect("FTPS connect should succeed");
        let msg = b.test().await.expect("FTPS probe should succeed");
        assert!(msg.contains("OK"), "unexpected: {msg}");
        let entries = b.list("").await.expect("FTPS list should succeed");
        assert!(!entries.is_empty(), "root listing should not be empty");
    }

    #[tokio::test]
    #[ignore = "hits public test.rebex.net"]
    async fn ftps_wrong_password_is_auth() {
        ensure_crypto();
        let err = FtpBackend::connect(&conn(ConnectionKind::Ftps, "demo"), &pw("definitely-wrong"))
            .await
            .err()
            .expect("auth should fail");
        let a = err.to_app();
        assert_eq!(a.category, ErrorCategory::Auth, "got {a:?}");
    }

    #[tokio::test]
    #[ignore = "hits public test.rebex.net"]
    async fn ftp_plain_connect() {
        ensure_crypto();
        let b = FtpBackend::connect(&conn(ConnectionKind::Ftp, "demo"), &pw("password"))
            .await
            .expect("plain FTP connect should succeed");
        let msg = b.test().await.expect("plain FTP probe should succeed");
        assert!(msg.contains("OK"), "unexpected: {msg}");
    }
}
