use crate::connections::{Connection, ConnectionSecret};
use crate::error::{AppError, Error, Result};
use crate::logging::{self, LogBuilder};
use crate::remote::{Caps, Remote};
use crate::taxonomy::{category_for_sftp, Connector, ErrorCategory, Level, Phase};
use async_trait::async_trait;
use chrono::DateTime;
use russh::client;
use russh::keys::PublicKey;
use russh_sftp::client::SftpSession;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::s3::{emit_transfer_error, ObjectEntry, TransferEvent};

/// Build a classified SFTP error. russh-sftp exposes SSH_FX_* status only as a
/// stringified message, so we classify from the text with the `phase` as the
/// fallback (e.g. a failure during `Phase::Auth` with no clearer text → Auth).
fn sftp_err(phase: Phase, e: impl std::fmt::Display) -> Error {
    let msg = e.to_string();
    let category = category_for_sftp(phase, &msg);
    Error::App(
        AppError::new(category, format!("SFTP {phase:?} failed: {msg}"))
            .connector(Connector::Sftp)
            .phase(phase)
            .detail(msg),
    )
}

/// SFTP backend. A session is opened per operation for now.
pub struct SftpBackend<'a> {
    pub c: &'a Connection,
    pub s: &'a ConnectionSecret,
}

#[async_trait]
impl Remote for SftpBackend<'_> {
    fn caps(&self) -> Caps {
        Caps { multipart: false, resume: false, virtual_buckets: false }
    }

    async fn test(&self) -> Result<String> {
        test(self.c, self.s).await
    }

    async fn list(&self, path: &str) -> Result<Vec<ObjectEntry>> {
        list(self.c, self.s, path).await
    }

    async fn search(
        &self,
        path: &str,
        query: &str,
        limit: usize,
        max_depth: usize,
    ) -> Result<Vec<ObjectEntry>> {
        search(self.c, self.s, path, query, limit, max_depth).await
    }

    async fn download(
        &self,
        app: &AppHandle,
        remote: &str,
        dest: &Path,
        transfer_id: String,
    ) -> Result<()> {
        download(app, self.c, self.s, remote, dest, transfer_id).await
    }

    async fn upload_file(
        &self,
        app: &AppHandle,
        src: &Path,
        remote: &str,
        transfer_id: String,
    ) -> Result<()> {
        upload(app, self.c, self.s, src, remote, transfer_id).await
    }

    async fn upload_dir(
        &self,
        app: &AppHandle,
        remote_base: &str,
        files: &[(PathBuf, String)],
    ) -> Result<usize> {
        upload_dir(app, self.c, self.s, remote_base, files).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        delete(self.c, self.s, path).await
    }
}

/// Location of our known_hosts file, set once at startup from the app config
/// dir (same directory as connections.json — see `connections::Store::load`).
/// Unset in unit tests, where we fall back to plain accept-on-first-use.
static KNOWN_HOSTS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Record where accepted host keys are persisted. Called from app setup.
pub fn init_known_hosts(config_dir: &Path) {
    let _ = KNOWN_HOSTS.set(config_dir.join("known_hosts"));
}

/// known_hosts host field: OpenSSH style — bare host for port 22, `[host]:port`
/// otherwise.
fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Outcome of host-key verification against known_hosts, kept distinct so the
/// log line can say "verified" vs "accepted on first use (TOFU)".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostKeyStatus {
    /// Key matches the one previously stored for this host+algorithm.
    Known,
    /// No stored key for this host+algorithm; accepted and persisted now.
    FirstUse,
    /// A DIFFERENT key is stored for this host+algorithm (possible MITM).
    Mismatch,
}

/// Trust-on-first-use against the known_hosts file.
///
/// Read/write failures are treated leniently (first-use), matching the rest of
/// the app's best-effort local-IO style; a mismatch is always rejected.
fn check_known_host(path: &Path, host: &str, port: u16, key: &PublicKey) -> HostKeyStatus {
    let pattern = host_pattern(host, port);
    let algo = key.algorithm().to_string();
    let Ok(openssh) = key.to_openssh() else {
        return HostKeyStatus::Mismatch; // unencodable key: refuse rather than trust blindly
    };
    // `to_openssh()` yields "<algo> <base64> [comment]"; compare the base64 blob.
    let Some(presented) = openssh.split_whitespace().nth(1) else {
        return HostKeyStatus::Mismatch;
    };

    let data = std::fs::read_to_string(path).unwrap_or_default();
    for line in data.lines() {
        let mut f = line.split_whitespace();
        if let (Some(h), Some(a), Some(b)) = (f.next(), f.next(), f.next()) {
            if h == pattern && a == algo {
                return if b == presented {
                    HostKeyStatus::Known
                } else {
                    HostKeyStatus::Mismatch
                };
            }
        }
    }

    // Unknown host: accept and persist (TOFU). Best-effort write.
    let mut out = data;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("{pattern} {algo} {presented}\n"));
    let _ = std::fs::write(path, out);
    HostKeyStatus::FirstUse
}

