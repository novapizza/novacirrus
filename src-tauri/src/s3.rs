use crate::connections::{Connection, ConnectionKind, ConnectionSecret};
use crate::error::{AppError, Error, Result};
use crate::remote::{Caps, Remote};
use crate::taxonomy::{
    category_for_http, category_for_s3_code, Connector, ErrorCategory, Level, Phase, StatusCode,
};
use async_trait::async_trait;
use std::path::PathBuf;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, SharedCredentialsProvider};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketInfo {
    pub name: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEntry {
    pub key: String,        // full key inside bucket
    pub name: String,       // last path segment (display)
    pub kind: String,       // "folder" | "file"
    pub size: Option<i64>,
    pub modified: Option<DateTime<Utc>>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TransferEvent {
    #[serde(rename = "start")]
    Start { id: String, name: String, total: u64, direction: String },
    #[serde(rename = "progress")]
    Progress { id: String, transferred: u64, total: u64 },
    #[serde(rename = "done")]
    Done { id: String },
    #[serde(rename = "error")]
    Error { id: String, error: AppError },
}

/// Emit a structured error on the `transfer` channel so a failed transfer's
/// queue row resolves to "error" instead of being stuck "active" forever.
/// Shared by every backend (FTP/SFTP import it).
pub(crate) fn emit_transfer_error(app: &AppHandle, id: &str, e: &Error) {
    let _ = app.emit(
        "transfer",
        TransferEvent::Error {
            id: id.to_string(),
            error: e.to_app(),
        },
    );
}

/// Project an AWS SDK error onto the [`AppError`] IR: pull the S3 error code and
/// HTTP status, classify the 3xx/4xx/5xx family, and keep the raw debug as detail.
pub(crate) fn classify_s3<E>(connector: Connector, phase: Phase, e: &SdkError<E, HttpResponse>) -> Error
where
    E: ProvideErrorMetadata + std::fmt::Debug,
{
    let http = e.raw_response().map(|r| r.status().as_u16());
    let s3_code = e.code().map(|c| c.to_string());
    let s3_msg = e.message().map(|m| m.to_string());

    let category = match e {
        SdkError::TimeoutError(_) | SdkError::DispatchFailure(_) => ErrorCategory::Network,
        SdkError::ConstructionFailure(_) => ErrorCategory::Config,
        _ => s3_code
            .as_deref()
            .and_then(category_for_s3_code)
            .or_else(|| http.map(category_for_http))
            .unwrap_or(ErrorCategory::Unknown),
    };

    let summary = match (&s3_code, &s3_msg) {
        (Some(c), Some(m)) => format!("{c}: {m}"),
        (Some(c), None) => c.clone(),
        (None, _) => match http {
            Some(h) => format!("S3 request failed (HTTP {h})"),
            None => "S3 request failed".to_string(),
        },
    };

    let mut err = AppError::new(category, summary)
        .connector(connector)
        .phase(phase)
        .detail(format!("{e:?}"));
    if let Some(h) = http {
        err = err.code(StatusCode::Http(h));
    }
    Error::App(err)
}

/// Returns (bucket, key_or_prefix). Path is "bucket/key..." with no leading slash.
pub(crate) fn split_s3_path(path: &str) -> (String, String) {
    let trimmed = path.trim_start_matches('/');
    match trimmed.split_once('/') {
        Some((b, k)) => (b.to_string(), k.to_string()),
        None => (trimmed.to_string(), String::new()),
    }
}

/// Validation error for an S3 path missing its bucket segment.
pub(crate) fn needs_bucket(op: &str) -> Error {
    Error::App(
        AppError::new(
            ErrorCategory::Client,
            format!("{op} requires a bucket in the path"),
        )
        .phase(Phase::Config)
        .remediation("Open the bucket first, then retry — the path must start with a bucket name."),
    )
}

/// S3 / R2 / S3-compatible backend. Owns the bucket-vs-key path handling so the
/// dispatcher stays protocol-agnostic.
pub struct S3Backend<'a> {
    pub c: &'a Connection,
    pub s: &'a ConnectionSecret,
}

impl S3Backend<'_> {
    /// The connection's default bucket, if set to a non-empty value. The UI
    /// stores unset fields as "", so an empty string counts as "no bucket".
    fn configured_bucket(&self) -> Option<String> {
        self.c
            .bucket
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(|b| b.to_string())
    }
}

