use crate::connections::{Connection, ConnectionSecret, Store};
use crate::error::{Error, Result};
use crate::localfs::{self, LocalEntry};
use crate::logging::{log_error, LogBuilder};
use crate::remote::{self, Remote};
use crate::s3;
use crate::session::SessionPool;
use crate::taxonomy::{Level, Phase};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};
use uuid::Uuid;

#[tauri::command]
pub async fn list_connections(store: State<'_, Arc<Store>>) -> Result<Vec<Connection>> {
    Ok(store.list())
}

#[tauri::command]
pub async fn upsert_connection(
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    connection: Connection,
    secret: Option<ConnectionSecret>,
) -> Result<Connection> {
    // Drop any live session so edited host/credentials take effect on reconnect.
    pool.evict(&connection.id).await;
    store.upsert(connection, secret)
}

#[tauri::command]
pub async fn delete_connection(
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    id: String,
) -> Result<()> {
    pool.disconnect(&id).await; // close any live session before the config is gone
    store.delete(&id)
}

/// Verify we can reach the remote. Dispatches to the right backend.
#[tauri::command]
pub async fn test_connection(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    id: String,
) -> Result<String> {
    let c = store.get(&id).ok_or_else(|| Error::NotFound(id.clone()))?;
    let s = store.read_secret(&id)?;
    LogBuilder::new(Level::Info, "connection")
        .connector(c.kind)
        .phase(Phase::Connect)
        .connection(Some(&c.name))
        .message("Testing connection")
        .emit(&app);
    match remote::test(&c, &s).await {
        Ok(msg) => {
            LogBuilder::new(Level::Info, "connection")
                .connector(c.kind)
                .phase(Phase::Connect)
                .connection(Some(&c.name))
                .message(&msg)
                .emit(&app);
            Ok(msg)
        }
        Err(e) => {
            log_error(&app, "connection", Some(&c.name), "Test failed", &e);
            Err(e)
        }
    }
}

/// Open and pool a live session (the explicit "Connect" action). The handshake
/// — and any auth / host-key failure — happens here, so the UI can show a
/// connecting state and surface errors before showing the remote as connected.
#[tauri::command]
pub async fn connect(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    id: String,
) -> Result<()> {
    let c = store.get(&id).ok_or_else(|| Error::NotFound(id.clone()))?;
    let s = store.read_secret(&id)?;
    LogBuilder::new(Level::Info, "connection")
        .connector(c.kind)
        .phase(Phase::Connect)
        .connection(Some(&c.name))
        .message("Connecting")
        .emit(&app);
    match pool.connect(&c, &s).await {
        Ok(()) => {
            LogBuilder::new(Level::Info, "connection")
                .connector(c.kind)
                .phase(Phase::Connect)
                .connection(Some(&c.name))
                .message("Connected")
                .emit(&app);
            Ok(())
        }
        Err(e) => {
            log_error(&app, "connection", Some(&c.name), "Connect failed", &e);
            Err(e)
        }
    }
}

/// Close a pooled session (the explicit "Disconnect" action). Idempotent.
#[tauri::command]
pub async fn disconnect(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    id: String,
) -> Result<()> {
    pool.disconnect(&id).await;
    if let Some(c) = store.get(&id) {
        LogBuilder::new(Level::Info, "connection")
            .connector(c.kind)
            .phase(Phase::Connect)
            .connection(Some(&c.name))
            .message("Disconnected")
            .emit(&app);
    }
    Ok(())
}

/// Whether a connection currently has a live pooled session.
#[tauri::command]
pub async fn is_connected(pool: State<'_, Arc<SessionPool>>, id: String) -> Result<bool> {
    Ok(pool.is_connected(&id).await)
}

