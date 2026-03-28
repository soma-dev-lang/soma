use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// soma.lock — exact resolved versions and hashes
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LockFile {
    pub packages: HashMap<String, LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    /// Source: git URL, local path, or registry
    pub source: String,
    /// Git commit hash or content hash
    pub hash: String,
    /// Files included in this package
    pub files: Vec<String>,
}

impl LockFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("invalid soma.lock: {}", e))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize lock: {}", e))?;
        std::fs::write(path, content)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))
    }

    pub fn is_locked(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.get(name)
    }
}