#[async_trait]
impl Remote for S3Backend<'_> {
    fn caps(&self) -> Caps {
        Caps {
            multipart: true,
            resume: false,
            virtual_buckets: true,
        }
    }

    async fn test(&self) -> Result<String> {
        // A bucket-scoped token (e.g. R2 "Object Read & Write") can't ListBuckets.
        // If a default bucket is configured, verify against it instead so such
        // tokens work; otherwise fall back to listing buckets.
        if let Some(bucket) = self.configured_bucket() {
            let client = make_client(self.c, self.s).await?;
            client
                .list_objects_v2()
                .bucket(&bucket)
                .max_keys(1)
                .send()
                .await
                .map_err(|e| classify_s3(self.c.kind.into(), Phase::List, &e))?;
            return Ok(format!("OK — bucket \"{bucket}\" reachable"));
        }
        let bs = list_buckets(self.c, self.s).await?;
        Ok(format!("OK — {} bucket(s)", bs.len()))
    }

    async fn list(&self, path: &str) -> Result<Vec<ObjectEntry>> {
        let (bucket, prefix) = split_s3_path(path);
        if bucket.is_empty() {
            // Top of an S3 connection. A configured default bucket is shown as
            // the sole virtual folder (works with bucket-scoped tokens that
            // can't ListBuckets); otherwise list all buckets.
            if let Some(b) = self.configured_bucket() {
                return Ok(vec![ObjectEntry {
                    key: b.clone(),
                    name: b,
                    kind: "folder".into(),
                    size: None,
                    modified: None,
                    etag: None,
                }]);
            }
            let bs = list_buckets(self.c, self.s).await?;
            return Ok(bs
                .into_iter()
                .map(|b| ObjectEntry {
                    key: b.name.clone(),
                    name: b.name,
                    kind: "folder".into(),
                    size: None,
                    modified: b.created_at,
                    etag: None,
                })
                .collect());
        }
        list_objects(self.c, self.s, &bucket, &prefix).await
    }

    async fn search(
        &self,
        path: &str,
        query: &str,
        limit: usize,
        _max_depth: usize,
    ) -> Result<Vec<ObjectEntry>> {
        let (bucket, prefix) = split_s3_path(path);
        if bucket.is_empty() {
            return Err(needs_bucket("S3 search"));
        }
        search_objects(self.c, self.s, &bucket, &prefix, query, limit).await
    }

    async fn download(
        &self,
        app: &AppHandle,
        remote: &str,
        dest: &Path,
        transfer_id: String,
    ) -> Result<()> {
        let (bucket, key) = split_s3_path(remote);
        if bucket.is_empty() {
            return Err(needs_bucket("S3 download"));
        }
        download(app, self.c, self.s, &bucket, &key, dest, transfer_id).await
    }

    async fn upload_file(
        &self,
        app: &AppHandle,
        src: &Path,
        remote: &str,
        transfer_id: String,
    ) -> Result<()> {
        let (bucket, key) = split_s3_path(remote);
        if bucket.is_empty() {
            return Err(needs_bucket("S3 upload"));
        }
        upload(app, self.c, self.s, &bucket, &key, src, transfer_id).await
    }

    async fn upload_dir(
        &self,
        app: &AppHandle,
        remote_base: &str,
        files: &[(PathBuf, String)],
    ) -> Result<usize> {
        let (bucket, prefix) = split_s3_path(remote_base);
        if bucket.is_empty() {
            return Err(needs_bucket("S3 upload"));
        }
        upload_dir(app, self.c, self.s, &bucket, &prefix, files).await
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let (bucket, key) = split_s3_path(path);
        if bucket.is_empty() {
            return Err(needs_bucket("S3 delete"));
        }
        delete_object(self.c, self.s, &bucket, &key).await
    }
}

