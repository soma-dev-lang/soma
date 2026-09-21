use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Where `.soma_data/` lives: beside the program (set once by run / serve
/// from the program's path), else the working directory. It used to be the
/// working directory always, so `soma run dir/app.cell` from elsewhere read
/// a different database than `cd dir && soma run app.cell`.
static DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_data_dir_beside(program: &std::path::Path) {
    let dir = program.parent().filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(".soma_data")).unwrap_or_else(|| std::path::PathBuf::from(".soma_data"));
    let _ = DATA_DIR.set(dir);
}

pub fn data_dir() -> std::path::PathBuf {
    DATA_DIR.get().cloned().unwrap_or_else(|| std::path::PathBuf::from(".soma_data"))
}

/// A value stored in a memory slot
#[derive(Debug, Clone)]
pub enum StoredValue {
    Int(i64),
    /// An Int beyond i64, kept as its decimal digits so a slot gives back the
    /// Int it accepted (it used to come back as a String).
    BigInt(String),
    Float(f64),
    String(String),
    Bool(bool),
    List(Vec<StoredValue>),
    /// insertion-ordered: a record comes back with its fields in the order
    /// they were written (a HashMap shuffled them per process)
    Map(indexmap::IndexMap<String, StoredValue>),
    /// V1.6: sum-type values stored with their tag and fields,
    /// so a `Map<String, TodoStatus>` round-trips through any backend.
    Variant {
        type_name: String,
        variant: String,
        fields: StoredVariantFields,
    },
    Null,
}

#[derive(Debug, Clone)]
pub enum StoredVariantFields {
    Unit,
    Tuple(Vec<StoredValue>),
    Struct(Vec<(String, StoredValue)>),
}

