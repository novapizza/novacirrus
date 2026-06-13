//! Error IR.
//!
//! [`AppError`] is the structured, connector-agnostic error that crosses the
//! Tauri boundary. Every command serializes its failure as an `AppError` object
//! (not a string), so the frontend can badge the category, show a remediation
//! hint, and offer retry only when `retryable`.
//!
//! Connectors build a classified `AppError` at the failure site (see
//! `classify_*` helpers in each backend + [`crate::taxonomy`]); the legacy
//! `Error` variants below remain for ergonomic `?` conversion and are mapped to
//! a best-effort `AppError` at serialize time.

use crate::connections::ConnectionKind;
use crate::taxonomy::{ErrorCategory, Phase, StatusCode};
use serde::{Serialize, Serializer};

/// The structured error surfaced to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub category: ErrorCategory,
    pub connector: Option<ConnectionKind>,
    pub phase: Option<Phase>,
    pub code: Option<StatusCode>,
    pub retryable: bool,
    /// One-line, human-readable summary.
    pub summary: String,
    /// Raw protocol/library text for power users (collapsible in the UI).
    pub detail: Option<String>,
    /// Actionable hint. Falls back to a category-level default when unset.
    pub remediation: Option<String>,
}

impl AppError {
    pub fn new(category: ErrorCategory, summary: impl Into<String>) -> Self {
        Self {
            category,
            connector: None,
            phase: None,
            code: None,
            retryable: category.retryable(),
            summary: summary.into(),
            detail: None,
            remediation: None,
        }
    }

    pub fn connector(mut self, c: impl Into<ConnectionKind>) -> Self {
        self.connector = Some(c.into());
        self
    }

    pub fn phase(mut self, p: Phase) -> Self {
        self.phase = Some(p);
        self
    }

    pub fn code(mut self, c: StatusCode) -> Self {
        self.code = Some(c);
        self
    }

    pub fn retryable(mut self, r: bool) -> Self {
        self.retryable = r;
        self
    }

    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }

    pub fn remediation(mut self, r: impl Into<String>) -> Self {
        self.remediation = Some(r.into());
        self
    }

    /// Fill in a category-default remediation when the call site didn't set one.
    fn with_default_remediation(mut self) -> Self {
        if self.remediation.is_none() {
            self.remediation = self.category.remediation().map(str::to_string);
        }
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("keyring: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("tauri: {0}")]
    Tauri(#[from] tauri::Error),
    // Retained for back-compat and future use; connectors now emit `App` with a
    // classified `AppError` instead. Mapped in `to_app` so they still serialize.
    #[allow(dead_code)]
    #[error("s3: {0}")]
    S3(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[allow(dead_code)]
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Msg(String),
    /// Fully classified error from a connector.
    #[error("{0}")]
    App(AppError),
}

impl Error {
    /// Project any `Error` onto the `AppError` IR. Rich connector errors pass
    /// through; legacy/string variants get a best-effort classification.
    pub fn to_app(&self) -> AppError {
        let app = match self {
            Error::App(a) => a.clone(),
            Error::Io(e) => AppError::new(ErrorCategory::Io, e.to_string()),
            Error::Json(e) => AppError::new(ErrorCategory::Protocol, format!("Malformed data: {e}")),
            Error::Keyring(e) => {
                AppError::new(ErrorCategory::Config, format!("Secret store error: {e}"))
                    .remediation("Could not read the OS keychain. Re-save the connection's credentials.")
            }
            Error::Tauri(e) => AppError::new(ErrorCategory::Unknown, e.to_string()),
            Error::S3(s) => AppError::new(ErrorCategory::Unknown, s.clone()).connector(ConnectionKind::S3),
            Error::NotFound(s) => AppError::new(ErrorCategory::NotFound, format!("Not found: {s}")),
            Error::Unsupported(s) => AppError::new(ErrorCategory::Client, s.clone()),
            Error::Msg(s) => AppError::new(ErrorCategory::Unknown, s.clone()),
        };
        app.with_default_remediation()
    }
}

impl From<AppError> for Error {
    fn from(a: AppError) -> Self {
        Error::App(a)
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.to_app().serialize(s)
    }
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Msg(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
