//! Shared classification vocabulary (the "IR") for errors and logs.
//!
//! Both [`crate::error::AppError`] and the structured log event draw their
//! `phase` / `category` / `code` fields from the enums here, so the UI can
//! filter and badge by stable machine values instead of parsing prose. The
//! `connector` axis reuses [`crate::connections::ConnectionKind`] directly —
//! there is no separate connector enum to keep in sync.
//!
//! To teach the app a new failure shape you extend *one* enum + its classifier
//! here; every connector and the frontend inherit it.

// This module is a deliberately complete vocabulary: some variants (e.g.
// `Phase::Passive`, `StatusCode::Sftp`, `Level`) are not emitted yet but exist
// as the documented extension points for new connectors / debug types and the
// log IR (#3). They are not dead code — they are the API surface.
#![allow(dead_code)]

use serde::Serialize;

/// Severity. Mirrors the frontend `LogLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// Coarse, connector-agnostic failure class. This is what the UI badges and what
/// "retry"-style affordances key off. Add a variant when a genuinely new
/// remediation path appears — not for every protocol code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCategory {
    /// Bad / missing / expired credentials.
    Auth,
    /// Authenticated, but the principal is not allowed to do this.
    Permission,
    /// Target object / path / bucket does not exist.
    NotFound,
    /// DNS, TCP, timeout, connection reset — the bytes never made it.
    Network,
    /// TLS handshake / certificate problems.
    Tls,
    /// A well-formed connection but a malformed protocol exchange.
    Protocol,
    /// Throttled; back off and retry.
    RateLimited,
    /// 3xx — usually a wrong region or endpoint.
    Redirect,
    /// 4xx miscellaneous — the request itself is bad.
    Client,
    /// 5xx — the remote is at fault; usually retryable.
    Server,
    /// Local misconfiguration (missing host, malformed endpoint, …).
    Config,
    /// Local filesystem / IO error.
    Io,
    Unknown,
}

impl ErrorCategory {
    /// Whether a fresh attempt could plausibly succeed without user action.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCategory::Network | ErrorCategory::RateLimited | ErrorCategory::Server
        )
    }

    /// A generic, category-level hint. Connectors may override with something
    /// more specific at the call site.
    pub fn remediation(self) -> Option<&'static str> {
        Some(match self {
            ErrorCategory::Auth => "Check the credentials for this connection (key / password / token).",
            ErrorCategory::Permission => {
                "Authenticated, but not permitted. Check the account's access policy for this path."
            }
            ErrorCategory::NotFound => "Verify the path, bucket, or object name still exists.",
            ErrorCategory::Network => {
                "Check your network and the host/port. The server may be unreachable; retrying may help."
            }
            ErrorCategory::Tls => {
                "TLS negotiation failed. Check the certificate, the host name, and that the port speaks TLS."
            }
            ErrorCategory::Protocol => "The server replied unexpectedly. Check protocol/mode settings.",
            ErrorCategory::RateLimited => "The server is throttling requests. Retry after a short backoff.",
            ErrorCategory::Redirect => {
                "The endpoint redirected the request — usually a wrong region or endpoint URL."
            }
            ErrorCategory::Client => "The request was rejected as invalid. Check the path and parameters.",
            ErrorCategory::Server => "The server reported an internal error. Retrying may help.",
            ErrorCategory::Config => "This connection is misconfigured. Check host, endpoint, and region.",
            ErrorCategory::Io => "A local file error occurred. Check the file path, permissions, and disk space.",
            ErrorCategory::Unknown => return None,
        })
    }
}

/// Where in an operation's lifecycle an event happened. This is the axis that
/// makes logs debuggable (filter to `Tls`, `Passive`, …) and the example the
/// brief calls out: add a phase here, emit it from the connector, done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Connect,
    Dns,
    Tcp,
    Tls,
    Handshake,
    Auth,
    Passive,
    List,
    Search,
    Stat,
    Transfer,
    Multipart,
    Delete,
    Mkdir,
    Config,
    /// Raw protocol traffic forwarded from the protocol libraries
    /// (russh kex/cipher negotiation, suppaftp command/response lines, rustls).
    Protocol,
}

/// A protocol-native status code, tagged by which protocol it came from so the
/// UI can render "HTTP 403" vs "FTP 530" vs "SFTP 3" without guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "proto", content = "value", rename_all = "lowercase")]
pub enum StatusCode {
    Http(u16),
    Ftp(u16),
    Sftp(u32),
    Os(i32),
}