impl std::fmt::Display for StoredValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoredValue::BigInt(d) => write!(f, "{}", d),
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
            StoredValue::Variant { variant, fields, .. } => match fields {
                StoredVariantFields::Unit => write!(f, "{}", variant),
                StoredVariantFields::Tuple(items) => {
                    write!(f, "{}(", variant)?;
                    for (i, v) in items.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", v)?;
                    }
                    write!(f, ")")
                }
                StoredVariantFields::Struct(entries) => {
                    write!(f, "{} {{", variant)?;
                    for (i, (k, v)) in entries.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, " {}: {}", k, v)?;
                    }
                    write!(f, " }}")
                }
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
    /// Remove the most recently appended entry — the undo of `append`,
    /// used to roll a failed handler back.
    fn unappend(&self);
    /// Replace the whole append log (`rows[i] = v`, `rows.delete(i)` on a
    /// List slot). Default: pop everything, append the new items.
    fn replace_list(&self, items: Vec<StoredValue>) {
        let mut remaining = self.list().len();
        if has_storage_error() { return; }
        while remaining > 0 {
            self.unappend();
            if has_storage_error() { return; }
            let next = self.list().len();
            if has_storage_error() { return; }
            if next >= remaining {
                note_write_error("replace list", &"the backend did not remove an item");
                return;
            }
            remaining = next;
        }
        for item in items {
            self.append(item);
            if has_storage_error() { return; }
        }
    }
    /// `rows[i] = v` on a List slot: replace ONE element. Default: rewrite
    /// the whole log. `false` when i is past the end.
    fn list_set(&self, i: usize, value: StoredValue) -> bool {
        let mut items = self.list();
        if has_storage_error() || i >= items.len() { return false; }
        items[i] = value;
        self.replace_list(items);
        true
    }
    /// `rows.delete(i)` on a List slot: remove ONE element and hand back a
    /// token that `list_restore` uses to put it back at its place (a
    /// failing `try` or handler). Default: rewrite the log; the token is
    /// the index.
    fn list_remove(&self, i: usize) -> Option<i64> {
        let mut items = self.list();
        if has_storage_error() || i >= items.len() { return None; }
        items.remove(i);
        self.replace_list(items);
        if has_storage_error() { None } else { Some(i as i64) }
    }
    fn list_restore(&self, token: i64, value: StoredValue) {
        let mut items = self.list();
        if has_storage_error() { return; }
        let at = usize::try_from(token).unwrap_or(0).min(items.len());
        items.insert(at, value);
        self.replace_list(items);
    }
    fn list(&self) -> Vec<StoredValue>;
    /// Length / one element of the list, without materializing it
    /// (`rows.get(i)` in a loop loaded the whole log per call).
    fn list_len(&self) -> usize { self.list().len() }
    fn list_get(&self, i: usize) -> Option<StoredValue> { self.list().into_iter().nth(i) }
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
    fn list_len(&self) -> usize {
        let log = self.log.read().unwrap_or_else(|e| e.into_inner());
        if !log.is_empty() { return log.len(); }
        drop(log);
        self.list().len()
    }
    fn list_get(&self, i: usize) -> Option<StoredValue> {
        let log = self.log.read().unwrap_or_else(|e| e.into_inner());
        if !log.is_empty() { return log.get(i).cloned(); }
        drop(log);
        self.list().into_iter().nth(i)
    }
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
        let mut log = self.log.write().unwrap_or_else(|e| e.into_inner());
        let mut map = self.map.write().unwrap_or_else(|e| e.into_inner());
        if log.is_empty() {
            let mut keys: Vec<_> = map.keys().filter(|k| !k.starts_with("__")).cloned().collect();
            keys.sort();
            for key in keys { log.push(map.remove(&key).unwrap()); }
        } else { map.retain(|k, _| k.starts_with("__")); }
        log.push(value);
    }

    fn replace_list(&self, items: Vec<StoredValue>) {
        let mut log = self.log.write().unwrap_or_else(|e| e.into_inner());
        self.map.write().unwrap_or_else(|e| e.into_inner()).retain(|k, _| k.starts_with("__"));
        *log = items;
    }

    fn unappend(&self) {
        self.log.write().unwrap_or_else(|e| e.into_inner()).pop();
    }

    fn list(&self) -> Vec<StoredValue> {
        let log = self.log.read().unwrap_or_else(|e| e.into_inner());
        if !log.is_empty() {
            return log.clone();
        }
        // Fall back to map values when log is empty (data was added via set())
        // — sorted by key, like the SQLite backend (a HashMap walk gave a
        // different order on every run)
        let m = self.map.read().unwrap_or_else(|e| e.into_inner());
        let mut ks: Vec<&String> = m.keys().filter(|k| !k.starts_with("__")).collect();
        ks.sort();
        ks.into_iter().map(|k| m[k].clone())
            .collect()
    }

    fn keys(&self) -> Vec<String> {
        let mut ks: Vec<String> = self.map.read().unwrap_or_else(|e| e.into_inner()).keys()
            .filter(|k| !k.starts_with("__"))
            .cloned().collect();
        ks.sort();
        ks
    }

    fn values(&self) -> Vec<StoredValue> {
        let m = self.map.read().unwrap_or_else(|e| e.into_inner());
        let mut ks: Vec<&String> = m.keys().filter(|k| !k.starts_with("__")).collect();
        ks.sort();
        ks.into_iter().map(|k| m[k].clone()).collect()
    }

    fn has(&self, key: &str) -> bool {
        self.map.read().unwrap_or_else(|e| e.into_inner()).contains_key(key)
    }

    fn len(&self) -> usize {
        let log = self.log.read().unwrap_or_else(|e| e.into_inner());
        let map = self.map.read().unwrap_or_else(|e| e.into_inner());
        map.keys().filter(|k| !k.starts_with("__")).count() + log.len()
    }

    fn backend_name(&self) -> &str {
        "memory"
    }
}

pub use super::file_storage::FileBackend;

/// SQLite storage — real ACID database. Used for [persistent, consistent].
/// Zero config: creates a .soma.db file automatically.
pub struct SqliteBackend {
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
    table: String,
    list_layout: std::sync::Mutex<Option<ListLayout>>,
}

struct ListLayout {
    version: i64,
    changes: i64,
    epoch: u64,
    first: Option<i64>,
    contiguous: bool,
}

