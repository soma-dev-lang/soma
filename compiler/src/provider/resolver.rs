use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::manifest::{ProviderManifest, BackendDecl};
use super::types::{StorageRequest, Property, BackendConfig, StorageError};
use crate::runtime::storage::{StorageBackend, MemoryBackend, SqliteBackend, FileBackend};

/// Storage provider configuration from soma.toml [storage] section
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub provider: String,
    pub config: HashMap<String, String>,
    pub overrides: HashMap<String, String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            provider: "local".to_string(),
            config: HashMap::new(),
            overrides: HashMap::new(),
        }
    }
}

/// The provider resolver: takes storage requests and returns backend instances
pub struct ProviderResolver {
    manifest: ProviderManifest,
    config: StorageConfig,
}

impl ProviderResolver {
    /// Create a resolver for the default "local" provider
    pub fn local() -> Self {
        Self {
            manifest: local_manifest(),
            config: StorageConfig::default(),
        }
    }

    /// Create a resolver from a soma.toml storage config
    pub fn from_config(config: StorageConfig, project_dir: &Path) -> Result<Self, String> {
        let manifest = if config.provider == "local" {
            local_manifest()
        } else {
            // Look for provider in .soma_env/providers/{name}/
            let manifest_path = project_dir
                .join(".soma_env/providers")
                .join(&config.provider)
                .join("soma-provider.toml");

            if manifest_path.exists() {
                ProviderManifest::load(&manifest_path)?
            } else {
                return Err(format!(
                    "provider '{}' not found. Run: soma add-provider {}",
                    config.provider, config.provider
                ));
            }
        };

        Ok(Self { manifest, config })
    }

    /// Resolve a storage request to a backend instance
    pub fn resolve(&self, request: &StorageRequest) -> Result<Arc<dyn StorageBackend>, String> {
        // Check for field-specific override
        let provider = self.config.overrides.get(&request.field_name)
            .unwrap_or(&self.config.provider);

        let properties: Vec<String> = request.properties.iter()
            .map(|p| p.name().to_string())
            .collect();

        // Find matching backend in manifest
        let backend = self.manifest.resolve(&properties)
            .ok_or_else(|| {
                format!(
                    "provider '{}' cannot satisfy [{}] on field '{}.{}'",
                    provider,
                    properties.join(", "),
                    request.cell_name,
                    request.field_name
                )
            })?;

        // Instantiate the native backend
        let native = backend.native.as_deref().unwrap_or(&backend.name);

        // If native = "http", use the URL from config
        let native = if native == "http" {
            self.config.config.get("url").map(|s| s.as_str()).unwrap_or("http://localhost:9100")
        } else {
            native
        };

        Ok(instantiate(native, &request.cell_name, &request.field_name))
    }

    pub fn provider_name(&self) -> &str {
        &self.manifest.provider.name
    }
}

/// Built-in "local" provider manifest
fn local_manifest() -> ProviderManifest {
    ProviderManifest {
        provider: super::manifest::ProviderInfo {
            name: "local".to_string(),
            version: "0.17.0".to_string(),
            description: "Built-in SQLite + memory backends".to_string(),
            auth: None,
        },
        backend: vec![
            BackendDecl {
                name: "sqlite".to_string(),
                description: "SQLite — persistent, ACID, zero config".to_string(),
                requires: vec!["persistent".to_string()],
                optional: vec!["consistent".to_string(), "encrypted".to_string(), "retain".to_string(), "immutable".to_string()],
                impl_path: String::new(),
                native: Some("sqlite".to_string()),
            },
            BackendDecl {
                name: "memory".to_string(),
                description: "In-memory HashMap — fast, ephemeral".to_string(),
                requires: vec!["ephemeral".to_string()],
                optional: vec!["local".to_string(), "ttl".to_string()],
                impl_path: String::new(),
                native: Some("memory".to_string()),
            },
            BackendDecl {
                name: "memory-default".to_string(),
                description: "Default fallback".to_string(),
                requires: vec![],
                optional: vec![],
                impl_path: String::new(),
                native: Some("memory".to_string()),
            },
        ],
    }
}

/// Instantiate a native backend
fn instantiate(native_name: &str, cell_name: &str, field_name: &str) -> Arc<dyn StorageBackend> {
    // HTTP backend: native = "http://host:port"
    if native_name.starts_with("http://") || native_name.starts_with("https://") {
        return Arc::new(super::HttpBackend::new(native_name, cell_name, field_name));
    }

    match native_name {
        "sqlite" => Arc::new(SqliteBackend::new(cell_name, field_name)),
        "memory" => Arc::new(MemoryBackend::new()),
        "file" => Arc::new(FileBackend::new(cell_name, field_name)),
        _ => {
            eprintln!("warning: unknown native backend '{}', using memory", native_name);
            Arc::new(MemoryBackend::new())
        }
    }
}

/// Parse [storage] section from soma.toml content
pub fn parse_storage_config(toml_content: &str) -> StorageConfig {
    #[derive(serde::Deserialize)]
    struct TomlFile {
        #[serde(default)]
        storage: Option<StorageSection>,
    }
    #[derive(serde::Deserialize)]
    struct StorageSection {
        #[serde(default = "default_provider")]
        provider: String,
        #[serde(default)]
        config: HashMap<String, String>,
        #[serde(default)]
        overrides: HashMap<String, String>,
    }
    fn default_provider() -> String { "local".to_string() }

    if let Ok(parsed) = toml::from_str::<TomlFile>(toml_content) {
        if let Some(storage) = parsed.storage {
            return StorageConfig {
                provider: storage.provider,
                config: storage.config,
                overrides: storage.overrides,
            };
        }
    }
    StorageConfig::default()
}

/// Build StorageRequest from AST memory slot
pub fn build_request(cell_name: &str, slot: &crate::ast::MemorySlot) -> StorageRequest {
    let properties: Vec<Property> = slot.properties.iter().map(|p| {
        match &p.node {
            crate::ast::MemoryProperty::Flag(name) => Property::Flag(name.clone()),
            crate::ast::MemoryProperty::Param(param) => {
                let value = param.values.first()
                    .map(|v| format!("{}", v.node))
                    .unwrap_or_default();
                Property::Parameterized { name: param.name.clone(), value }
            }
        }
    }).collect();

    StorageRequest {
        cell_name: cell_name.to_string(),
        field_name: slot.name.clone(),
        field_type: format!("{:?}", slot.ty.node),
        properties,
    }
}

impl std::fmt::Display for crate::ast::Literal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            crate::ast::Literal::Int(n) => write!(f, "{}", n),
            crate::ast::Literal::Float(n) => write!(f, "{}", n),
            crate::ast::Literal::String(s) => write!(f, "{}", s),
            crate::ast::Literal::Bool(b) => write!(f, "{}", b),
            crate::ast::Literal::Duration(d) => write!(f, "{}{:?}", d.value, d.unit),
            crate::ast::Literal::Percentage(p) => write!(f, "{}%", p),
            crate::ast::Literal::Unit => write!(f, "()"),
        }
    }
}
