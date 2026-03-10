use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Root data directory: ~/.local/share/seal-tld/
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("seal-tld")
}

/// Per-app metadata stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalApp {
    /// Keccak256 hash of the zip file (base36-encoded, 192-bit truncated).
    pub hash: String,
    /// Human-readable name (derived from zip filename or directory).
    pub name: String,
    /// When the app was added (unix timestamp).
    pub installed_at: u64,
}

/// Registered app from on-chain SealRegistry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredApp {
    /// Full seal URL (e.g. "https://voltrevo.github--io.seal/seal/calculator").
    pub seal_url: String,
    /// .seal hostname (e.g. "voltrevo.github--io.seal").
    pub hostname: String,
    /// Base path (e.g. "/seal/calculator"). No trailing slash.
    pub base_path: String,
    /// Human-readable name from on-chain AppRecord.
    pub name: String,
    /// Owner address (hex with 0x prefix).
    pub owner: String,
    /// Full 256-bit bundle hash (hex with 0x prefix).
    pub bundle_hash: String,
    /// Truncated base36 content hash (used for site_dir key).
    pub content_hash: String,
    /// Semver version string.
    pub version: String,
    /// When this version was installed locally (unix timestamp).
    pub installed_at: u64,
}

/// Shared daemon state.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<Inner>>,
    pub data_dir: PathBuf,
}

struct Inner {
    /// Local apps keyed by keccak256 hash (base36).
    local_apps: HashMap<String, LocalApp>,
    /// Registered apps keyed by seal_url.
    registered_apps: HashMap<String, RegisteredApp>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir.join("bundles"))?;
        std::fs::create_dir_all(data_dir.join("sites"))?;
        std::fs::create_dir_all(data_dir.join("state"))?;
        std::fs::create_dir_all(data_dir.join("ca"))?;

        let local_apps = Self::load_local_apps(&data_dir)?;
        let registered_apps = Self::load_registered_apps(&data_dir)?;

        Ok(Self {
            inner: Arc::new(RwLock::new(Inner {
                local_apps,
                registered_apps,
            })),
            data_dir,
        })
    }

    fn load_local_apps(data_dir: &Path) -> anyhow::Result<HashMap<String, LocalApp>> {
        let state_dir = data_dir.join("state");
        let mut apps = HashMap::new();

        let entries = match std::fs::read_dir(&state_dir) {
            Ok(e) => e,
            Err(_) => return Ok(apps),
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(app) = serde_json::from_str::<LocalApp>(&data) {
                        apps.insert(app.hash.clone(), app);
                    }
                }
            }
        }

        Ok(apps)
    }

    pub async fn list_local_apps(&self) -> Vec<LocalApp> {
        let inner = self.inner.read().await;
        let mut apps: Vec<_> = inner.local_apps.values().cloned().collect();
        apps.sort_by(|a, b| b.installed_at.cmp(&a.installed_at));
        apps
    }

    pub async fn get_local_app(&self, hash: &str) -> Option<LocalApp> {
        let inner = self.inner.read().await;
        inner.local_apps.get(hash).cloned()
    }

    fn load_registered_apps(data_dir: &Path) -> anyhow::Result<HashMap<String, RegisteredApp>> {
        let state_dir = data_dir.join("state");
        let mut apps = HashMap::new();

        let entries = match std::fs::read_dir(&state_dir) {
            Ok(e) => e,
            Err(_) => return Ok(apps),
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && filename.starts_with("reg-")
            {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(app) = serde_json::from_str::<RegisteredApp>(&data) {
                        apps.insert(app.seal_url.clone(), app);
                    }
                }
            }
        }

        Ok(apps)
    }

    /// Find a registered app matching the given hostname and request path.
    pub async fn find_registered_app(
        &self,
        hostname: &str,
        path: &str,
    ) -> Option<RegisteredApp> {
        let inner = self.inner.read().await;
        inner
            .registered_apps
            .values()
            .find(|app| {
                app.hostname == hostname
                    && (path == app.base_path
                        || path.starts_with(&format!("{}/", app.base_path))
                        || app.base_path.is_empty())
            })
            .cloned()
    }

    pub async fn register_app(&self, app: RegisteredApp) -> anyhow::Result<()> {
        let state_path = self
            .data_dir
            .join("state")
            .join(format!("reg-{}.json", app.content_hash));
        let json = serde_json::to_string_pretty(&app)?;
        std::fs::write(&state_path, &json)?;

        let mut inner = self.inner.write().await;
        inner.registered_apps.insert(app.seal_url.clone(), app);
        Ok(())
    }

    pub async fn list_registered_apps(&self) -> Vec<RegisteredApp> {
        let inner = self.inner.read().await;
        let mut apps: Vec<_> = inner.registered_apps.values().cloned().collect();
        apps.sort_by(|a, b| a.name.cmp(&b.name));
        apps
    }

    pub async fn register_local_app(&self, app: LocalApp) -> anyhow::Result<()> {
        let state_path = self.data_dir.join("state").join(format!("{}.json", app.hash));
        let json = serde_json::to_string_pretty(&app)?;
        std::fs::write(&state_path, &json)?;

        let mut inner = self.inner.write().await;
        inner.local_apps.insert(app.hash.clone(), app);
        Ok(())
    }

    /// Forget a local app: remove its state file and site directory.
    /// Returns Ok(true) if the app was found, Ok(false) if not.
    pub async fn forget_local_app(&self, hash: &str) -> anyhow::Result<bool> {
        let mut inner = self.inner.write().await;
        if inner.local_apps.remove(hash).is_none() {
            return Ok(false);
        }

        let state_path = self.data_dir.join("state").join(format!("{hash}.json"));
        if state_path.exists() {
            std::fs::remove_file(&state_path)?;
        }

        let site_dir = self.data_dir.join("sites").join(hash);
        if site_dir.exists() {
            std::fs::remove_dir_all(&site_dir)?;
        }

        Ok(true)
    }

    /// Path to extracted site content for a local app.
    pub fn site_dir(&self, hash: &str) -> PathBuf {
        self.data_dir.join("sites").join(hash)
    }

    pub fn ca_dir(&self) -> PathBuf {
        self.data_dir.join("ca")
    }

    pub fn pid_file(&self) -> PathBuf {
        self.data_dir.join("seal.pid")
    }
}

pub fn pid_file() -> PathBuf {
    data_dir().join("seal.pid")
}

pub fn write_pid(path: &std::path::Path) -> anyhow::Result<()> {
    let pid = std::process::id();
    std::fs::write(path, pid.to_string())?;
    Ok(())
}

pub fn read_pid(path: &std::path::Path) -> anyhow::Result<Option<u32>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s.trim().parse()?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn remove_pid(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}