pub async fn make_client(c: &Connection, s: &ConnectionSecret) -> Result<Client> {
    // The UI stores unset optional fields as "" (not null), so treat an empty
    // region as absent and fall back to the per-kind default. An empty region
    // string reaches the SDK as an invalid endpoint config (ConstructionFailure).
    // Default region is "auto": for R2 and S3-compat the signing region is
    // irrelevant (the explicit endpoint drives addressing), so "auto" is the
    // friendliest default. Real AWS S3 has no endpoint override, and "auto" is
    // neither a valid signing region nor a resolvable endpoint — so bootstrap
    // AWS via us-east-1, which transparently redirects to the bucket's region.
    let region = c
        .region
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "auto".to_string());
    let has_endpoint = c.endpoint.as_ref().is_some_and(|s| !s.is_empty());
    let region = if region == "auto" && matches!(c.kind, ConnectionKind::S3) && !has_endpoint {
        "us-east-1".to_string()
    } else {
        region
    };

    let creds = Credentials::new(
        s.access_key_id.clone().unwrap_or_default(),
        s.secret_access_key.clone().unwrap_or_default(),
        s.session_token.clone(),
        None,
        "novacirrus",
    );

    crate::logging::emit_global(
        Level::Debug,
        c.kind.into(),
        Phase::Connect,
        Some(&c.name),
        format!(
            "Endpoint {}, region {region}, path-style={}",
            c.endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("aws default"),
            c.use_path_style
        ),
    );

    let mut builder = S3ConfigBuilder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(region))
        .credentials_provider(SharedCredentialsProvider::new(creds))
        .force_path_style(c.use_path_style);

    if let Some(ep) = c.endpoint.as_ref().filter(|s| !s.is_empty()) {
        builder = builder.endpoint_url(ep.clone());
    } else if matches!(c.kind, ConnectionKind::R2) {
        return Err(AppError::new(
            ErrorCategory::Config,
            "R2 connections require an endpoint URL (https://<account>.r2.cloudflarestorage.com)",
        )
        .connector(c.kind)
        .phase(Phase::Config)
        .into());
    }

    Ok(Client::from_conf(builder.build()))
}

pub async fn list_buckets(c: &Connection, s: &ConnectionSecret) -> Result<Vec<BucketInfo>> {
    let client = make_client(c, s).await?;
    let resp = client
        .list_buckets()
        .send()
        .await
        .map_err(|e| classify_s3(c.kind.into(), Phase::List, &e))?;
    Ok(resp
        .buckets()
        .iter()
        .map(|b| BucketInfo {
            name: b.name().unwrap_or_default().to_string(),
            created_at: b
                .creation_date()
                .and_then(|d| DateTime::<Utc>::from_timestamp(d.secs(), 0)),
        })
        .collect())
}

