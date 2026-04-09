use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A value stored in a memory slot
#[derive(Debug, Clone)]
pub enum StoredValue {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    List(Vec<StoredValue>),
    Map(HashMap<String, StoredValue>),
    Null,
}

impl std::fmt::Display for StoredValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoredValue::Int(n) => write!(f, "{}", n),
            StoredValue::Float(n) => write!(f, "{}", n),
            StoredValue::String(s) => write!(f, "{}", s),
            StoredValue::Bool(b) => write!(f, "{}", b),
            StoredValue::Null => write!(f, "null"),
            StoredValue::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            StoredValue::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// Storage backend trait — the interface that property declarations resolve to.
/// Memory, SQLite, etc. all implement this.
pub trait StorageBackend: Send + Sync {
    fn get(&self, key: &str) -> Option<StoredValue>;
    fn set(&self, key: &str, value: StoredValue);
    fn delete(&self, key: &str) -> bool;
    fn append(&self, value: StoredValue);
    fn list(&self) -> Vec<StoredValue>;
    fn keys(&self) -> Vec<String>;
    fn values(&self) -> Vec<StoredValue>;
    fn has(&self, key: &str) -> bool;
    fn len(&self) -> usize;
    fn backend_name(&self) -> &str;
}

/// In-memory storage — used for [ephemeral] or [local] properties
pub struct MemoryBackend {
    map: RwLock<HashMap<String, StoredValue>>,
    log: RwLock<Vec<StoredValue>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            log: RwLock::new(Vec::new()),
        }
    }
}

impl StorageBackend for MemoryBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        self.map.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    fn set(&self, key: &str, value: StoredValue) {
        self.map.write().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), value);
    }

    fn delete(&self, key: &str) -> bool {
        self.map.write().unwrap_or_else(|e| e.into_inner()).remove(key).is_some()
    }

    fn append(&self, value: StoredValue) {
        self.log.write().unwrap_or_else(|e| e.into_inner()).push(value);
    }

    fn list(&self) -> Vec<StoredValue> {
        let log = self.log.read().unwrap_or_else(|e| e.into_inner());
        if !log.is_empty() {
            return log.clone();
        }
        // Fall back to map values when log is empty (data was added via set())
        self.map.read().unwrap_or_else(|e| e.into_inner()).iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn keys(&self) -> Vec<String> {
        self.map.read().unwrap_or_else(|e| e.into_inner()).keys()
            .filter(|k| !k.starts_with("__"))
            .cloned().collect()
    }

    fn values(&self) -> Vec<StoredValue> {
        self.map.read().unwrap_or_else(|e| e.into_inner()).iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn has(&self, key: &str) -> bool {
        self.map.read().unwrap_or_else(|e| e.into_inner()).contains_key(key)
    }

    fn len(&self) -> usize {
        self.map.read().unwrap_or_else(|e| e.into_inner()).keys()
            .filter(|k| !k.starts_with("__")).count()
            + self.log.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn backend_name(&self) -> &str {
        "memory"
    }
}

/// Persistent storage — uses a simple JSON file for now.
/// In production this would be SQLite, Postgres, etc.
pub struct FileBackend {
    path: String,
    map: RwLock<HashMap<String, StoredValue>>,
    log: RwLock<Vec<StoredValue>>,
}

impl FileBackend {
    pub fn new(cell_name: &str, slot_name: &str) -> Self {
        let path = format!(".soma_data/{}_{}.json", cell_name, slot_name);

        // Load existing data if present, preserving types
        let (map, log) = if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) {
                let map = parsed.get("map")
                    .and_then(|m| m.as_object())
                    .map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), json_to_stored(v)))
                            .collect()
                    })
                    .unwrap_or_default();
                let log = parsed.get("log")
                    .and_then(|l| l.as_array())
                    .map(|arr| arr.iter().map(json_to_stored).collect())
                    .unwrap_or_default();
                (map, log)
            } else {
                (HashMap::new(), Vec::new())
            }
        } else {
            (HashMap::new(), Vec::new())
        };

        Self {
            path,
            map: RwLock::new(map),
            log: RwLock::new(log),
        }
    }

    fn persist(&self) {
        let _ = std::fs::create_dir_all(".soma_data");

        let map: HashMap<String, serde_json::Value> = self.map.read().unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), stored_to_json(v)))
            .collect();
        let log: Vec<serde_json::Value> = self.log.read().unwrap()
            .iter()
            .map(stored_to_json)
            .collect();

        let data = serde_json::json!({
            "map": map,
            "log": log,
        });

        // Atomic write: write to temp file then rename to avoid TOCTOU races
        let tmp_path = format!("{}.tmp", self.path);
        if std::fs::write(&tmp_path, serde_json::to_string_pretty(&data).unwrap()).is_ok() {
            let _ = std::fs::rename(&tmp_path, &self.path);
        }
    }
}

