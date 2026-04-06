use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// soma.toml — the project manifest
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
    /// Peer connections for the signal bus
    #[serde(default)]
    pub peers: HashMap<String, String>,
    /// Verification properties
    #[serde(default)]
    pub verify: VerifyConfig,
    /// Compute configuration
    #[serde(default)]
    pub compute: ComputeConfig,
    /// Cluster configuration for distributed execution
    #[serde(default)]
    pub cluster: ClusterConfig,
    /// Agent LLM configuration
    #[serde(default)]
    pub agent: AgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// LLM provider: "openai", "ollama", "anthropic", or custom URL
    #[serde(default)]
    pub provider: String,
    /// Model name (e.g., "gpt-4o-mini", "gemma3:12b", "claude-sonnet-4-20250514")
    #[serde(default)]
    pub model: String,
    /// Full API URL (overrides provider). For ollama: http://localhost:11434/v1/chat/completions
    #[serde(default)]
    pub url: String,
    /// API key — prefer env vars over putting this in soma.toml:
    ///   ANTHROPIC_API_KEY, OPENAI_API_KEY, or SOMA_LLM_KEY
    /// Only use this field for local testing. Never commit keys to git.
    #[serde(default)]
    pub key: String,
    /// Max retries on rate limit / transient errors
    #[serde(default = "default_retries")]
    pub retries: usize,
    /// Mock mode: "echo", "fixed:response text" (for testing)
    #[serde(default)]
    pub mock: String,
}

fn default_retries() -> usize { 3 }

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            url: String::new(),
            key: String::new(),
            retries: 3,
            mock: String::new(),
        }
    }
}

impl AgentConfig {
    /// Resolve the full API URL from provider shorthand or explicit url
    pub fn resolve_url(&self) -> String {
        if !self.url.is_empty() {
            return self.url.clone();
        }
        match self.provider.as_str() {
            "ollama" => "http://localhost:11434/v1/chat/completions".to_string(),
            "anthropic" => "https://api.anthropic.com/v1/messages".to_string(),
            _ => "https://api.openai.com/v1/chat/completions".to_string(),
        }
    }

    /// Resolve model name with sensible defaults per provider
    pub fn resolve_model(&self) -> String {
        if !self.model.is_empty() {
            return self.model.clone();
        }
        match self.provider.as_str() {
            "ollama" => "gemma3:12b".to_string(),
            "anthropic" => "claude-sonnet-4-20250514".to_string(),
            _ => "gpt-4o-mini".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComputeConfig {
    /// Backend: "threads", "spark", "ray", "gpu"
    #[serde(default = "default_compute_backend")]
    pub backend: String,
    /// Number of threads (for backend = "threads")
    #[serde(default)]
    pub threads: usize,
    /// Parallel handlers configuration
    #[serde(default)]
    pub parallel: ParallelConfig,
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            backend: "sequential".to_string(),
            threads: 0,
            parallel: ParallelConfig::default(),
        }
    }
}

fn default_compute_backend() -> String { "sequential".to_string() }

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Seed node addresses for cluster discovery
    #[serde(default)]
    pub seeds: Vec<String>,
    /// Node ID for this instance (auto-generated if not set)
    #[serde(default)]
    pub node_id: Option<String>,
}

impl ClusterConfig {
    pub fn is_enabled(&self) -> bool {
        !self.seeds.is_empty()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ParallelConfig {
    /// Handler names to parallelize
    #[serde(default)]
    pub handlers: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VerifyConfig {
    /// Check for deadlocks
    #[serde(default)]
    pub deadlock_free: bool,
    /// States that must eventually be reached (OR semantics)
    #[serde(default)]
    pub eventually: Vec<String>,
    /// States that must never be reached
    #[serde(default)]
    pub never: Vec<String>,
    /// States that must always be reachable
    #[serde(default)]
    pub always: Vec<String>,
    /// After reaching state X, must eventually reach one of Y
    #[serde(default)]
    pub after: HashMap<String, AfterConfig>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AfterConfig {
    #[serde(default)]
    pub eventually: Vec<String>,
    #[serde(default)]
    pub never: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_version() -> String { "0.1.0".to_string() }
fn default_entry() -> String { "main.cell".to_string() }

/// A dependency declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    /// Simple version: `auth = "0.1"`
    Version(String),
    /// Full spec: `auth = { git = "https://...", version = "0.1" }`
    Full(DependencySpec),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencySpec {
    #[serde(default)]
    pub git: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

impl Dependency {
    pub fn git_url(&self) -> Option<&str> {
        match self {
            Dependency::Version(_) => None,
            Dependency::Full(spec) => spec.git.as_deref(),
        }
    }

    pub fn local_path(&self) -> Option<&str> {
        match self {
            Dependency::Version(_) => None,
            Dependency::Full(spec) => spec.path.as_deref(),
        }
    }

    pub fn version_str(&self) -> &str {
        match self {
            Dependency::Version(v) => v,
            Dependency::Full(spec) => spec.version.as_deref().unwrap_or("*"),
        }
    }
}

impl Manifest {
    /// Load manifest from a soma.toml file
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        toml::from_str(&content)
            .map_err(|e| format!("invalid soma.toml: {}", e))
    }

    /// Save manifest to a file
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize manifest: {}", e))?;
        std::fs::write(path, content)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))
    }

    /// Create a new default manifest
    pub fn new(name: &str) -> Self {
        Self {
            package: PackageInfo {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                description: String::new(),
                author: String::new(),
                entry: "main.cell".to_string(),
            },
            dependencies: HashMap::new(),
            peers: HashMap::new(),
            verify: VerifyConfig::default(),
            compute: ComputeConfig::default(),
            cluster: ClusterConfig::default(),
            agent: AgentConfig::default(),
        }
    }
}