/// List objects with delimiter='/' so we get virtual folders.
pub async fn list_objects(
    c: &Connection,
    s: &ConnectionSecret,
    bucket: &str,
    prefix: &str,
) -> Result<Vec<ObjectEntry>> {
    let client = make_client(c, s).await?;
    let mut out = Vec::new();
    let mut continuation: Option<String> = None;
    let norm_prefix = if prefix.is_empty() || prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };

    loop {
        let mut req = client
            .list_objects_v2()
            .bucket(bucket)
            .delimiter("/")
            .prefix(&norm_prefix);
        if let Some(t) = continuation.as_ref() {
            req = req.continuation_token(t);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| classify_s3(c.kind.into(), Phase::List, &e))?;

        for cp in resp.common_prefixes() {
            if let Some(p) = cp.prefix() {
                let trimmed = p.trim_end_matches('/');
                let name = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
                out.push(ObjectEntry {
                    key: p.to_string(),
                    name,
                    kind: "folder".into(),
                    size: None,
                    modified: None,
                    etag: None,
                });
            }
        }

        for obj in resp.contents() {
            let Some(key) = obj.key() else { continue };
            if key == norm_prefix {
                continue; // skip the prefix marker itself
            }
            let name = key.rsplit('/').next().unwrap_or(key).to_string();
            out.push(ObjectEntry {
                key: key.to_string(),
                name,
                kind: "file".into(),
                size: obj.size(),
                modified: obj
                    .last_modified()
                    .and_then(|d| DateTime::<Utc>::from_timestamp(d.secs(), 0)),
                etag: obj.e_tag().map(|s| s.trim_matches('"').to_string()),
            });
        }

        match resp.next_continuation_token() {
            Some(t) if resp.is_truncated().unwrap_or(false) => continuation = Some(t.to_string()),
            _ => break,
        }
    }

    Ok(out)
}

/// Recursively search all objects under a prefix (no delimiter) whose name
/// contains `query` (case-insensitive). Returned `name` is the key relative to
/// `prefix`, so the UI can reconstruct the absolute path via the usual join.
pub async fn search_objects(
    c: &Connection,
    s: &ConnectionSecret,
    bucket: &str,
    prefix: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<ObjectEntry>> {
    let client = make_client(c, s).await?;
    let needle = query.to_lowercase();
    let norm_prefix = if prefix.is_empty() || prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };

    let mut out = Vec::new();
    let mut continuation: Option<String> = None;
    loop {
        let mut req = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(&norm_prefix); // no delimiter => full subtree
        if let Some(t) = continuation.as_ref() {
            req = req.continuation_token(t);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| classify_s3(c.kind.into(), Phase::Search, &e))?;

        for obj in resp.contents() {
            let Some(key) = obj.key() else { continue };
            if key == norm_prefix || key.ends_with('/') {
                continue; // skip the prefix marker and folder placeholders
            }
            let rel = key.strip_prefix(&norm_prefix).unwrap_or(key);
            let leaf = rel.rsplit('/').next().unwrap_or(rel);
            if !leaf.to_lowercase().contains(&needle) {
                continue;
            }
            out.push(ObjectEntry {
                key: key.to_string(),
                name: rel.to_string(),
                kind: "file".into(),
                size: obj.size(),
                modified: obj
                    .last_modified()
                    .and_then(|d| DateTime::<Utc>::from_timestamp(d.secs(), 0)),
                etag: obj.e_tag().map(|s| s.trim_matches('"').to_string()),
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }

        match resp.next_continuation_token() {
            Some(t) if resp.is_truncated().unwrap_or(false) => continuation = Some(t.to_string()),
            _ => break,
        }
    }

    Ok(out)
}

pub async fn delete_object(
    c: &Connection,
    s: &ConnectionSecret,
    bucket: &str,
    key: &str,
) -> Result<()> {
    let client = make_client(c, s).await?;
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify_s3(c.kind.into(), Phase::Delete, &e))?;
    Ok(())
}

pub async fn download(
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    bucket: &str,
    key: &str,
    dest: &Path,
    transfer_id: String,
) -> Result<()> {
    let connector: Connector = c.kind.into();
    let res: Result<()> = async {
        let client = make_client(c, s).await?;

        let head = client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| classify_s3(connector, Phase::Stat, &e))?;
        let total = head.content_length().unwrap_or(0).max(0) as u64;

        let _ = app.emit(
            "transfer",
            TransferEvent::Start {
                id: transfer_id.clone(),
                name: key.rsplit('/').next().unwrap_or(key).to_string(),
                total,
                direction: "down".into(),
            },
        );

        let resp = client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| classify_s3(connector, Phase::Transfer, &e))?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = File::create(dest).await?;
        let mut transferred: u64 = 0;
        let mut stream = resp.body;

        while let Some(chunk) = stream
            .try_next()
            .await
            .map_err(|e| {
                Error::App(
                    AppError::new(ErrorCategory::Network, format!("S3 read failed: {e}"))
                        .connector(connector)
                        .phase(Phase::Transfer)
                        .detail(format!("{e:?}")),
                )
            })?
        {
            file.write_all(&chunk).await?;
            transferred += chunk.len() as u64;
            let _ = app.emit(
                "transfer",
                TransferEvent::Progress {
                    id: transfer_id.clone(),
                    transferred,
                    total,
                },
            );
        }
        file.flush().await?;

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
    bucket: &str,
    key: &str,
    src: &Path,
    transfer_id: String,
) -> Result<()> {
    let client = make_client(c, s).await?;
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("upload").to_string();
    put_file(&client, app, c.kind.into(), bucket, key, src, &name, transfer_id).await
}