impl StorageBackend for FileBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        self.map.read().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, value: StoredValue) {
        self.map.write().unwrap().insert(key.to_string(), value);
        self.persist();
    }

    fn delete(&self, key: &str) -> bool {
        let removed = self.map.write().unwrap().remove(key).is_some();
        if removed { self.persist(); }
        removed
    }

    fn append(&self, value: StoredValue) {
        self.log.write().unwrap().push(value);
        self.persist();
    }

    fn list(&self) -> Vec<StoredValue> {
        let log = self.log.read().unwrap();
        if !log.is_empty() {
            return log.clone();
        }
        self.map.read().unwrap().iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn keys(&self) -> Vec<String> {
        self.map.read().unwrap().keys()
            .filter(|k| !k.starts_with("__"))
            .cloned().collect()
    }

    fn values(&self) -> Vec<StoredValue> {
        self.map.read().unwrap().iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn has(&self, key: &str) -> bool {
        self.map.read().unwrap().contains_key(key)
    }

    fn len(&self) -> usize {
        self.map.read().unwrap().keys()
            .filter(|k| !k.starts_with("__")).count()
            + self.log.read().unwrap().len()
    }

    fn backend_name(&self) -> &str {
        "file"
    }
}

/// SQLite storage — real ACID database. Used for [persistent, consistent].
/// Zero config: creates a .soma.db file automatically.
pub struct SqliteBackend {
    conn: std::sync::Mutex<rusqlite::Connection>,
    table: String,
}

impl SqliteBackend {
    pub fn new(cell_name: &str, slot_name: &str) -> Self {
        let _ = std::fs::create_dir_all(".soma_data");
        let db_path = ".soma_data/soma.db";
        let conn = rusqlite::Connection::open(db_path)
            .expect("failed to open SQLite database");

        let table = format!("{}_{}", cell_name, slot_name);

        // Create the KV table if it doesn't exist
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS \"{table}\" (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                type TEXT NOT NULL DEFAULT 'string'
            );
            CREATE TABLE IF NOT EXISTS \"{table}_log\" (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                value TEXT NOT NULL,
                type TEXT NOT NULL DEFAULT 'string'
            );"
        )).expect("failed to create tables");

        // Enable WAL mode for concurrent readers
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

        Self {
            conn: std::sync::Mutex::new(conn),
            table,
        }
    }

    fn store_typed(value: &StoredValue) -> (String, &'static str) {
        match value {
            StoredValue::Int(n) => (n.to_string(), "int"),
            StoredValue::Float(n) => (n.to_string(), "float"),
            StoredValue::Bool(b) => (b.to_string(), "bool"),
            StoredValue::String(s) => (s.clone(), "string"),
            StoredValue::Null => ("null".to_string(), "null"),
            StoredValue::List(items) => (serde_json::to_string(&items.iter().map(|v| stored_to_json(v)).collect::<Vec<_>>()).unwrap_or_default(), "json"),
            StoredValue::Map(map) => {
                let obj: serde_json::Map<String, serde_json::Value> = map.iter().map(|(k, v)| (k.clone(), stored_to_json(v))).collect();
                (serde_json::to_string(&obj).unwrap_or_default(), "json")
            }
        }
    }

    fn load_typed(value: &str, type_tag: &str) -> StoredValue {
        match type_tag {
            "int" => value.parse::<i64>().map(StoredValue::Int).unwrap_or(StoredValue::String(value.to_string())),
            "float" => value.parse::<f64>().map(StoredValue::Float).unwrap_or(StoredValue::String(value.to_string())),
            "bool" => StoredValue::Bool(value == "true"),
            "null" => StoredValue::Null,
            "json" => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(value) {
                    json_to_stored(&v)
                } else {
                    StoredValue::String(value.to_string())
                }
            }
            _ => StoredValue::String(value.to_string()),
        }
    }
}

