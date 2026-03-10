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
    /// Keccak256 hash of the zip file (hex, no 0x prefix).
    pub hash: String,
    /// Human-readable name (derived from zip filename or directory).
    pub name: String,
    /// When the app was installed (unix timestamp).
    pub installed_at: u64,
}

/// Shared daemon state.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<Inner>>,
    pub data_dir: PathBuf,
}

struct Inner {
    /// Local apps keyed by keccak256 hash (hex).
    local_apps: HashMap<String, LocalApp>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir.join("bundles"))?;
        std::fs::create_dir_all(data_dir.join("sites"))?;
        std::fs::create_dir_all(data_dir.join("state"))?;
        std::fs::create_dir_all(data_dir.join("ca"))?;

        let local_apps = Self::load_local_apps(&data_dir)?;

        Ok(Self {
            inner: Arc::new(RwLock::new(Inner { local_apps })),
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

    pub async fn register_local_app(&self, app: LocalApp) -> anyhow::Result<()> {
        let state_path = self.data_dir.join("state").join(format!("{}.json", app.hash));
        let json = serde_json::to_string_pretty(&app)?;
        std::fs::write(&state_path, &json)?;

        let mut inner = self.inner.write().await;
        inner.local_apps.insert(app.hash.clone(), app);
        Ok(())
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