/// Upload every file under `files` (abs path, key relative to the upload root)
/// into `prefix`, reusing a single client. S3 needs no directory creation.
pub async fn upload_dir(
    app: &AppHandle,
    c: &Connection,
    s: &ConnectionSecret,
    bucket: &str,
    prefix: &str,
    files: &[(std::path::PathBuf, String)],
) -> Result<usize> {
    let client = make_client(c, s).await?;
    let connector: Connector = c.kind.into();
    let base = prefix.trim_end_matches('/');
    let mut count = 0;
    for (abs, rel) in files {
        let key = if base.is_empty() { rel.clone() } else { format!("{base}/{rel}") };
        let id = Uuid::new_v4().to_string();
        put_file(&client, app, connector, bucket, &key, abs, rel, id).await?;
        count += 1;
    }
    Ok(count)
}

const MIB: u64 = 1024 * 1024;
/// Files at or above this use multipart upload; smaller ones a single PUT.
const MULTIPART_THRESHOLD: u64 = 16 * MIB;
/// S3 minimum part size (except the final part).
const MIN_PART_SIZE: u64 = 8 * MIB;
/// S3 hard cap on parts per upload.
const MAX_PARTS: u64 = 10_000;

/// Pick a part size that keeps the count under [`MAX_PARTS`] and each part at or
/// above [`MIN_PART_SIZE`], rounded up to a whole MiB.
fn part_size_for(total: u64) -> u64 {
    let needed = total.div_ceil(MAX_PARTS).max(MIN_PART_SIZE);
    needed.div_ceil(MIB) * MIB
}

async fn put_file(
    client: &Client,
    app: &AppHandle,
    connector: Connector,
    bucket: &str,
    key: &str,
    src: &Path,
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

        // Bridge the AppHandle-free transfer core to the `transfer` event channel.
        let mut on_progress = |transferred: u64| {
            let _ = app.emit(
                "transfer",
                TransferEvent::Progress { id: transfer_id.clone(), transferred, total },
            );
        };

        if total < MULTIPART_THRESHOLD {
            put_small(client, connector, bucket, key, src).await?;
            on_progress(total);
        } else {
            put_multipart(client, connector, bucket, key, src, total, &mut on_progress).await?;
        }

        let _ = app.emit("transfer", TransferEvent::Done { id: transfer_id.clone() });
        Ok(())
    }
    .await;

    if let Err(e) = &res {
        emit_transfer_error(app, &transfer_id, e);
    }
    res
}

/// Single-shot PUT that streams from disk (no whole-file buffer). Small files
/// transfer fast enough that one start→done progress jump is acceptable.
///
/// AppHandle-free so it is callable from integration tests.
async fn put_small(
    client: &Client,
    connector: Connector,
    bucket: &str,
    key: &str,
    src: &Path,
) -> Result<()> {
    let body = ByteStream::from_path(src).await.map_err(|e| {
        Error::App(
            AppError::new(ErrorCategory::Io, format!("Could not read file: {e}"))
                .connector(connector)
                .phase(Phase::Transfer)
                .detail(format!("{e:?}")),
        )
    })?;
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .send()
        .await
        .map_err(|e| classify_s3(connector, Phase::Transfer, &e))?;
    Ok(())
}

