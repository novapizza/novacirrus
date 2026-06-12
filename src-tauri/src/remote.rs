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
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// What a backend can do, so the dispatcher / UI can adapt instead of
/// hard-coding "is this the S3 family?". Extended as features land (e.g. #5
/// flips `multipart` on for S3). Not yet consumed — capability surface only.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct Caps {
    /// Supports chunked/multipart uploads (resumable per-part, no whole-file buffer).
    pub multipart: bool,
    /// Supports resuming an interrupted transfer from an offset.
    pub resume: bool,
    /// Top level is a set of buckets shown as virtual folders.
    pub virtual_buckets: bool,
}

/// The capability surface every connector implements. One trait, one factory —
/// no per-operation `match c.kind`.
#[async_trait]
pub trait Remote: Send + Sync {
    #[allow(dead_code)] // capability surface; consumed by UI / #5
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
}

/// The single dispatch point: pick the backend for this connection's kind.
pub fn connect<'a>(c: &'a Connection, s: &'a ConnectionSecret) -> Box<dyn Remote + 'a> {
    match c.kind {
        ConnectionKind::S3 | ConnectionKind::R2 | ConnectionKind::S3Compat => {
            Box::new(s3::S3Backend { c, s })
        }
        ConnectionKind::Sftp => Box::new(sftp::SftpBackend { c, s }),
        ConnectionKind::Ftp | ConnectionKind::Ftps => Box::new(ftp::FtpBackend { c, s }),
    }
}

// --- Thin wrappers used by the command layer. Each just resolves the backend
//     and forwards; no behavior of its own except the upload dir-vs-file split. ---

pub async fn test(c: &Connection, s: &ConnectionSecret) -> Result<String> {
    connect(c, s).test().await
}

pub async fn list(c: &Connection, s: &ConnectionSecret, path: &str) -> Result<Vec<ObjectEntry>> {
    connect(c, s).list(path).await
}

pub async fn search(
    c: &Connection,
    s: &ConnectionSecret,
    path: &str,
    query: &str,
) -> Result<Vec<ObjectEntry>> {
    const LIMIT: usize = 1000;
    const MAX_DEPTH: usize = 12;
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    connect(c, s).search(path, query, LIMIT, MAX_DEPTH).await
}

pub async fn download(
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    remote: &str,
    dest: &Path,
    transfer_id: String,
) -> Result<()> {
    connect(c, s).download(app, remote, dest, transfer_id).await
}

pub async fn upload(
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    src: &Path,
    remote: &str,
    transfer_id: String,
) -> Result<()> {
    // A directory source means a recursive folder upload.
    if std::fs::metadata(src).map(|m| m.is_dir()).unwrap_or(false) {
        let files = collect_files_rel(src)?;
        connect(c, s).upload_dir(app, remote, &files).await?;
        return Ok(());
    }
    connect(c, s).upload_file(app, src, remote, transfer_id).await
}

pub async fn delete(c: &Connection, s: &ConnectionSecret, path: &str) -> Result<()> {
    connect(c, s).delete(path).await
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
