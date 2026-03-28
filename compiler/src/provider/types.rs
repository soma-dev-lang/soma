use std::collections::HashMap;

/// Error type for storage operations
#[derive(Debug)]
pub enum StorageError {
    ConnectionError(String),
    AuthError(String),
    LimitError(String),
    Other(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionError(s) => write!(f, "connection error: {}", s),
            Self::AuthError(s) => write!(f, "auth error: {}", s),
            Self::LimitError(s) => write!(f, "limit error: {}", s),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

/// A storage request emitted by the compiler for each memory field
#[derive(Debug, Clone)]
pub struct StorageRequest {
    pub cell_name: String,
    pub field_name: String,
    pub field_type: String,
    pub properties: Vec<Property>,
}

impl StorageRequest {
    pub fn has_property(&self, name: &str) -> bool {
        self.properties.iter().any(|p| p.name() == name)
    }

    pub fn get_param(&self, name: &str) -> Option<&str> {
        self.properties.iter().find_map(|p| {
            if let Property::Parameterized { name: n, value } = p {
                if n == name { Some(value.as_str()) } else { None }
            } else { None }
        })
    }
}

/// A property declared on a memory field
#[derive(Debug, Clone)]
pub enum Property {
    Flag(String),
    Parameterized { name: String, value: String },
}

impl Property {
    pub fn name(&self) -> &str {
        match self {
            Property::Flag(n) => n,
            Property::Parameterized { name, .. } => name,
        }
    }
}

/// Configuration passed to a backend constructor
#[derive(Debug, Clone)]
pub struct BackendConfig {
    pub provider_config: HashMap<String, String>,
    pub request: StorageRequest,
    pub credentials: HashMap<String, String>,
}
