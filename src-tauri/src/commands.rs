use crate::connections::{Connection, ConnectionSecret, Store};
use crate::error::{Error, Result};
use crate::localfs::{self, LocalEntry};
use crate::logging::{log_error, LogBuilder};
use crate::remote;
use crate::s3;
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
    connection: Connection,
    secret: Option<ConnectionSecret>,
) -> Result<Connection> {
    store.upsert(connection, secret)
}

#[tauri::command]
pub async fn delete_connection(
    store: State<'_, Arc<Store>>,
    id: String,
) -> Result<()> {
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
    match remote::list(&c, &s, &path).await {
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
            log_error(&app, "connection", Some(&c.name), "List failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_search(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
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
    match remote::search(&c, &s, &path, &query).await {
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
            log_error(&app, "connection", Some(&c.name), "Search failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_download(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
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
    match remote::download(&app, &c, &s, &path, PathBuf::from(dest).as_path(), id.clone()).await {
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
            log_error(&app, "transfer", Some(&c.name), "Download failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_upload(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
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
    match remote::upload(&app, &c, &s, PathBuf::from(&src).as_path(), &path, id.clone()).await {
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
            log_error(&app, "transfer", Some(&c.name), "Upload failed", &e);
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn remote_delete(
    app: AppHandle,
    store: State<'_, Arc<Store>>,
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
    match remote::delete(&c, &s, &path).await {
        Ok(()) => Ok(()),
        Err(e) => {
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