/// Multipart upload: create → upload parts sequentially → complete. Any failure
/// aborts the upload so no orphaned parts are left behind (orphans accrue
/// storage cost). Parts are sequential by design — throughput is not the
/// bottleneck here, correctness and progress fidelity are.
///
/// `on_progress(cumulative_bytes)` is invoked after each part S3 accepts.
/// AppHandle-free so it is callable from integration tests (the caller supplies
/// the progress sink — the real app emits a `transfer` event, tests collect).
async fn put_multipart(
    client: &Client,
    connector: Connector,
    bucket: &str,
    key: &str,
    src: &Path,
    total: u64,
    on_progress: &mut (dyn FnMut(u64) + Send),
) -> Result<()> {
    let create = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| classify_s3(connector, Phase::Multipart, &e))?;
    let upload_id = create
        .upload_id()
        .ok_or_else(|| {
            Error::App(
                AppError::new(ErrorCategory::Protocol, "S3 did not return an upload id")
                    .connector(connector)
                    .phase(Phase::Multipart),
            )
        })?
        .to_string();

    let part_size = part_size_for(total);
    match upload_parts(client, connector, bucket, key, &upload_id, src, part_size, on_progress).await {
        Ok(parts) => {
            let completed = CompletedMultipartUpload::builder().set_parts(Some(parts)).build();
            client
                .complete_multipart_upload()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .multipart_upload(completed)
                .send()
                .await
                .map_err(|e| classify_s3(connector, Phase::Multipart, &e))?;
            Ok(())
        }
        Err(e) => {
            // Best-effort: leave the bucket clean even on failure.
            let _ = client
                .abort_multipart_upload()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .send()
                .await;
            Err(e)
        }
    }
}

async fn upload_parts(
    client: &Client,
    connector: Connector,
    bucket: &str,
    key: &str,
    upload_id: &str,
    src: &Path,
    part_size: u64,
    on_progress: &mut (dyn FnMut(u64) + Send),
) -> Result<Vec<CompletedPart>> {
    let mut file = File::open(src).await?;
    let mut parts = Vec::new();
    let mut part_number: i32 = 1;
    let mut transferred: u64 = 0;
    let mut buf = vec![0u8; part_size as usize];

    loop {
        // Fill a whole part (the last one may be short).
        let mut filled = 0usize;
        while filled < buf.len() {
            let n = file.read(&mut buf[filled..]).await?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break; // clean EOF on a part boundary
        }

        let body = ByteStream::from(Bytes::copy_from_slice(&buf[..filled]));
        let resp = client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(body)
            .send()
            .await
            .map_err(|e| classify_s3(connector, Phase::Multipart, &e))?;

        parts.push(
            CompletedPart::builder()
                .part_number(part_number)
                .set_e_tag(resp.e_tag().map(|s| s.to_string()))
                .build(),
        );

        // Progress reflects bytes actually accepted by S3, not bytes read off disk.
        transferred += filled as u64;
        on_progress(transferred);

        part_number += 1;
        if (filled as u64) < part_size {
            break; // short read => last part
        }
    }

    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_path_variants() {
        assert_eq!(split_s3_path("bucket/key"), ("bucket".into(), "key".into()));
        assert_eq!(
            split_s3_path("bucket/a/b/c.txt"),
            ("bucket".into(), "a/b/c.txt".into())
        );
        assert_eq!(split_s3_path("bucket"), ("bucket".into(), String::new()));
        // leading slash is tolerated
        assert_eq!(split_s3_path("/bucket/key"), ("bucket".into(), "key".into()));
        // empty / root => no bucket (caller treats as "list buckets")
        assert_eq!(split_s3_path(""), (String::new(), String::new()));
        assert_eq!(split_s3_path("/"), (String::new(), String::new()));
    }

    #[test]
    fn part_size_invariants() {
        // A range of sizes from just over the threshold up to the 5 TiB S3 max.
        let sizes = [
            MULTIPART_THRESHOLD,
            20 * MIB,
            100 * MIB,
            1024 * MIB,            // 1 GiB
            100 * 1024 * MIB,      // 100 GiB
            5 * 1024 * 1024 * MIB, // 5 TiB (S3 max object size)
        ];
        for total in sizes {
            let ps = part_size_for(total);
            assert_eq!(ps % MIB, 0, "part size must be a whole MiB for {total}");
            assert!(ps >= MIN_PART_SIZE, "part size below S3 minimum for {total}");
            let parts = total.div_ceil(ps);
            assert!(
                parts <= MAX_PARTS,
                "size {total} needs {parts} parts (> {MAX_PARTS}) at part size {ps}"
            );
        }
    }

    #[test]
    fn part_size_small_files_use_minimum() {
        // Anything that reaches multipart but is comfortably under 8 MiB*10000
        // should land on the 8 MiB minimum part size.
        assert_eq!(part_size_for(MULTIPART_THRESHOLD), MIN_PART_SIZE);
        assert_eq!(part_size_for(50 * MIB), MIN_PART_SIZE);
    }
}