impl StorageBackend for SqliteBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT value, type FROM \"{}\" WHERE key = ?1", self.table
        )).ok()?;
        stmt.query_row(rusqlite::params![key], |row| {
            let val: String = row.get(0)?;
            let typ: String = row.get(1)?;
            Ok(Self::load_typed(&val, &typ))
        }).ok()
    }

    fn set(&self, key: &str, value: StoredValue) {
        let conn = self.conn.lock().unwrap();
        let (val_str, type_tag) = Self::store_typed(&value);
        conn.execute(
            &format!("INSERT OR REPLACE INTO \"{}\" (key, value, type) VALUES (?1, ?2, ?3)", self.table),
            rusqlite::params![key, val_str, type_tag],
        ).ok();
    }

    fn delete(&self, key: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        let changes = conn.execute(
            &format!("DELETE FROM \"{}\" WHERE key = ?1", self.table),
            rusqlite::params![key],
        ).unwrap_or(0);
        changes > 0
    }

    fn append(&self, value: StoredValue) {
        let conn = self.conn.lock().unwrap();
        let (val_str, type_tag) = Self::store_typed(&value);
        conn.execute(
            &format!("INSERT INTO \"{}_log\" (value, type) VALUES (?1, ?2)", self.table),
            rusqlite::params![val_str, type_tag],
        ).ok();
    }

    fn list(&self) -> Vec<StoredValue> {
        let conn = self.conn.lock().unwrap();
        // First try the log table
        let mut stmt = conn.prepare(&format!(
            "SELECT value, type FROM \"{}_log\" ORDER BY id", self.table
        )).unwrap();
        let log_items: Vec<StoredValue> = stmt.query_map([], |row| {
            let val: String = row.get(0)?;
            let typ: String = row.get(1)?;
            Ok(Self::load_typed(&val, &typ))
        }).unwrap().filter_map(|r| r.ok()).collect();
        if !log_items.is_empty() {
            return log_items;
        }
        // Fall back to KV table values when log is empty (data was added via set())
        let mut stmt = conn.prepare(&format!(
            "SELECT value, type FROM \"{}\" WHERE key NOT LIKE '__%%' ORDER BY key", self.table
        )).unwrap();
        stmt.query_map([], |row| {
            let val: String = row.get(0)?;
            let typ: String = row.get(1)?;
            Ok(Self::load_typed(&val, &typ))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    fn keys(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT key FROM \"{}\" ORDER BY key", self.table
        )).unwrap();
        stmt.query_map([], |row| {
            let key: String = row.get(0)?;
            Ok(key)
        }).unwrap()
            .filter_map(|r| r.ok())
            .filter(|k| !k.starts_with("__"))
            .collect()
    }

    fn values(&self) -> Vec<StoredValue> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT value, type FROM \"{}\" WHERE key NOT LIKE '\\_\\_%' ESCAPE '\\' ORDER BY key", self.table
        )).unwrap();
        stmt.query_map([], |row| {
            let val: String = row.get(0)?;
            let typ: String = row.get(1)?;
            Ok(Self::load_typed(&val, &typ))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    fn has(&self, key: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT 1 FROM \"{}\" WHERE key = ?1", self.table
        )).unwrap();
        stmt.exists(rusqlite::params![key]).unwrap_or(false)
    }

    fn len(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM \"{}\" WHERE key NOT LIKE '\\_\\_%' ESCAPE '\\'", self.table),
            [],
            |row| row.get(0),
        ).unwrap_or(0);
        count as usize
    }

    fn backend_name(&self) -> &str {
        "sqlite"
    }
}