// --- Classifiers: protocol code -> category. One place per protocol. ---

/// HTTP/S3 status → category. This is the 3xx/4xx/5xx split the brief asks for.
pub fn category_for_http(status: u16) -> ErrorCategory {
    match status {
        300..=399 => ErrorCategory::Redirect,
        401 => ErrorCategory::Auth,
        403 => ErrorCategory::Permission,
        404 | 410 => ErrorCategory::NotFound,
        408 => ErrorCategory::Network,
        429 => ErrorCategory::RateLimited,
        400..=499 => ErrorCategory::Client,
        500..=599 => ErrorCategory::Server,
        _ => ErrorCategory::Unknown,
    }
}

/// Map an S3 / S3-compatible error *code* string (e.g. "AccessDenied") to a
/// category. Returns `None` when the code is unknown so the caller can fall back
/// to the HTTP status. These strings are stable across SDK versions.
pub fn category_for_s3_code(code: &str) -> Option<ErrorCategory> {
    Some(match code {
        "AccessDenied" | "AllAccessDisabled" => ErrorCategory::Permission,
        "InvalidAccessKeyId"
        | "SignatureDoesNotMatch"
        | "InvalidToken"
        | "ExpiredToken"
        | "TokenRefreshRequired"
        | "InvalidSecurity" => ErrorCategory::Auth,
        "NoSuchBucket" | "NoSuchKey" | "NoSuchUpload" | "NotFound" => ErrorCategory::NotFound,
        "SlowDown" | "RequestLimitExceeded" | "Throttling" | "ThrottlingException" => {
            ErrorCategory::RateLimited
        }
        "PermanentRedirect" | "TemporaryRedirect" | "AuthorizationHeaderMalformed" => {
            // region / endpoint mismatch
            ErrorCategory::Redirect
        }
        "InternalError" | "ServiceUnavailable" => ErrorCategory::Server,
        "InvalidBucketName" | "InvalidRequest" | "InvalidArgument" | "MalformedXML"
        | "EntityTooLarge" | "MissingContentLength" => ErrorCategory::Client,
        _ => return None,
    })
}

/// FTP reply code → category (RFC 959 families: 4xx transient, 5xx permanent).
pub fn category_for_ftp(code: u16) -> ErrorCategory {
    match code {
        421 | 425 | 426 | 434 => ErrorCategory::Network, // service/data-connection problems
        430 | 530 | 532 => ErrorCategory::Auth,
        550 | 551 | 553 => ErrorCategory::NotFound,
        552 => ErrorCategory::Permission, // exceeded storage / not allowed
        450 | 451 | 452 => ErrorCategory::Server, // transient action failures
        500..=504 => ErrorCategory::Protocol, // syntax / not implemented
        400..=499 => ErrorCategory::Server,
        500..=599 => ErrorCategory::Client,
        _ => ErrorCategory::Unknown,
    }
}

/// Whether an FTP reply code is worth auto-retrying (4xx transient family).
pub fn ftp_retryable(code: u16) -> bool {
    matches!(code, 421 | 425 | 426 | 450 | 451)
}

