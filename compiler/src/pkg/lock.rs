use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// soma.lock — exact resolved versions and hashes
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub packages: HashMap<String, LockedPackage>,
    /// 3.0: external tools, pinned by `soma tools sync` — key `server/name`
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tools: std::collections::BTreeMap<String, LockedTool>,
}

/// The contract of one external tool as it was when it was pinned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockedTool {
    /// "mcp" or "openapi"
    pub kind: String,
    /// sha256 of `schema`
    pub sha256: String,
    #[serde(default)]
    pub description: String,
    /// Canonical JSON: an MCP tool's `inputSchema`, or an OpenAPI
    /// operation's method, path, parameters and body.
    pub schema: String,
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
    /// sha256 of the package's files as installed: checked when the package
    /// is imported and when `soma install` reuses the cache (a tampered
    /// `.soma_env` copy ran silently — the old hash was never compared)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
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
