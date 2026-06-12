use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEntry {
    pub name: String,
    pub path: String,
    pub kind: String, // "folder" | "file"
    pub size: Option<u64>,
    pub modified: Option<DateTime<Utc>>,
    pub hidden: bool,
}

pub fn home_dir() -> Result<String> {
    let p = dirs_home_fallback();
    Ok(p.to_string_lossy().to_string())
}

fn dirs_home_fallback() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            return std::path::PathBuf::from(p);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(p) = std::env::var("HOME") {
            return std::path::PathBuf::from(p);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
}

pub fn list(path: &str, show_hidden: bool) -> Result<Vec<LocalEntry>> {
    let root = Path::new(path);
    let mut out = Vec::new();
    let rd = std::fs::read_dir(root)?;
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let hidden = name.starts_with('.');
        if hidden && !show_hidden {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = meta.is_dir();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .and_then(|d| DateTime::<Utc>::from_timestamp(d.as_secs() as i64, 0));
        let full = entry.path().to_string_lossy().to_string();
        out.push(LocalEntry {
            name,
            path: full,
            kind: if is_dir { "folder" } else { "file" }.into(),
            size: if is_dir { None } else { Some(meta.len()) },
            modified,
            hidden,
        });
    }
    out.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
        ("folder", "file") => std::cmp::Ordering::Less,
        ("file", "folder") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

pub fn parent(path: &str) -> Result<String> {
    let p = Path::new(path);
    Ok(p.parent()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string()))
}
