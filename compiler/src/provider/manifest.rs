use serde::Deserialize;
use std::path::Path;

/// A provider manifest (soma-provider.toml)
#[derive(Debug, Deserialize)]
pub struct ProviderManifest {
    pub provider: ProviderInfo,
    #[serde(default)]
    pub backend: Vec<BackendDecl>,
}

#[derive(Debug, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub config: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendDecl {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Properties this backend requires (must all be present)
    #[serde(default)]
    pub requires: Vec<String>,
    /// Additional properties this backend can handle
    #[serde(default)]
    pub optional: Vec<String>,
    /// Rust impl path (for dynamic loading)
    #[serde(default, rename = "impl")]
    pub impl_path: String,
    /// Native backend name (for built-in resolution)
    #[serde(default)]
    pub native: Option<String>,
}

impl ProviderManifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("invalid provider manifest: {}", e))
    }

    /// Resolve a set of properties to the best matching backend
    pub fn resolve(&self, properties: &[String]) -> Option<&BackendDecl> {
        let mut best: Option<(&BackendDecl, usize)> = None;

        for backend in &self.backend {
            // Check: all required properties must be present in the request
            let all_required = backend.requires.iter()
                .all(|req| properties.iter().any(|p| p == req));

            if !all_required {
                continue;
            }

            // Check: all request properties must be in requires OR optional
            let all_covered = properties.iter().all(|p| {
                backend.requires.contains(p) || backend.optional.contains(p)
                    || p.contains('(') // parameterized props like ttl(30min) — check base name
                    || backend.optional.iter().any(|opt| p.starts_with(opt))
            });

            if !all_covered {
                continue;
            }

            // Score: number of matched required properties (most specific wins)
            let score = backend.requires.len();
            if best.is_none() || score > best.unwrap().1 {
                best = Some((backend, score));
            }
        }

        best.map(|(b, _)| b)
    }
}