/// SSH host-key verification handler: trust-on-first-use, persisted to a
/// known_hosts file. A changed key for a known host is rejected (the rejection
/// surfaces as `russh::Error::UnknownKey`, classified in `open`).
struct Handler {
    host: String,
    port: u16,
    /// Connection display name, for the Debug Log panel.
    name: String,
}

impl client::Handler for Handler {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let status = match KNOWN_HOSTS.get() {
            Some(path) => check_known_host(path, &self.host, self.port, server_public_key),
            // Not initialized (unit tests): plain TOFU without persistence.
            None => HostKeyStatus::FirstUse,
        };

        // Surface the negotiated host key (algorithm + SHA256 fingerprint) and
        // the trust decision. The fingerprint is public material — safe to log.
        if let Some(app) = logging::app_handle() {
            let algo = server_public_key.algorithm().to_string();
            let fp = server_public_key
                .fingerprint(russh::keys::HashAlg::Sha256)
                .to_string();
            let (level, verdict) = match status {
                HostKeyStatus::Known => (Level::Debug, "known host key verified"),
                HostKeyStatus::FirstUse => {
                    (Level::Info, "host key accepted on first use (TOFU)")
                }
                HostKeyStatus::Mismatch => {
                    (Level::Warn, "host key MISMATCH — rejecting connection")
                }
            };
            LogBuilder::new(level, "connection")
                .connector(Connector::Sftp)
                .phase(Phase::Handshake)
                .connection(Some(&self.name))
                .field("algorithm", algo.clone())
                .field("fingerprint", fp.clone())
                .message(format!("Server host key {algo} {fp}: {verdict}"))
                .emit(app);
        }

        Ok(status != HostKeyStatus::Mismatch)
    }
}

async fn open(c: &Connection, s: &ConnectionSecret) -> Result<SftpSession> {
    // The UI stores unset optional fields as "" (not null); treat empty as missing.
    let host = c.host.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
        Error::App(
            AppError::new(ErrorCategory::Config, "SFTP: missing host")
                .connector(Connector::Sftp)
                .phase(Phase::Config),
        )
    })?;
    let port = c.port.unwrap_or(22);
    let user = c.username.clone().filter(|s| !s.is_empty()).ok_or_else(|| {
        Error::App(
            AppError::new(ErrorCategory::Config, "SFTP: missing username")
                .connector(Connector::Sftp)
                .phase(Phase::Config),
        )
    })?;

    logging::emit_global(
        Level::Debug,
        Connector::Sftp,
        Phase::Connect,
        Some(&c.name),
        format!("Connecting to {host}:{port}"),
    );

    let config = Arc::new(client::Config::default());
    let handler = Handler { host: host.clone(), port, name: c.name.clone() };
    let mut session = client::connect(config, (host.as_str(), port), handler)
        .await
        .map_err(|e| match e {
            // Our handler returns `false` only when the presented host key
            // contradicts the one persisted in known_hosts.
            russh::Error::UnknownKey => Error::App(
                AppError::new(
                    ErrorCategory::Auth,
                    format!("SFTP host key for {host} has changed"),
                )
                .connector(Connector::Sftp)
                .phase(Phase::Handshake)
                .remediation(
                    "The server presented a different host key than the one previously trusted. \
                     This can indicate a man-in-the-middle attack. If the server's key was \
                     legitimately rotated, remove its entry from the known_hosts file in the \
                     app's config directory and reconnect.",
                )
                .detail(e.to_string()),
            ),
            e => sftp_err(Phase::Connect, e),
        })?;

    let method = if s.private_key_pem.as_ref().is_some_and(|k| !k.is_empty()) {
        "publickey"
    } else {
        "password"
    };
    logging::emit_global(
        Level::Debug,
        Connector::Sftp,
        Phase::Auth,
        Some(&c.name),
        format!("Authenticating as {user} ({method})"),
    );

    let authed = if let Some(key_pem) = s.private_key_pem.as_ref().filter(|k| !k.is_empty()) {
        let passphrase = s.passphrase.as_deref();
        let key_pair = russh::keys::decode_secret_key(key_pem, passphrase)
            .map_err(|e| {
                Error::App(
                    AppError::new(ErrorCategory::Auth, format!("SFTP key decode failed: {e}"))
                        .connector(Connector::Sftp)
                        .phase(Phase::Auth)
                        .remediation("The private key could not be decoded. Check the key format and passphrase.")
                        .detail(format!("{e:?}")),
                )
            })?;
        let hash = session
            .best_supported_rsa_hash()
            .await
            .map_err(|e| sftp_err(Phase::Handshake, e))?
            .flatten();
        session
            .authenticate_publickey(&user, russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash))
            .await
            .map_err(|e| sftp_err(Phase::Auth, e))?
    } else {
        let pw = s.password.clone().unwrap_or_default();
        session
            .authenticate_password(&user, &pw)
            .await
            .map_err(|e| sftp_err(Phase::Auth, e))?
    };

    if !authed.success() {
        return Err(Error::App(
            AppError::new(ErrorCategory::Auth, "SFTP authentication failed")
                .connector(Connector::Sftp)
                .phase(Phase::Auth)
                .remediation("Check the username and password / private key for this connection."),
        ));
    }
    logging::emit_global(
        Level::Info,
        Connector::Sftp,
        Phase::Auth,
        Some(&c.name),
        format!("Authenticated as {user} ({method})"),
    );

    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| sftp_err(Phase::Handshake, e))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| sftp_err(Phase::Handshake, e))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| sftp_err(Phase::Handshake, e))?;
    logging::emit_global(
        Level::Debug,
        Connector::Sftp,
        Phase::Handshake,
        Some(&c.name),
        "SFTP subsystem ready",
    );
    Ok(sftp)
}