/// Resolve memory properties to a storage backend.
/// Uses the registry to find the best matching backend definition,
/// then instantiates the corresponding native implementation.
pub fn resolve_backend_from_registry(
    cell_name: &str,
    slot_name: &str,
    properties: &[String],
    registry: &crate::registry::Registry,
) -> Arc<dyn StorageBackend> {
    // Ask the registry which backend matches these properties
    if let Some(backend_def) = registry.resolve_backend(properties) {
        let native = backend_def.native_impl.as_deref().unwrap_or("memory");
        return instantiate_native_backend(native, cell_name, slot_name);
    }
    // Fallback: use old hardcoded logic
    resolve_backend(cell_name, slot_name, properties)
}

/// Fallback resolver (used when no registry is available).
pub fn resolve_backend(
    cell_name: &str,
    slot_name: &str,
    properties: &[String],
) -> Arc<dyn StorageBackend> {
    let is_persistent = properties.iter().any(|p| p == "persistent");
    let is_ephemeral = properties.iter().any(|p| p == "ephemeral");

    if is_persistent && !is_ephemeral {
        Arc::new(FileBackend::new(cell_name, slot_name))
    } else {
        Arc::new(MemoryBackend::new())
    }
}

/// The native boundary: maps a backend name (from `native "name"` in a cell)
/// to an actual Rust implementation. This is the ONLY place where native code
/// is hardcoded. Everything above this is Soma.
fn instantiate_native_backend(
    native_name: &str,
    cell_name: &str,
    slot_name: &str,
) -> Arc<dyn StorageBackend> {
    match native_name {
        "memory" => Arc::new(MemoryBackend::new()),
        "file" => Arc::new(FileBackend::new(cell_name, slot_name)),
        "sqlite" => Arc::new(SqliteBackend::new(cell_name, slot_name)),
        unknown => {
            eprintln!("warning: unknown native backend '{}', falling back to memory", unknown);
            Arc::new(MemoryBackend::new())
        }
    }
}

/// Convert a serde_json::Value to StoredValue (preserving types)
fn json_to_stored(v: &serde_json::Value) -> StoredValue {
    match v {
        serde_json::Value::Null => StoredValue::Null,
        serde_json::Value::Bool(b) => StoredValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                StoredValue::Int(i)
            } else {
                StoredValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => StoredValue::String(s.clone()),
        serde_json::Value::Array(arr) => {
            StoredValue::List(arr.iter().map(json_to_stored).collect())
        }
        serde_json::Value::Object(obj) => {
            StoredValue::Map(obj.iter().map(|(k, v)| (k.clone(), json_to_stored(v))).collect())
        }
    }
}

/// Convert a StoredValue to serde_json::Value
fn stored_to_json(v: &StoredValue) -> serde_json::Value {
    match v {
        StoredValue::Int(n) => serde_json::Value::Number((*n).into()),
        StoredValue::Float(n) => serde_json::json!(*n),
        StoredValue::String(s) => serde_json::Value::String(s.clone()),
        StoredValue::Bool(b) => serde_json::Value::Bool(*b),
        StoredValue::Null => serde_json::Value::Null,
        StoredValue::List(items) => {
            serde_json::Value::Array(items.iter().map(stored_to_json).collect())
        }
        StoredValue::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map.iter()
                .map(|(k, v)| (k.clone(), stored_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
    }
}