#[cfg(test)]
mod r2_live {
    //! Live multipart-upload tests against a real R2 (or S3) bucket. Gated on
    //! env vars and ignored by default. Run with:
    //!   R2_ENDPOINT=https://<acct>.r2.cloudflarestorage.com \
    //!   R2_ACCESS_KEY_ID=... R2_SECRET_ACCESS_KEY=... R2_BUCKET=... \
    //!   cargo test --lib s3::r2_live -- --ignored --nocapture
    //!
    //! Needs a bucket with write + multipart permissions. Each test cleans up
    //! after itself.
    use super::*;
    use crate::connections::ConnectionKind;

    struct Env {
        endpoint: String,
        access: String,
        secret: String,
        bucket: String,
    }

    /// Parse `src-tauri/.env` (KEY=VALUE lines) into a map. Best-effort; a
    /// missing file yields an empty map. Located via CARGO_MANIFEST_DIR so it is
    /// independent of the test's working directory.
    fn load_env_file() -> std::collections::HashMap<String, String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
        let mut map = std::collections::HashMap::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    map.insert(k.trim().to_string(), v.to_string());
                }
            }
        }
        map
    }

    /// Returns the R2 config from the process env (preferred) or `.env`, or None
    /// (with a printed note) so running `--ignored` without creds — or with the
    /// untouched REPLACE_ME placeholders — is a no-op rather than a failure.
    fn env() -> Option<Env> {
        let file = load_env_file();
        let get = |k: &str| {
            std::env::var(k)
                .ok()
                .or_else(|| file.get(k).cloned())
                .filter(|v| !v.is_empty() && !v.contains("REPLACE_ME"))
        };
        match (
            get("R2_ENDPOINT"),
            get("R2_ACCESS_KEY_ID"),
            get("R2_SECRET_ACCESS_KEY"),
            get("R2_BUCKET"),
        ) {
            (Some(endpoint), Some(access), Some(secret), Some(bucket)) => {
                Some(Env { endpoint, access, secret, bucket })
            }
            _ => {
                eprintln!("[r2_live] skipped: set R2_ENDPOINT / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY / R2_BUCKET");
                None
            }
        }
    }

    fn conn(e: &Env) -> Connection {
        Connection {
            id: "t".into(),
            name: "r2".into(),
            kind: ConnectionKind::R2,
            host: None,
            port: None,
            region: Some("auto".into()),
            endpoint: Some(e.endpoint.clone()),
            bucket: Some(e.bucket.clone()),
            default_path: None,
            username: None,
            use_path_style: false,
            ftp_mode: None,
            ftps_implicit: false,
            color: "#fff".into(),
            created_at: None,
        }
    }
    fn secret(e: &Env) -> ConnectionSecret {
        ConnectionSecret {
            access_key_id: Some(e.access.clone()),
            secret_access_key: Some(e.secret.clone()),
            ..Default::default()
        }
    }

    /// A 20 MiB temp file → exercises multipart with 8 MiB parts (8 + 8 + 4),
    /// i.e. multiple full parts plus a short final part.
    fn make_temp(size: usize) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("novacirrus-mpu-{}.bin", Uuid::new_v4()));
        // Non-uniform bytes so a buggy offset would corrupt detectably.
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, &data).expect("write temp file");
        p
    }

    #[tokio::test]
    #[ignore = "needs R2_* env + a writable bucket"]
    async fn multipart_roundtrips_and_reports_progress() {
        let Some(e) = env() else { return };
        let c = conn(&e);
        let s = secret(&e);
        let client = make_client(&c, &s).await.expect("client");

        let size = 20 * 1024 * 1024usize; // 20 MiB -> 3 parts
        let src = make_temp(size);
        let key = format!("novacirrus-test/mpu-{}.bin", Uuid::new_v4());

        // Sanity: our plan should produce >1 part for this size.
        assert!(size as u64 >= MULTIPART_THRESHOLD);
        let part_size = part_size_for(size as u64);
        let expected_parts = (size as u64).div_ceil(part_size);
        assert!(expected_parts >= 2, "test should span multiple parts");

        let mut progress: Vec<u64> = Vec::new();
        let res = put_multipart(
            &client,
            c.kind.into(),
            &e.bucket,
            &key,
            &src,
            size as u64,
            &mut |t| progress.push(t),
        )
        .await;

        // Always remove the local temp file.
        let _ = std::fs::remove_file(&src);

        // Read back the object size while it exists, THEN delete it — all before
        // any assertion, so a failing assert can never leave a stray object or
        // temp file behind ("always delete what you create").
        let uploaded_len = if res.is_ok() {
            client
                .head_object()
                .bucket(&e.bucket)
                .key(&key)
                .send()
                .await
                .ok()
                .and_then(|h| h.content_length())
                .map(|l| l as u64)
        } else {
            None
        };
        let _ = client.delete_object().bucket(&e.bucket).key(&key).send().await;

        // Now it's safe to assert.
        res.expect("multipart upload should succeed");
        assert!(!progress.is_empty(), "expected progress callbacks");
        assert!(
            progress.windows(2).all(|w| w[1] >= w[0]),
            "progress must be monotonic: {progress:?}"
        );
        assert_eq!(*progress.last().unwrap(), size as u64, "final progress != total");
        assert_eq!(progress.len() as u64, expected_parts, "one progress tick per part");
        assert_eq!(uploaded_len, Some(size as u64), "object size mismatch after upload");
    }

    #[tokio::test]
    #[ignore = "needs R2_* env + a writable bucket"]
    async fn abort_removes_pending_upload() {
        let Some(e) = env() else { return };
        let c = conn(&e);
        let client = make_client(&c, &secret(&e)).await.expect("client");
        let key = format!("novacirrus-test/abort-{}.bin", Uuid::new_v4());

        // Create a multipart upload, then abort it — mirrors our error path.
        let created = client
            .create_multipart_upload()
            .bucket(&e.bucket)
            .key(&key)
            .send()
            .await
            .expect("create mpu");
        let upload_id = created.upload_id().expect("upload id").to_string();

        client
            .abort_multipart_upload()
            .bucket(&e.bucket)
            .key(&key)
            .upload_id(&upload_id)
            .send()
            .await
            .expect("abort mpu");

        // The aborted upload must no longer be listed as in-progress.
        let listed = client
            .list_multipart_uploads()
            .bucket(&e.bucket)
            .prefix(&key)
            .send()
            .await
            .expect("list mpu");
        let still_pending = listed
            .uploads()
            .iter()
            .any(|u| u.upload_id() == Some(upload_id.as_str()));
        assert!(!still_pending, "aborted upload should not remain pending");
    }
}
