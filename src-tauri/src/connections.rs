use crate::error::Result;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// What kind of remote this connection talks to.
/// S3, R2, and S3-compatible all use the AWS S3 protocol — they differ only
/// in the endpoint URL and the conventional region string.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionKind {
    S3,
    R2,
    S3Compat,
    Sftp,
    Ftp,
    Ftps,
}

/// FTP/FTPS data-channel mode. `Passive` (PASV) is the most widely compatible
/// default; `ExtendedPassive` (EPSV) is required by some IPv6 servers; `Active`
/// (PORT) has the server connect back to the client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FtpMode {
    Active,
    #[default]
    Passive,
    ExtendedPassive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub kind: ConnectionKind,

    // Display / connection metadata
    pub host: Option<String>,        // e.g. s3.amazonaws.com or sftp.example.com
    pub port: Option<u16>,
    pub region: Option<String>,      // S3 region (us-east-1, auto for R2)
    pub endpoint: Option<String>,    // explicit endpoint URL override (R2 / compat)
    pub bucket: Option<String>,      // optional default bucket
    pub default_path: Option<String>,

    pub username: Option<String>,    // SFTP user, or "AKIA…" display
    pub use_path_style: bool,        // S3 path-style addressing (compat)

    // FTP / FTPS tuning. Defaulted so connections saved before these existed
    // (and non-FTP kinds) deserialize cleanly.
    #[serde(default)]
    pub ftp_mode: Option<FtpMode>,   // data-channel mode (None = Passive)
    #[serde(default)]
    pub ftps_implicit: bool,         // FTPS: implicit TLS (port 990) vs explicit AUTH TLS

    pub color: String,               // accent dot for sidebar
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// Secret payload kept out of the on-disk JSON; persisted via the OS keyring.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSecret {
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
    pub password: Option<String>,
    pub private_key_pem: Option<String>,
    pub passphrase: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct StoreFile {
    connections: Vec<Connection>,
}

pub struct Store {
    path: PathBuf,
    inner: RwLock<StoreFile>,
    /// In-memory cache of decrypted secrets, keyed by connection id.
    ///
    /// Without this, every remote operation (listing, navigating, transfers)
    /// re-reads from the OS keyring, which on macOS pops a keychain auth prompt
    /// per access. We hit the keyring at most once per connection per run.
    secrets: RwLock<HashMap<String, ConnectionSecret>>,
}

impl Store {
    pub fn load(app: &AppHandle) -> Result<Arc<Self>> {
        let dir = app.path().app_config_dir()?;
        fs::create_dir_all(&dir)?;
        let path = dir.join("connections.json");
        let inner = if path.exists() {
            let bytes = fs::read(&path)?;
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            StoreFile::default()
        };
        Ok(Arc::new(Self {
            path,
            inner: RwLock::new(inner),
            secrets: RwLock::new(HashMap::new()),
        }))
    }

    pub fn list(&self) -> Vec<Connection> {
        self.inner.read().connections.clone()
    }

    pub fn get(&self, id: &str) -> Option<Connection> {
        self.inner
            .read()
            .connections
            .iter()
            .find(|c| c.id == id)
            .cloned()
    }

    fn persist(&self) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&*self.inner.read())?;
        fs::write(&self.path, bytes)?;
        Ok(())
    }

    pub fn upsert(&self, mut c: Connection, secret: Option<ConnectionSecret>) -> Result<Connection> {
        if c.id.is_empty() {
            c.id = Uuid::new_v4().to_string();
        }
        if c.created_at.is_none() {
            c.created_at = Some(Utc::now());
        }
        {
            let mut g = self.inner.write();
            if let Some(slot) = g.connections.iter_mut().find(|x| x.id == c.id) {
                *slot = c.clone();
            } else {
                g.connections.push(c.clone());
            }
        }
        self.persist()?;
        if let Some(s) = secret {
            #[cfg(debug_assertions)]
            self.dev_store_secret(&c.id, &s)?;
            #[cfg(not(debug_assertions))]
            write_secret(&c.id, &s)?;
            self.secrets.write().insert(c.id.clone(), s);
        }
        Ok(c)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        {
            let mut g = self.inner.write();
            g.connections.retain(|c| c.id != id);
        }
        self.persist()?;
        self.secrets.write().remove(id);
        #[cfg(debug_assertions)]
        self.dev_remove_secret(id);
        #[cfg(not(debug_assertions))]
        {
            let _ = clear_secret(id); // best-effort
        }
        Ok(())
    }

    pub fn read_secret(&self, id: &str) -> Result<ConnectionSecret> {
        // Fast path: already cached.
        if let Some(s) = self.secrets.read().get(id) {
            return Ok(s.clone());
        }
        // Slow path under the write lock. Selecting a connection fires several
        // commands at once, each calling read_secret; without this they would
        // all miss the cache simultaneously and each hit the backend, firing
        // multiple keychain prompts. Holding the write lock across the load
        // makes concurrent first-readers wait, so the backend is hit ONCE.
        let mut cache = self.secrets.write();
        if let Some(s) = cache.get(id) {
            return Ok(s.clone());
        }
        // Debug builds read ONLY the dev file — the keychain is never touched,
        // so there is no prompt, ever. A connection whose secret isn't in the
        // dev file yet returns empty; re-saving the connection populates it.
        #[cfg(debug_assertions)]
        let s = self.dev_load().get(id).cloned().unwrap_or_default();
        #[cfg(not(debug_assertions))]
        let s = read_secret(id)?;
        cache.insert(id.to_string(), s.clone());
        Ok(s)
    }

    // --- Dev-only secret store (debug builds) ---------------------------------
    //
    // In debug builds, secrets live in a plaintext JSON file alongside
    // connections.json so local development NEVER triggers the macOS keychain
    // (an unsigned/ad-hoc dev binary re-prompts for the keychain password on
    // every access). Release builds use the OS keychain / Touch ID via
    // `secret_store` — see the release branches above.

    #[cfg(debug_assertions)]
    fn dev_secrets_path(&self) -> PathBuf {
        self.path.with_file_name("dev-secrets.json")
    }

    #[cfg(debug_assertions)]
    fn dev_load(&self) -> HashMap<String, ConnectionSecret> {
        fs::read(self.dev_secrets_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    #[cfg(debug_assertions)]
    fn dev_store_secret(&self, id: &str, s: &ConnectionSecret) -> Result<()> {
        let mut m = self.dev_load();
        m.insert(id.to_string(), s.clone());
        fs::write(self.dev_secrets_path(), serde_json::to_vec_pretty(&m)?)?;
        Ok(())
    }

    #[cfg(debug_assertions)]
    fn dev_remove_secret(&self, id: &str) {
        let mut m = self.dev_load();
        if m.remove(id).is_some() {
            if let Ok(bytes) = serde_json::to_vec_pretty(&m) {
                let _ = fs::write(self.dev_secrets_path(), bytes);
            }
        }
    }
}

// Release builds persist secrets in the OS keychain (Touch ID on macOS) via
// `secret_store`. Debug builds use the in-Store dev file and never call these.
#[cfg(not(debug_assertions))]
fn write_secret(id: &str, s: &ConnectionSecret) -> Result<()> {
    let json = serde_json::to_string(s)?;
    crate::secret_store::write(id, &json)
}

#[cfg(not(debug_assertions))]
fn read_secret(id: &str) -> Result<ConnectionSecret> {
    match crate::secret_store::read(id)? {
        Some(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
        None => Ok(ConnectionSecret::default()),
    }
}

#[cfg(not(debug_assertions))]
fn clear_secret(id: &str) -> Result<()> {
    crate::secret_store::clear(id)
}
