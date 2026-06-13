//! Live session pool — the difference between this and a stateless tool.
//!
//! SFTP/FTP(S) keep one authenticated connection open per connection id and
//! reuse it across every list/transfer/delete, the way a real FTP client does,
//! until the user disconnects or the server drops the pipe. S3 is stateless
//! (an HTTPS request per call) but is pooled too as a cheap handle so the UI's
//! connect / connected / disconnect model is uniform across protocols.
//!
//! The session itself lives inside the backend (`Arc<dyn Remote>`); this module
//! only owns the id→backend map and the connect/disconnect/evict lifecycle.

use crate::connections::{Connection, ConnectionSecret};
use crate::error::{Error, Result};
use crate::remote::{self, Remote};
use crate::taxonomy::ErrorCategory;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

/// Emitted to the frontend when a pooled session goes away on its own (the
/// server closed the connection mid-operation), so the sidebar can drop its
/// "connected" indicator without the user having clicked Disconnect.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DisconnectedEvent {
    id: String,
    reason: String,
}

#[derive(Default)]
pub struct SessionPool {
    map: Mutex<HashMap<String, Arc<dyn Remote>>>,
}

impl SessionPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the live session for `c`, opening and caching one if absent.
    /// Operations call this; the cache hit is what makes browsing reuse a single
    /// connection instead of reconnecting per request.
    pub async fn get_or_connect(
        &self,
        c: &Connection,
        s: &ConnectionSecret,
    ) -> Result<Arc<dyn Remote>> {
        if let Some(b) = self.map.lock().await.get(&c.id).cloned() {
            return Ok(b);
        }
        // Open OUTSIDE the lock — an SFTP/FTP handshake takes seconds and must
        // not stall operations on other connections. A rare concurrent
        // double-open is harmless: `or_insert` keeps the first, and the loser
        // `Arc` closes its socket when it drops.
        let backend = remote::open_backend(c, s).await?;
        let mut map = self.map.lock().await;
        Ok(map.entry(c.id.clone()).or_insert(backend).clone())
    }

    /// The explicit "Connect" action: establish (or reuse) the session eagerly so
    /// the handshake — and any auth/host-key error — happens now, not on first list.
    pub async fn connect(&self, c: &Connection, s: &ConnectionSecret) -> Result<()> {
        self.get_or_connect(c, s).await.map(|_| ())
    }

    /// The explicit "Disconnect" action: drop the session, closing it gracefully.
    /// No-op if not connected.
    pub async fn disconnect(&self, id: &str) {
        let backend = self.map.lock().await.remove(id);
        if let Some(b) = backend {
            b.disconnect().await;
        }
    }

    /// Forget a session without a graceful close — used when the connection is
    /// edited or deleted, so the next operation reopens with fresh settings.
    pub async fn evict(&self, id: &str) {
        self.map.lock().await.remove(id);
    }

    pub async fn is_connected(&self, id: &str) -> bool {
        self.map.lock().await.contains_key(id)
    }

    /// Inspect an operation error: if it looks like the connection itself died
    /// (network / TLS level), drop the dead session so the next call reconnects,
    /// and notify the UI so it can clear the "connected" state.
    pub async fn note_op_error(&self, app: &AppHandle, id: &str, e: &Error) {
        let app_err = e.to_app();
        if !matches!(app_err.category, ErrorCategory::Network | ErrorCategory::Tls) {
            return;
        }
        if self.map.lock().await.remove(id).is_some() {
            let _ = app.emit(
                "disconnected",
                DisconnectedEvent { id: id.to_string(), reason: app_err.summary },
            );
        }
    }
}
