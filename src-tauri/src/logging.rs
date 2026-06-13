//! Structured logging surfaced to the UI Debug Log panel.
//!
//! A log line is an IR record, not a sentence: besides `level` and a rendered
//! `message`, it carries the same taxonomy axes as [`crate::error::AppError`] —
//! `connector`, `phase`, `code`, `category` — plus a free `fields` bag. The
//! Debug panel filters on these (e.g. show only `tls` / `passive` phases) rather
//! than grepping text.
//!
//! Adding a new debug type is: pick/extend a [`Phase`], emit a `LogEvent` with it
//! (and any structured `fields`), done — the panel picks it up automatically.
//!
//! This is intentionally separate from the `transfer` event channel (which drives
//! the transfer queue UI).

use crate::connections::ConnectionKind;
use crate::error::Error;
use crate::taxonomy::{ErrorCategory, Level, Phase, StatusCode};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};

/// Global handle for emitters that don't have an `AppHandle` threaded through
/// (connector lifecycle logs, the `log`-crate bridge). Unset in unit tests, in
/// which case all such emissions are no-ops.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// The global [`AppHandle`], if the app is running (None in unit tests).
pub fn app_handle() -> Option<&'static AppHandle> {
    APP_HANDLE.get()
}

/// Emit a structured log line via the global [`APP_HANDLE`]. No-op when the
/// handle is unset (unit tests). For deeper structure use [`LogBuilder`] with
/// [`app_handle`] directly.
pub fn emit_global(
    level: Level,
    connector: ConnectionKind,
    phase: Phase,
    connection: Option<&str>,
    message: impl Into<String>,
) {
    if let Some(app) = APP_HANDLE.get() {
        LogBuilder::new(level, "connection")
            .connector(connector)
            .phase(phase)
            .connection(connection)
            .message(message)
            .emit(app);
    }
}

/// Install the global app handle and bridge `log`-crate records from the
/// protocol libraries (russh, suppaftp, rustls) into the Debug Log panel —
/// FileZilla-style raw traces: SSH kex/cipher negotiation, FTP command and
/// reply lines (`227 Entering Passive Mode …`), TLS handshake detail.
///
/// Called once from app setup. Never panics if a logger is already installed.
pub fn init_log_bridge(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
    let _ = log::set_boxed_logger(Box::new(ProtocolLogBridge));
    log::set_max_level(log::LevelFilter::Debug);
}

/// Forwards protocol-library log records only; everything else is ignored to
/// avoid noise and feedback loops.
struct ProtocolLogBridge;

fn bridge_target(target: &str) -> Option<Option<ConnectionKind>> {
    if target.starts_with("russh") {
        Some(Some(ConnectionKind::Sftp))
    } else if target.starts_with("suppaftp") {
        Some(Some(ConnectionKind::Ftp))
    } else if target.starts_with("rustls") {
        Some(None) // TLS lines are shared between FTPS and S3; no single connector
    } else {
        None
    }
}

impl log::Log for ProtocolLogBridge {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        bridge_target(metadata.target()).is_some()
    }

    fn log(&self, record: &log::Record) {
        let Some(connector) = bridge_target(record.target()) else {
            return;
        };
        let Some(app) = APP_HANDLE.get() else {
            return;
        };
        let level = match record.level() {
            log::Level::Error => Level::Error,
            log::Level::Warn => Level::Warn,
            log::Level::Info => Level::Info,
            log::Level::Debug | log::Level::Trace => Level::Debug,
        };
        let mut b = LogBuilder::new(level, "protocol")
            .phase(Phase::Protocol)
            .field("target", record.target())
            .message(record.args().to_string());
        if let Some(c) = connector {
            b = b.connector(c);
        }
        b.emit(app);
    }

    fn flush(&self) {}
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    pub id: String,
    pub ts: i64, // epoch millis
    pub level: Level,
    /// Coarse domain: "connection" | "transfer" | …
    pub scope: String,
    pub connector: Option<ConnectionKind>,
    pub phase: Option<Phase>,
    pub code: Option<StatusCode>,
    /// Set on error lines so the panel can badge the failure class.
    pub category: Option<ErrorCategory>,
    pub message: String,
    pub connection: Option<String>,
    /// Structured detail (cipher suite, rtt, part number, byte counts, …).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// Fluent builder for a structured log line. Terminal call is [`LogBuilder::emit`].
pub struct LogBuilder {
    ev: LogEvent,
}

impl LogBuilder {
    pub fn new(level: Level, scope: impl Into<String>) -> Self {
        Self {
            ev: LogEvent {
                id: uuid::Uuid::new_v4().to_string(),
                ts: chrono::Utc::now().timestamp_millis(),
                level,
                scope: scope.into(),
                connector: None,
                phase: None,
                code: None,
                category: None,
                message: String::new(),
                connection: None,
                fields: BTreeMap::new(),
            },
        }
    }

    pub fn connector(mut self, c: impl Into<ConnectionKind>) -> Self {
        self.ev.connector = Some(c.into());
        self
    }

    pub fn phase(mut self, p: Phase) -> Self {
        self.ev.phase = Some(p);
        self
    }

    pub fn code(mut self, c: StatusCode) -> Self {
        self.ev.code = Some(c);
        self
    }

    pub fn category(mut self, c: ErrorCategory) -> Self {
        self.ev.category = Some(c);
        self
    }

    pub fn connection(mut self, name: Option<&str>) -> Self {
        self.ev.connection = name.map(|s| s.to_string());
        self
    }

    pub fn field(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.ev.fields.insert(key.to_string(), value.into());
        self
    }

    pub fn message(mut self, m: impl Into<String>) -> Self {
        self.ev.message = m.into();
        self
    }

    pub fn emit(self, app: &AppHandle) {
        let _ = app.emit("log", self.ev);
    }
}

/// Simple log line (no protocol detail). Use [`LogBuilder`] for structured logs
/// and [`log_error`] for failures.
#[allow(dead_code)]
pub fn log(
    app: &AppHandle,
    level: Level,
    scope: &str,
    connection: Option<&str>,
    message: impl Into<String>,
) {
    LogBuilder::new(level, scope)
        .connection(connection)
        .message(message)
        .emit(app);
}

/// Log a failure, lifting the classification (connector / phase / code /
/// category) off the [`Error`]'s `AppError` so the Debug panel can filter and
/// badge it. `prefix` is a short human lead-in (e.g. "Upload failed").
pub fn log_error(
    app: &AppHandle,
    scope: &str,
    connection: Option<&str>,
    prefix: &str,
    e: &Error,
) {
    let a = e.to_app();
    let mut b = LogBuilder::new(Level::Error, scope)
        .category(a.category)
        .connection(connection)
        .message(format!("{prefix}: {}", a.summary));
    if let Some(c) = a.connector {
        b = b.connector(c);
    }
    if let Some(p) = a.phase {
        b = b.phase(p);
    }
    if let Some(code) = a.code {
        b = b.code(code);
    }
    b.emit(app);
}