pub async fn test(c: &Connection, s: &ConnectionSecret) -> Result<String> {
    let sftp = open(c, s).await?;
    let cwd = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into());
    Ok(format!("OK — connected, cwd={cwd}"))
}

pub async fn list(c: &Connection, s: &ConnectionSecret, path: &str) -> Result<Vec<ObjectEntry>> {
    let sftp = open(c, s).await?;
    let path = if path.is_empty() { "." } else { path };
    let entries = sftp
        .read_dir(path)
        .await
        .map_err(|e| sftp_err(Phase::List, e))?;
    let mut out = Vec::new();
    for e in entries {
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let meta = e.metadata();
        let is_dir = meta.is_dir();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok()
            })
            .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0));
        let full_key = if path == "/" || path == "." {
            name.clone()
        } else {
            format!("{}/{}", path.trim_end_matches('/'), name)
        };
        out.push(ObjectEntry {
            key: full_key,
            name,
            kind: if is_dir { "folder".into() } else { "file".into() },
            size: if is_dir { None } else { Some(meta.size.unwrap_or(0) as i64) },
            modified,
            etag: None,
        });
    }
    out.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
        ("folder", "file") => std::cmp::Ordering::Less,
        ("file", "folder") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

/// Recursively walk from `root`, returning entries whose name contains `query`
/// (case-insensitive). `name` is the path relative to `root`. Bounded by
/// `max_depth` levels and `limit` results to keep slow links responsive.
pub async fn search(
    c: &Connection,
    s: &ConnectionSecret,
    root: &str,
    query: &str,
    limit: usize,
    max_depth: usize,
) -> Result<Vec<ObjectEntry>> {
    let sftp = open(c, s).await?;
    let needle = query.to_lowercase();
    let start_abs = if root.is_empty() { ".".to_string() } else { root.to_string() };

    let mut out = Vec::new();
    // (absolute dir, relative dir, depth)
    let mut stack: Vec<(String, String, usize)> = vec![(start_abs, String::new(), 0)];

    while let Some((abs_dir, rel_dir, depth)) = stack.pop() {
        let entries = match sftp.read_dir(&abs_dir).await {
            Ok(e) => e,
            Err(_) => continue, // skip unreadable dirs rather than aborting the whole search
        };
        for e in entries {
            let name = e.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let abs_child = if abs_dir == "/" {
                format!("/{name}")
            } else if abs_dir == "." {
                name.clone()
            } else {
                format!("{}/{}", abs_dir.trim_end_matches('/'), name)
            };
            let rel_child = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            let meta = e.metadata();
            let is_dir = meta.is_dir();
            if name.to_lowercase().contains(&needle) {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                    .and_then(|d| DateTime::from_timestamp(d.as_secs() as i64, 0));
                out.push(ObjectEntry {
                    key: abs_child.clone(),
                    name: rel_child.clone(),
                    kind: if is_dir { "folder".into() } else { "file".into() },
                    size: if is_dir { None } else { Some(meta.size.unwrap_or(0) as i64) },
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
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    remote: &str,
    dest: &Path,
    transfer_id: String,
) -> Result<()> {
    let res: Result<()> = async {
        let sftp = open(c, s).await?;
        let meta = sftp
            .metadata(remote)
            .await
            .map_err(|e| sftp_err(Phase::Stat, e))?;
        let total = meta.size.unwrap_or(0);

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
        let mut remote_file = sftp
            .open(remote)
            .await
            .map_err(|e| sftp_err(Phase::Transfer, e))?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut transferred: u64 = 0;
        loop {
            let n = remote_file.read(&mut buf).await?;
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
                    total,
                },
            );
        }
        local.flush().await?;
        let _ = app.emit("transfer", TransferEvent::Done { id: transfer_id.clone() });
        Ok(())
    }
    .await;

    if let Err(e) = &res {
        emit_transfer_error(app, &transfer_id, e);
    }
    res
}

pub async fn upload(
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    src: &Path,
    remote: &str,
    transfer_id: String,
) -> Result<()> {
    let sftp = open(c, s).await?;
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("upload").to_string();
    put_file(&sftp, app, src, remote, &name, transfer_id).await
}

/// Upload every file under `files` (abs path, path relative to the upload root)
/// into `remote_base`, reusing one session and creating remote dirs as needed.
pub async fn upload_dir(
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    remote_base: &str,
    files: &[(PathBuf, String)],
) -> Result<usize> {
    let sftp = open(c, s).await?;
    let mut created: HashSet<String> = HashSet::new();
    ensure_dir(&sftp, &mut created, remote_base).await;

    let mut count = 0;
    for (abs, rel) in files {
        let remote = join_remote(remote_base, rel);
        if let Some((parent, _)) = remote.rsplit_once('/') {
            if !parent.is_empty() {
                ensure_dir(&sftp, &mut created, parent).await;
            }
        }
        put_file(&sftp, app, abs, &remote, rel, Uuid::new_v4().to_string()).await?;
        count += 1;
    }
    Ok(count)
}

/// Create `dir` and all its ancestors (mkdir -p). Errors (e.g. "already exists")
/// are ignored; a genuinely unwritable path surfaces when the file create fails.
async fn ensure_dir(sftp: &SftpSession, created: &mut HashSet<String>, dir: &str) {
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
        let _ = sftp.create_dir(&cur).await;
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
    sftp: &SftpSession,
    app: &AppHandle,
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
        let mut remote_file = sftp
            .create(remote)
            .await
            .map_err(|e| sftp_err(Phase::Transfer, e))?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut transferred: u64 = 0;
        loop {
            let n = local.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            remote_file.write_all(&buf[..n]).await?;
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
        remote_file.flush().await?;
        drop(remote_file);
        let _ = app.emit("transfer", TransferEvent::Done { id: transfer_id.clone() });
        Ok(())
    }
    .await;

    if let Err(e) = &res {
        emit_transfer_error(app, &transfer_id, e);
    }
    res
}

pub async fn delete(c: &Connection, s: &ConnectionSecret, path: &str) -> Result<()> {
    let sftp = open(c, s).await?;
    let meta = sftp
        .metadata(path)
        .await
        .map_err(|e| sftp_err(Phase::Stat, e))?;
    if meta.is_dir() {
        sftp.remove_dir(path)
            .await
            .map_err(|e| sftp_err(Phase::Delete, e))?;
    } else {
        sftp.remove_file(path)
            .await
            .map_err(|e| sftp_err(Phase::Delete, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod live {
    //! Live, read-only integration tests against the public test.rebex.net SFTP
    //! server. Ignored by default (network); run with:
    //!   cargo test --lib sftp::live -- --ignored --nocapture
    use super::*;
    use crate::connections::ConnectionKind;
    use crate::taxonomy::ErrorCategory;

    fn conn(user: &str) -> Connection {
        Connection {
            id: "t".into(),
            name: "rebex".into(),
            kind: ConnectionKind::Sftp,
            host: Some("test.rebex.net".into()),
            port: Some(22),
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
    async fn connect_and_list() {
        let c = conn("demo");
        let s = pw("password");
        let msg = test(&c, &s).await.expect("connect should succeed");
        assert!(msg.contains("OK"), "unexpected: {msg}");
        let entries = list(&c, &s, "").await.expect("list should succeed");
        assert!(!entries.is_empty(), "home should not be empty");
        assert!(
            entries.iter().any(|e| e.name.to_lowercase().contains("readme")),
            "expected a readme entry, got: {:?}",
            entries.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    #[ignore = "hits public test.rebex.net"]
    async fn wrong_password_is_auth() {
        let err = test(&conn("demo"), &pw("definitely-wrong")).await.unwrap_err();
        let a = err.to_app();
        assert_eq!(a.category, ErrorCategory::Auth, "got {a:?}");
    }

    #[tokio::test]
    #[ignore = "hits public test.rebex.net"]
    async fn missing_path_is_classified() {
        let err = list(&conn("demo"), &pw("password"), "/no/such/dir/zzz")
            .await
            .unwrap_err();
        let a = err.to_app();
        // Should be a clean, classified error — not Unknown.
        assert!(
            matches!(a.category, ErrorCategory::NotFound | ErrorCategory::Permission),
            "expected NotFound/Permission, got {a:?}"
        );
    }
}
