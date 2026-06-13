//! Unified remote-backend abstraction.
//!
//! Every protocol implements the [`Remote`] trait; [`connect`] is the single
//! place that maps a [`ConnectionKind`] to a backend. Adding a connector is one
//! new `impl Remote` + one arm here — the rest of the app talks to `dyn Remote`
//! and never matches on kind.
//!
//! Path convention: S3-family uses "<bucket>/<key prefix...>" (the bucket /
//! key split is handled inside the S3 backend); SFTP/FTP/FTPS use a regular
//! absolute path (or "" for the connection's default location).

use crate::connections::{Connection, ConnectionKind, ConnectionSecret};
use crate::error::Result;
use crate::s3::ObjectEntry;
use crate::{ftp, s3, sftp};
use async_trait::async_trait;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;

/// What a backend can do, so the dispatcher / UI can adapt instead of
/// hard-coding "is this the S3 family?". Serialized to the frontend (attached to
/// each connection) so the UI keys behavior off capabilities rather than kind.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    /// Supports chunked/multipart uploads (resumable per-part, no whole-file buffer).
    pub multipart: bool,
    /// Supports resuming an interrupted transfer from an offset.
    pub resume: bool,
    /// Top level is a set of buckets shown as virtual folders.
    pub virtual_buckets: bool,
}

impl Caps {
    /// The capability profile for a connection kind — the single source of truth.
    /// Derived from `kind` alone so the UI can know caps without opening a
    /// session (sidebar grouping/icons render before any Connect). `Remote::caps`
    /// delegates here; the `connection_*` commands attach the result to each
    /// connection they return.
    pub const fn for_kind(kind: ConnectionKind) -> Self {
        match kind {
            ConnectionKind::S3 | ConnectionKind::R2 | ConnectionKind::S3Compat => Caps {
                multipart: true,
                resume: false,
                virtual_buckets: true,
            },
            ConnectionKind::Sftp | ConnectionKind::Ftp | ConnectionKind::Ftps => Caps {
                multipart: false,
                resume: false,
                virtual_buckets: false,
            },
        }
    }
}

/// The capability surface every connector implements. One trait, one factory —
/// no per-operation `match c.kind`.
#[async_trait]
pub trait Remote: Send + Sync {
    /// This backend's capability profile. Per-instance surface; the default
    /// derives from the connection kind via [`Caps::for_kind`], which is also
    /// what the UI receives. Override only if an instance differs from its kind.
    #[allow(dead_code)] // per-instance surface; the UI path goes through Caps::for_kind
    fn caps(&self) -> Caps;

    /// Verify the remote is reachable; returns a short human status string.
    async fn test(&self) -> Result<String>;

    /// List the immediate children of `path`.
    async fn list(&self, path: &str) -> Result<Vec<ObjectEntry>>;

    /// Recursive name search under `path`. `name` in each result is relative to `path`.
    async fn search(
        &self,
        path: &str,
        query: &str,
        limit: usize,
        max_depth: usize,
    ) -> Result<Vec<ObjectEntry>>;

    async fn download(
        &self,
        app: &AppHandle,
        remote: &str,
        dest: &Path,
        transfer_id: String,
    ) -> Result<()>;

    async fn upload_file(
        &self,
        app: &AppHandle,
        src: &Path,
        remote: &str,
        transfer_id: String,
    ) -> Result<()>;

    /// Upload every `(abs path, path relative to the upload root)` pair into
    /// `remote_base`, reusing one connection. Returns the number of files sent.
    async fn upload_dir(
        &self,
        app: &AppHandle,
        remote_base: &str,
        files: &[(PathBuf, String)],
    ) -> Result<usize>;

    async fn delete(&self, path: &str) -> Result<()>;

    /// Gracefully close the underlying session, if any. Called when the user
    /// disconnects or a throwaway (test) backend is done. Default: nothing —
    /// dropping the backend closes the socket. FTP overrides to send QUIT.
    async fn disconnect(&self) {}
}

/// Establish a live backend for `c`. SFTP/FTP open and authenticate a real
/// session here (the expensive handshake); S3 is stateless so this just wraps
/// the credentials. The returned `Arc` is what the [`crate::session::SessionPool`]
/// caches and reuses across operations.
pub async fn open_backend(c: &Connection, s: &ConnectionSecret) -> Result<Arc<dyn Remote>> {
    Ok(match c.kind {
        ConnectionKind::S3 | ConnectionKind::R2 | ConnectionKind::S3Compat => {
            Arc::new(s3::S3Backend::new(c.clone(), s.clone()))
        }
        ConnectionKind::Sftp => Arc::new(sftp::SftpBackend::connect(c, s).await?),
        ConnectionKind::Ftp | ConnectionKind::Ftps => {
            Arc::new(ftp::FtpBackend::connect(c, s).await?)
        }
    })
}

/// Connectivity check for the "Test" button: open a throwaway session, probe it,
/// then close. Never touches the pool — testing must not disturb a live session.
pub async fn test(c: &Connection, s: &ConnectionSecret) -> Result<String> {
    let backend = open_backend(c, s).await?;
    let result = backend.test().await;
    backend.disconnect().await;
    result
}

/// Recursive search with the app's standard bounds; empty query short-circuits.
pub async fn search(backend: &dyn Remote, path: &str, query: &str) -> Result<Vec<ObjectEntry>> {
    const LIMIT: usize = 1000;
    const MAX_DEPTH: usize = 12;
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    backend.search(path, query, LIMIT, MAX_DEPTH).await
}

/// Upload a file or, if `src` is a directory, recurse into a folder upload —
/// the one piece of dispatch behavior that isn't a straight method forward.
pub async fn upload(
    backend: &dyn Remote,
    app: &AppHandle,
    src: &Path,
    remote: &str,
    transfer_id: String,
) -> Result<()> {
    if std::fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false) {
        let files = collect_files_rel(src)?;
        backend.upload_dir(app, remote, &files).await?;
        return Ok(());
    }
    backend.upload_file(app, src, remote, transfer_id).await
}

/// Walk `dir` recursively, returning (absolute path, path relative to `dir`
/// using '/' separators) for every regular file. Symlinks are skipped to avoid
/// cycles. Empty directories are not represented (no file to carry them).
fn collect_files_rel(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, rel)) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let ft = entry.file_type()?;
            if ft.is_dir() {
                stack.push((entry.path(), child_rel));
            } else if ft.is_file() {
                out.push((entry.path(), child_rel));
            }
            // symlinks (neither is_dir nor is_file) are intentionally skipped
        }
    }
    Ok(out)
}