/// russh-sftp surfaces SSH_FX_* status as a stringified message rather than the
/// numeric code, so we classify SFTP failures from the message text. `phase`
/// gives the fallback when the message is opaque.
pub fn category_for_sftp(phase: Phase, msg: &str) -> ErrorCategory {
    let m = msg.to_lowercase();
    if m.contains("permission denied") {
        ErrorCategory::Permission
    } else if m.contains("no such file") || m.contains("not found") {
        ErrorCategory::NotFound
    } else if m.contains("no connection") || m.contains("connection lost") || m.contains("eof") {
        ErrorCategory::Network
    } else if m.contains("bad message") || m.contains("unsupported") {
        ErrorCategory::Protocol
    } else {
        match phase {
            Phase::Auth => ErrorCategory::Auth,
            Phase::Connect | Phase::Tcp | Phase::Dns => ErrorCategory::Network,
            Phase::Handshake | Phase::Tls => ErrorCategory::Tls,
            _ => ErrorCategory::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_families() {
        // 3xx redirect
        assert_eq!(category_for_http(301), ErrorCategory::Redirect);
        assert_eq!(category_for_http(307), ErrorCategory::Redirect);
        // auth / permission split
        assert_eq!(category_for_http(401), ErrorCategory::Auth);
        assert_eq!(category_for_http(403), ErrorCategory::Permission);
        assert_eq!(category_for_http(404), ErrorCategory::NotFound);
        assert_eq!(category_for_http(410), ErrorCategory::NotFound);
        assert_eq!(category_for_http(408), ErrorCategory::Network);
        assert_eq!(category_for_http(429), ErrorCategory::RateLimited);
        // generic 4xx -> client
        assert_eq!(category_for_http(400), ErrorCategory::Client);
        assert_eq!(category_for_http(422), ErrorCategory::Client);
        // 5xx -> server
        assert_eq!(category_for_http(500), ErrorCategory::Server);
        assert_eq!(category_for_http(503), ErrorCategory::Server);
        // out of range
        assert_eq!(category_for_http(200), ErrorCategory::Unknown);
    }

    #[test]
    fn s3_error_codes() {
        assert_eq!(category_for_s3_code("AccessDenied"), Some(ErrorCategory::Permission));
        assert_eq!(category_for_s3_code("InvalidAccessKeyId"), Some(ErrorCategory::Auth));
        assert_eq!(category_for_s3_code("SignatureDoesNotMatch"), Some(ErrorCategory::Auth));
        assert_eq!(category_for_s3_code("NoSuchBucket"), Some(ErrorCategory::NotFound));
        assert_eq!(category_for_s3_code("NoSuchKey"), Some(ErrorCategory::NotFound));
        assert_eq!(category_for_s3_code("SlowDown"), Some(ErrorCategory::RateLimited));
        assert_eq!(category_for_s3_code("PermanentRedirect"), Some(ErrorCategory::Redirect));
        assert_eq!(category_for_s3_code("InternalError"), Some(ErrorCategory::Server));
        // unknown code -> caller falls back to HTTP status
        assert_eq!(category_for_s3_code("SomeNewCode"), None);
    }

    #[test]
    fn ftp_reply_codes() {
        assert_eq!(category_for_ftp(530), ErrorCategory::Auth); // not logged in
        assert_eq!(category_for_ftp(550), ErrorCategory::NotFound); // file unavailable
        assert_eq!(category_for_ftp(552), ErrorCategory::Permission); // over quota
        assert_eq!(category_for_ftp(421), ErrorCategory::Network); // service not available
        assert_eq!(category_for_ftp(425), ErrorCategory::Network); // can't open data conn
        assert_eq!(category_for_ftp(451), ErrorCategory::Server); // transient action failure
        assert_eq!(category_for_ftp(500), ErrorCategory::Protocol); // syntax error
        assert_eq!(category_for_ftp(505), ErrorCategory::Client); // other permanent 5xx
    }

    #[test]
    fn ftp_retryable_only_transient() {
        assert!(ftp_retryable(421));
        assert!(ftp_retryable(425));
        assert!(ftp_retryable(450));
        assert!(!ftp_retryable(530)); // auth is not retryable
        assert!(!ftp_retryable(550)); // not-found is not retryable
    }

    #[test]
    fn sftp_message_and_phase_fallback() {
        // message-driven
        assert_eq!(
            category_for_sftp(Phase::Transfer, "Permission denied"),
            ErrorCategory::Permission
        );
        assert_eq!(
            category_for_sftp(Phase::Transfer, "No such file"),
            ErrorCategory::NotFound
        );
        assert_eq!(
            category_for_sftp(Phase::Transfer, "connection lost"),
            ErrorCategory::Network
        );
        // opaque message -> phase fallback
        assert_eq!(category_for_sftp(Phase::Auth, "whatever"), ErrorCategory::Auth);
        assert_eq!(category_for_sftp(Phase::Connect, "whatever"), ErrorCategory::Network);
        assert_eq!(category_for_sftp(Phase::Tls, "whatever"), ErrorCategory::Tls);
    }

    #[test]
    fn category_retryability() {
        assert!(ErrorCategory::Network.retryable());
        assert!(ErrorCategory::RateLimited.retryable());
        assert!(ErrorCategory::Server.retryable());
        assert!(!ErrorCategory::Auth.retryable());
        assert!(!ErrorCategory::Permission.retryable());
        assert!(!ErrorCategory::NotFound.retryable());
    }
}