// total_changes includes rolled-back writes. A transaction boundary must
// also invalidate a layout observed before those writes were rolled back.
static LIST_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TRANSACTION_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn transaction_started() {
    TRANSACTION_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub fn transaction_ended() {
    TRANSACTION_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
    LIST_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// ONE connection per process for `soma.db`, shared by every slot: a
/// handler's writes then sit in one SQLite transaction (`BEGIN IMMEDIATE` …
/// `COMMIT` around the handler, see `Interpreter::atomically`), so a process
/// killed mid-handler leaves nothing behind — with a connection per slot the
/// writes were committed one statement at a time.
static SHARED_CONN: std::sync::OnceLock<Arc<std::sync::Mutex<rusqlite::Connection>>> = std::sync::OnceLock::new();

/// A database that cannot be opened or is damaged: a clean diagnostic and
/// exit 1 (it was a Rust panic naming storage.rs).
fn db_fatal(path: &std::path::Path, why: &str) -> ! {
    eprintln!("error: the data file {} cannot be used: {} — restore it from a backup, or move it aside (`mv {} {}.bad`) to start with empty storage", path.display(), why, path.display(), path.display());
    std::process::exit(1)
}

/// `PRAGMA quick_check` on the shared database (serve runs it at start-up):
/// a page damaged mid-file was served as truth, rows silently missing.
pub fn integrity_check() -> Result<(), String> {
    let Some(conn) = shared_connection() else { return Ok(()) };
    let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
    let r: Result<String, _> = conn.query_row("PRAGMA quick_check", [], |row| row.get(0));
    match r {
        Ok(s) if s == "ok" => Ok(()),
        Ok(s) => Err(s),
        Err(e) => Err(e.to_string()),
    }
}

/// The shared connection, if any persistent slot opened the database.
pub fn shared_connection() -> Option<Arc<std::sync::Mutex<rusqlite::Connection>>> {
    SHARED_CONN.get().cloned()
}

impl SqliteBackend {
    fn read<T>(&self, what: &str, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> Option<T> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        match f(&conn) {
            Ok(value) => Some(value),
            Err(e) => { note_read_error(&format!("{} {what}", self.table), &e); None }
        }
    }
    fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredValue> {
        let value: String = row.get(0)?;
        let tag: String = row.get(1)?;
        Ok(Self::load_typed(&value, &tag))
    }

    // SQLite may refuse a lock upgrade without invoking busy_timeout,
    // notably while fresh processes switch the same database to WAL.
    // These initialization statements are idempotent and may be retried.
    fn initialize(conn: &rusqlite::Connection, sql: &str) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            match conn.execute_batch(sql) {
                Ok(()) => return Ok(()),
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if matches!(e.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
                        && std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    // A statement may end the whole transaction (SQLITE_FULL, I/O errors,
    // RAISE(ROLLBACK)). Refuse every subsequent write until its caller has
    // unwound the unit; otherwise it would silently run in autocommit mode.
    fn may_write(conn: &rusqlite::Connection) -> bool {
        if has_storage_error() { return false; }
        if TRANSACTION_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) && conn.is_autocommit() {
            note_write_error("write", &"the database already rolled back the transaction");
            false
        } else { true }
    }

    /// Keep one backend operation atomic even when SQLite's FAIL conflict
    /// policy leaves earlier changes from the statement in place. The
    /// enclosing handler still owns the final commit.
    fn write<T>(&self, what: &str, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> Option<T> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if !Self::may_write(&conn) { return None; }
        let result = conn.execute_batch("SAVEPOINT soma_storage_write")
            .and_then(|_| f(&conn))
            .and_then(|value| conn.execute_batch("RELEASE soma_storage_write").map(|_| value));
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                note_write_error(what, &e);
                if !conn.is_autocommit() {
                    if conn.execute_batch("ROLLBACK TO soma_storage_write; RELEASE soma_storage_write").is_err() {
                        // If the local rollback fails, continuing the handler
                        // is unsafe. Its boundary check detects the full abort.
                        let _ = conn.execute_batch("ROLLBACK");
                    }
                }
                LIST_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                None
            }
        }
    }

    pub fn new(cell_name: &str, slot_name: &str) -> Self {
        Self::try_new(cell_name, slot_name)
            .unwrap_or_else(|e| db_fatal(&data_dir().join("soma.db"), &e))
    }

    pub fn try_new(cell_name: &str, slot_name: &str) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
        let db_path = data_dir().join("soma.db");
        if SHARED_CONN.get().is_none() {
            let c = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
            c.busy_timeout(std::time::Duration::from_secs(120)).map_err(|e| e.to_string())?;
            Self::initialize(&c, "PRAGMA journal_mode=WAL;")?;
            let _ = SHARED_CONN.set(Arc::new(std::sync::Mutex::new(c)));
        }
        let shared = SHARED_CONN.get().unwrap().clone();
        let conn = shared.lock().unwrap_or_else(|e| e.into_inner());

        let table = format!("{}_{}", cell_name, slot_name);

        // Create the KV table if it doesn't exist
        Self::initialize(&conn, &format!(
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
        ))?;

        drop(conn);
        Ok(Self {
            conn: shared,
            table,
            list_layout: std::sync::Mutex::new(None),
        })
    }

    fn store_typed(value: &StoredValue) -> (String, &'static str) {
        match value {
            StoredValue::Int(n) => (n.to_string(), "int"),
            StoredValue::BigInt(d) => (d.clone(), "bigint"),
            StoredValue::Float(n) => (n.to_string(), "float"),
            StoredValue::Bool(b) => (b.to_string(), "bool"),
            StoredValue::String(s) => (s.clone(), "string"),
            StoredValue::Null => ("null".to_string(), "null"),
            StoredValue::List(items) => (serde_json::to_string(&items.iter().map(|v| stored_to_json(v)).collect::<Vec<_>>()).unwrap_or_default(), "json"),
            StoredValue::Map(_) => (serde_json::to_string(&stored_to_json(value)).unwrap_or_default(), "json"),
            StoredValue::Variant { .. } => {
                (serde_json::to_string(&stored_to_json(value)).unwrap_or_default(), "variant")
            }
        }
    }

    fn load_typed(value: &str, type_tag: &str) -> StoredValue {
        match type_tag {
            "int" => value.parse::<i64>().map(StoredValue::Int).unwrap_or(StoredValue::String(value.to_string())),
            "bigint" => StoredValue::BigInt(value.to_string()),
            "float" => value.parse::<f64>().map(StoredValue::Float).unwrap_or(StoredValue::String(value.to_string())),
            "bool" => StoredValue::Bool(value == "true"),
            "null" => StoredValue::Null,
            "json" | "variant" => {
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

/// Keep absence distinct from a backend failure until the interpreter can
/// raise it. A read failure cannot supply a trustworthy undo value.
#[derive(Debug)]
pub struct StorageFailure {
    pub read: bool,
    pub detail: String,
}
impl std::fmt::Display for StorageFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.detail.fmt(f) }
}
static WRITE_ERROR: std::sync::Mutex<Option<StorageFailure>> = std::sync::Mutex::new(None);
static HAS_ERROR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn note_error(read: bool, what: &str, e: &dyn std::fmt::Display) {
    let mut g = WRITE_ERROR.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() { *g = Some(StorageFailure { read, detail: format!("{what}: {e}") }); HAS_ERROR.store(true, std::sync::atomic::Ordering::Release); }
}
pub fn note_write_error(what: &str, e: &dyn std::fmt::Display) { note_error(false, what, e); }
pub fn note_read_error(what: &str, e: &dyn std::fmt::Display) { note_error(true, what, e); }
pub fn has_storage_error() -> bool { HAS_ERROR.load(std::sync::atomic::Ordering::Acquire) }
pub fn take_write_error() -> Option<StorageFailure> {
    let mut error = WRITE_ERROR.lock().unwrap_or_else(|e| e.into_inner());
    let value = error.take();
    HAS_ERROR.store(false, std::sync::atomic::Ordering::Release);
    value
}

impl StorageBackend for SqliteBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        self.read("get", |conn| {
            let mut stmt = conn.prepare(&format!("SELECT value, type FROM \"{}\" WHERE key = ?1", self.table))?;
            use rusqlite::OptionalExtension;
            stmt.query_row(rusqlite::params![key], Self::read_row).optional()
        }).flatten()
    }

    fn set(&self, key: &str, value: StoredValue) {
        let (val_str, type_tag) = Self::store_typed(&value);
        self.write("write", |conn| conn.execute(
            &format!("INSERT OR REPLACE INTO \"{}\" (key, value, type) VALUES (?1, ?2, ?3)", self.table),
            rusqlite::params![key, val_str, type_tag],
        ));
    }

    fn delete(&self, key: &str) -> bool {
        self.write("delete", |conn| conn.execute(
            &format!("DELETE FROM \"{}\" WHERE key = ?1", self.table), rusqlite::params![key],
        )).is_some_and(|n| n > 0)
    }

    fn append(&self, value: StoredValue) {
        let (val_str, type_tag) = Self::store_typed(&value);
        self.write("append", |conn| {
            let empty: bool = conn.query_row(&format!("SELECT NOT EXISTS (SELECT 1 FROM \"{}_log\")", self.table), [], |r| r.get(0))?;
            if empty {
                conn.execute(&format!("INSERT INTO \"{0}_log\" (value, type) SELECT value, type FROM \"{0}\" WHERE substr(key,1,2) != '__' ORDER BY key", self.table), [])?;
            }
            conn.execute(&format!("DELETE FROM \"{}\" WHERE substr(key,1,2) != '__'", self.table), [])?;
            conn.execute(&format!("INSERT INTO \"{}_log\" (value, type) VALUES (?1, ?2)", self.table), rusqlite::params![val_str, type_tag])
        });
    }

    fn unappend(&self) {
        self.write("remove last list item", |conn| conn.execute(
            &format!("DELETE FROM \"{0}_log\" WHERE id = (SELECT MAX(id) FROM \"{0}_log\")", self.table), [],
        ));
    }

    fn list_len(&self) -> usize {
        let count: Option<i64> = self.read("list length", |conn| conn.query_row(
            &format!("SELECT COUNT(*) FROM \"{}_log\"", self.table), [], |r| r.get(0)));
        match count { Some(0) => self.list().len(), Some(n) => n as usize, None => 0 }
    }

    fn list_get(&self, i: usize) -> Option<StoredValue> {
        // Use the primary key only after verifying the entire log has no
        // gaps. A missing earlier row shifts every later list position,
        // even when MIN(id) + i happens to exist.
        let conn = self.conn.lock().unwrap();
        let version = conn.query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0)).map_err(|e| note_read_error("list index", &e)).ok()?;
        let changes = conn.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0)).map_err(|e| note_read_error("list index", &e)).ok()?;
        let epoch = LIST_EPOCH.load(std::sync::atomic::Ordering::Relaxed);
        let mut layout = self.list_layout.lock().unwrap();
        if !layout.as_ref().is_some_and(|l| (l.version, l.changes, l.epoch) == (version, changes, epoch)) {
            let (first, last, count): (Option<i64>, Option<i64>, i64) = conn.query_row(
                &format!("SELECT MIN(id), MAX(id), COUNT(*) FROM \"{}_log\"", self.table), [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ).map_err(|e| note_read_error("list index", &e)).ok()?;
            let contiguous = first.zip(last).is_some_and(|(a,b)| i128::from(b) - i128::from(a) + 1 == i128::from(count));
            *layout = Some(ListLayout { version, changes, epoch, first, contiguous });
        }
        let l = layout.as_ref().unwrap();
        let Some(min) = l.first else {
            drop(layout);
            drop(conn);
            // no log rows: a list kept under the key/value table
            return self.list().into_iter().nth(i);
        };
        let index = i64::try_from(i).map_err(|e| note_read_error("list index", &e)).ok()?;
        let (query, position) = if l.contiguous {
            (format!("SELECT value, type FROM \"{}_log\" WHERE id = ?1", self.table), min.checked_add(index)?)
        } else {
            (format!("SELECT value, type FROM \"{}_log\" ORDER BY id LIMIT 1 OFFSET ?1", self.table), index)
        };
        use rusqlite::OptionalExtension;
        conn.query_row(&query, rusqlite::params![position], Self::read_row)
            .optional().map_err(|e| note_read_error("list index", &e)).ok().flatten()
    }

    fn list_set(&self, i: usize, value: StoredValue) -> bool {
        // one UPDATE by rowid (the whole log was deleted and re-inserted:
        // `rows[i] = v` on 20 000 rows took 23 ms each). The i-th row in id
        // order is right with or without gaps; a list still kept under the
        // key/value table (no log rows) takes the rewriting path.
        use rusqlite::OptionalExtension;
        let Ok(index) = i64::try_from(i) else { return false };
        let (text, tag) = Self::store_typed(&value);
        let table = self.table.clone();
        let done = self.write("list set", |conn| {
            let id: Option<i64> = conn.query_row(
                &format!("SELECT id FROM \"{}_log\" ORDER BY id LIMIT 1 OFFSET ?1", table),
                rusqlite::params![index], |r| r.get(0)).optional()?;
            match id {
                Some(id) => {
                    conn.execute(&format!("UPDATE \"{}_log\" SET value = ?1, type = ?2 WHERE id = ?3", table),
                        rusqlite::params![text, tag, id])?;
                    Ok(true)
                }
                None => Ok(false),
            }
        });
        match done {
            Some(true) => true,
            Some(false) => {
                let mut items = self.list();
                if has_storage_error() || i >= items.len() { return false; }
                items[i] = value;
                self.replace_list(items);
                !has_storage_error()
            }
            None => false,
        }
    }

    fn list_remove(&self, i: usize) -> Option<i64> {
        // one DELETE by rowid; the row's id is the token that puts it back
        // (a list kept under the key/value table, no log rows, is rewritten)
        use rusqlite::OptionalExtension;
        let Ok(index) = i64::try_from(i) else { return None };
        let table = self.table.clone();
        let removed = self.write("list remove", |conn| {
            let id: Option<i64> = conn.query_row(
                &format!("SELECT id FROM \"{}_log\" ORDER BY id LIMIT 1 OFFSET ?1", table),
                rusqlite::params![index], |r| r.get(0)).optional()?;
            if let Some(id) = id {
                conn.execute(&format!("DELETE FROM \"{}_log\" WHERE id = ?1", table), rusqlite::params![id])?;
            }
            Ok(id)
        })?;
        match removed {
            Some(id) => Some(id),
            None => {
                let mut items = self.list();
                if has_storage_error() || i >= items.len() { return None; }
                items.remove(i);
                self.replace_list(items);
                if has_storage_error() { None } else { Some(-1 - index) }
            }
        }
    }

    fn list_restore(&self, token: i64, value: StoredValue) {
        if token < 0 {
            // the rewriting path's token: an index
            let mut items = self.list();
            if has_storage_error() { return; }
            let at = usize::try_from(-1 - token).unwrap_or(0).min(items.len());
            items.insert(at, value);
            self.replace_list(items);
            return;
        }
        let (text, tag) = Self::store_typed(&value);
        let table = self.table.clone();
        self.write("list restore", |conn| {
            conn.execute(&format!("INSERT INTO \"{}_log\" (id, value, type) VALUES (?1, ?2, ?3)", table),
                rusqlite::params![token, text, tag])?;
            Ok(())
        });
    }

    fn replace_list(&self, items: Vec<StoredValue>) {
        self.write("replace list", |conn| {
            conn.execute(&format!("DELETE FROM \"{}_log\"", self.table), [])?;
            conn.execute(&format!("DELETE FROM \"{}\" WHERE substr(key,1,2) != '__'", self.table), [])?;
            for item in items {
                let (value, tag) = Self::store_typed(&item);
                conn.execute(
                    &format!("INSERT INTO \"{}_log\" (value, type) VALUES (?1, ?2)", self.table),
                    rusqlite::params![value, tag],
                )?;
            }
            Ok(())
        });
    }

    fn list(&self) -> Vec<StoredValue> {
        self.read("list", |conn| {
            let mut stmt = conn.prepare(&format!("SELECT value, type FROM \"{}_log\" ORDER BY id", self.table))?;
            let rows = stmt.query_map([], Self::read_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            if !rows.is_empty() { return Ok(rows); }
            let mut stmt = conn.prepare(&format!("SELECT value, type FROM \"{}\" WHERE substr(key,1,2) != '__' ORDER BY key", self.table))?;
            let rows = stmt.query_map([], Self::read_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).unwrap_or_default()
    }

    fn keys(&self) -> Vec<String> {
        self.read("keys", |conn| {
            let mut stmt = conn.prepare(&format!("SELECT key FROM \"{}\" ORDER BY key", self.table))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().filter(|k| !k.starts_with("__")).collect())
        }).unwrap_or_default()
    }

    fn values(&self) -> Vec<StoredValue> {
        self.read("values", |conn| {
            let mut stmt = conn.prepare(&format!("SELECT value, type FROM \"{}\" WHERE substr(key,1,2) != '__' ORDER BY key", self.table))?;
            let rows = stmt.query_map([], Self::read_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }).unwrap_or_default()
    }

    fn has(&self, key: &str) -> bool {
        self.read("has", |conn| {
            let mut stmt = conn.prepare(&format!("SELECT 1 FROM \"{}\" WHERE key = ?1", self.table))?;
            stmt.exists(rusqlite::params![key])
        }).unwrap_or(false)
    }

    fn len(&self) -> usize {
        // a plain COUNT(*) walks the index; the `__` metadata keys are
        // counted by an index RANGE and subtracted (`substr(key,1,2) != '__'`
        // evaluated every row: `m.len` cost 0.5 ms on 20 000 entries)
        self.read("length", |conn| conn.query_row(
            &format!("SELECT (SELECT COUNT(*) FROM \"{t}\") - (SELECT COUNT(*) FROM \"{t}\" WHERE key >= '__' AND key < '_`')", t = self.table), [],
            |r| r.get::<_, i64>(0),
        )).unwrap_or(0).max(0) as usize
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

/// A user key serde_json itself gives a meaning to: with `arbitrary_precision`
/// an object `{"$serde_json::private::Number": "42"}` deserializes as the
/// NUMBER 42 (and as a parse error for "abc"), so a stored Map with that key
/// read back as an Int or as a String. The key is spelled `__key__$serde…`
/// on disk (which also triggers the `__map__` wrap) and restored on read; a
/// user key that already starts with `__key__` is escaped the same way so
/// the unescape stays unambiguous.
fn escape_map_key(k: &str) -> String {
    if k.starts_with("$serde_json::private::") || k.starts_with("__key__") { format!("__key__{}", k) } else { k.to_string() }
}

fn unescape_map_key(k: &str) -> String {
    k.strip_prefix("__key__").unwrap_or(k).to_string()
}

/// Convert a serde_json::Value to StoredValue (preserving types).
/// A JSON object with `__variant__` key is decoded back to a Variant.
pub(crate) fn json_to_stored(v: &serde_json::Value) -> StoredValue {
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
            if let (1, Some(serde_json::Value::Object(inner))) = (obj.len(), obj.get("__map__")) {
                return StoredValue::Map(inner.iter().map(|(k, v)| (unescape_map_key(k), json_to_stored(v))).collect());
            }
            // NaN / ±inf nested in a Map or List (JSON has no such number:
            // they were written as null and read back as `()`)
            if let (1, Some(serde_json::Value::String(f))) = (obj.len(), obj.get("__float__")) {
                match f.as_str() {
                    "NaN" => return StoredValue::Float(f64::NAN),
                    "inf" => return StoredValue::Float(f64::INFINITY),
                    "-inf" => return StoredValue::Float(f64::NEG_INFINITY),
                    _ => {}
                }
            }
            if let (1, Some(serde_json::Value::String(d))) = (obj.len(), obj.get("__bigint__")) {
                return StoredValue::BigInt(d.clone());
            }
            // V1.6: decode tagged variants
            if let (Some(serde_json::Value::String(tn)),
                    Some(serde_json::Value::String(vn))) =
                (obj.get("__variant__"), obj.get("__name__"))
            {
                let kind = obj.get("__kind__").and_then(|x| x.as_str()).unwrap_or("unit");
                let fields = match kind {
                    "tuple" => {
                        let items = obj.get("__fields__")
                            .and_then(|x| x.as_array())
                            .map(|arr| arr.iter().map(json_to_stored).collect())
                            .unwrap_or_default();
                        StoredVariantFields::Tuple(items)
                    }
                    "struct" => {
                        let items = obj.get("__fields__")
                            .and_then(|x| x.as_array())
                            .map(|arr| arr.iter().filter_map(|el| {
                                let pair = el.as_array()?;
                                let key = pair.get(0)?.as_str()?.to_string();
                                let val = json_to_stored(pair.get(1)?);
                                Some((key, val))
                            }).collect())
                            .unwrap_or_default();
                        StoredVariantFields::Struct(items)
                    }
                    _ => StoredVariantFields::Unit,
                };
                return StoredValue::Variant {
                    type_name: tn.clone(),
                    variant: vn.clone(),
                    fields,
                };
            }
            StoredValue::Map(obj.iter().map(|(k, v)| (k.clone(), json_to_stored(v))).collect())
        }
    }
}

/// Convert a StoredValue to serde_json::Value.
/// Variants serialize as `{ "__variant__": type, "__name__": variant, "__kind__": ..., "__fields__": ... }`
pub(crate) fn stored_to_json(v: &StoredValue) -> serde_json::Value {
    match v {
        StoredValue::Int(n) => serde_json::Value::Number((*n).into()),
        StoredValue::BigInt(d) => serde_json::json!({"__bigint__": d}),
        StoredValue::Float(n) if n.is_nan() => serde_json::json!({"__float__": "NaN"}),
        StoredValue::Float(n) if n.is_infinite() => serde_json::json!({"__float__": if *n > 0.0 { "inf" } else { "-inf" }}),
        StoredValue::Float(n) => serde_json::json!(*n),
        StoredValue::String(s) => serde_json::Value::String(s.clone()),
        StoredValue::Bool(b) => serde_json::Value::Bool(*b),
        StoredValue::Null => serde_json::Value::Null,
        StoredValue::List(items) => {
            serde_json::Value::Array(items.iter().map(stored_to_json).collect())
        }
        StoredValue::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map.iter()
                .map(|(k, v)| (escape_map_key(k), stored_to_json(v)))
                .collect();
            // a user map whose keys look like the encoding's own tags
            // (`__variant__`, `__bigint__`, `__init__`…) is wrapped, so it
            // comes back as the map it was — never as a forged variant
            if obj.keys().any(|k| k.starts_with("__")) {
                serde_json::json!({"__map__": serde_json::Value::Object(obj)})
            } else {
                serde_json::Value::Object(obj)
            }
        }
        StoredValue::Variant { type_name, variant, fields } => {
            let (kind, encoded) = match fields {
                StoredVariantFields::Unit => ("unit", serde_json::Value::Null),
                StoredVariantFields::Tuple(items) => (
                    "tuple",
                    serde_json::Value::Array(items.iter().map(stored_to_json).collect()),
                ),
                StoredVariantFields::Struct(entries) => (
                    "struct",
                    serde_json::Value::Array(entries.iter().map(|(k, v)| {
                        serde_json::Value::Array(vec![
                            serde_json::Value::String(k.clone()),
                            stored_to_json(v),
                        ])
                    }).collect()),
                ),
            };
            serde_json::json!({
                "__variant__": type_name,
                "__name__": variant,
                "__kind__": kind,
                "__fields__": encoded,
            })
        }
    }
}