/// Short, log-friendly form of a path: its last segment (or "/" for the root).
fn display_path(p: &str) -> String {
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

// --- Unified remote commands (any backend) ---

#[tauri::command]
pub async fn remote_list(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    connection_id: String,
    path: String,
) -> Result<Vec<s3::ObjectEntry>> {
    let c = store.get(&connection_id).ok_or_else(|| Error::NotFound(connection_id.clone()))?;
    let s = store.read_secret(&connection_id)?;
    LogBuilder::new(Level::Debug, "connection")
        .connector(c.kind)
        .phase(Phase::List)
        .connection(Some(&c.name))
        .message(format!("List {}", display_path(&path)))
        .emit(&app);
    let backend = resolve_backend(&app, &pool, &c, &s).await?;
    match backend.list(&path).await {
        Ok(v) => {
            LogBuilder::new(Level::Debug, "connection")
                .connector(c.kind)
                .phase(Phase::List)
                .connection(Some(&c.name))
                .field("count", v.len() as i64)
                .message(format!("Listed {} item(s)", v.len()))
                .emit(&app);
            Ok(v)
        }
        Err(e) => {
            pool.note_op_error(&app, &connection_id, &e).await;
            log_error(&app, "connection", Some(&c.name), "List failed", &e);
            Err(e)
        }
    }
}

/// Resolve the live pooled backend for `c`, logging a connect failure. Shared by
/// the remote_* commands so a dropped session reconnects transparently on use.
async fn resolve_backend(
    app: &AppHandle,
    pool: &SessionPool,
    c: &Connection,
    s: &ConnectionSecret,
) -> Result<Arc<dyn Remote>> {
    pool.get_or_connect(c, s).await.map_err(|e| {
        log_error(app, "connection", Some(&c.name), "Connect failed", &e);
        e
    })
}

#[tauri::command]
pub async fn remote_search(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    connection_id: String,
    path: String,
    query: String,
) -> Result<Vec<s3::ObjectEntry>> {
    let c = store.get(&connection_id).ok_or_else(|| Error::NotFound(connection_id.clone()))?;
    let s = store.read_secret(&connection_id)?;
    LogBuilder::new(Level::Info, "connection")
        .connector(c.kind)
        .phase(Phase::Search)
        .connection(Some(&c.name))
        .message(format!("Search \"{query}\" in {}", display_path(&path)))
        .emit(&app);
    let backend = resolve_backend(&app, &pool, &c, &s).await?;
    match remote::search(backend.as_ref(), &path, &query).await {
        Ok(v) => {
            LogBuilder::new(Level::Info, "connection")
                .connector(c.kind)
                .phase(Phase::Search)
                .connection(Some(&c.name))
                .field("count", v.len() as i64)
                .message(format!("Search found {} result(s)", v.len()))
                .emit(&app);
            Ok(v)
        }
        Err(e) => {
            pool.note_op_error(&app, &connection_id, &e).await;
            log_error(&app, "connection", Some(&c.name), "Search failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_download(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    connection_id: String,
    path: String,
    dest: String,
) -> Result<String> {
    let c = store.get(&connection_id).ok_or_else(|| Error::NotFound(connection_id.clone()))?;
    let s = store.read_secret(&connection_id)?;
    let id = Uuid::new_v4().to_string();
    LogBuilder::new(Level::Info, "transfer")
        .connector(c.kind)
        .phase(Phase::Transfer)
        .connection(Some(&c.name))
        .message(format!("Download {}", display_path(&path)))
        .emit(&app);
    let backend = resolve_backend(&app, &pool, &c, &s).await?;
    match backend.download(&app, &path, PathBuf::from(dest).as_path(), id.clone()).await {
        Ok(()) => {
            LogBuilder::new(Level::Info, "transfer")
                .connector(c.kind)
                .phase(Phase::Transfer)
                .connection(Some(&c.name))
                .message(format!("Downloaded {}", display_path(&path)))
                .emit(&app);
            Ok(id)
        }
        Err(e) => {
            pool.note_op_error(&app, &connection_id, &e).await;
            log_error(&app, "transfer", Some(&c.name), "Download failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_upload(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    connection_id: String,
    src: String,
    path: String,
) -> Result<String> {
    let c = store.get(&connection_id).ok_or_else(|| Error::NotFound(connection_id.clone()))?;
    let s = store.read_secret(&connection_id)?;
    let id = Uuid::new_v4().to_string();
    let name = display_path(&src);
    LogBuilder::new(Level::Info, "transfer")
        .connector(c.kind)
        .phase(Phase::Transfer)
        .connection(Some(&c.name))
        .message(format!("Upload {name}"))
        .emit(&app);
    let backend = resolve_backend(&app, &pool, &c, &s).await?;
    match remote::upload(backend.as_ref(), &app, PathBuf::from(&src).as_path(), &path, id.clone()).await {
        Ok(()) => {
            LogBuilder::new(Level::Info, "transfer")
                .connector(c.kind)
                .phase(Phase::Transfer)
                .connection(Some(&c.name))
                .message(format!("Uploaded {name}"))
                .emit(&app);
            Ok(id)
        }
        Err(e) => {
            pool.note_op_error(&app, &connection_id, &e).await;
            log_error(&app, "transfer", Some(&c.name), "Upload failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_delete(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
    pool: State<'_, Arc<SessionPool>>,
    connection_id: String,
    path: String,
) -> Result<()> {
    let c = store.get(&connection_id).ok_or_else(|| Error::NotFound(connection_id.clone()))?;
    let s = store.read_secret(&connection_id)?;
    LogBuilder::new(Level::Warn, "connection")
        .connector(c.kind)
        .phase(Phase::Delete)
        .connection(Some(&c.name))
        .message(format!("Delete {}", display_path(&path)))
        .emit(&app);
    let backend = resolve_backend(&app, &pool, &c, &s).await?;
    match backend.delete(&path).await {
        Ok(()) => Ok(()),
        Err(e) => {
            pool.note_op_error(&app, &connection_id, &e).await;
            log_error(&app, "connection", Some(&c.name), "Delete failed", &e);
            Err(e)
        }
    }
}

// --- Local filesystem ---

#[tauri::command]
pub async fn fs_home() -> Result<String> {
    localfs::home_dir()
}

#[tauri::command]
pub async fn fs_list(path: String, show_hidden: Option<bool>) -> Result<Vec<LocalEntry>> {
    localfs::list(&path, show_hidden.unwrap_or(false))
}

#[tauri::command]
pub async fn fs_parent(path: String) -> Result<String> {
    localfs::parent(&path)
}

// --- Window controls ---

#[tauri::command]
pub async fn window_close(window: tauri::Window) -> Result<()> {
    window.close()?;
    Ok(())
}

#[tauri::command]
pub async fn window_minimize(window: tauri::Window) -> Result<()> {
    window.minimize()?;
    Ok(())
}

#[tauri::command]
pub async fn window_toggle_maximize(window: tauri::Window) -> Result<()> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize()?;
    } else {
        window.maximize()?;
    }
    Ok(())
}
