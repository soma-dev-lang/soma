//! Legacy JSON storage. Each file update is atomic; this backend does not
//! provide SQLite's multi-slot transactions or cross-process isolation.
use super::storage::{
    data_dir, has_storage_error, json_to_stored, note_read_error, note_write_error, stored_to_json,
    StorageBackend, StoredValue,
};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Clone, Default)]
struct Data {
    map: HashMap<String, StoredValue>,
    log: Vec<StoredValue>,
}
impl Data {
    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self
            .map
            .keys()
            .filter(|k| !k.starts_with("__"))
            .cloned()
            .collect();
        keys.sort();
        keys
    }
    fn list(&self) -> Vec<StoredValue> {
        if !self.log.is_empty() {
            self.log.clone()
        } else {
            self.keys().iter().map(|k| self.map[k].clone()).collect()
        }
    }
    fn migrate_list(&mut self) {
        if self.log.is_empty() {
            for k in self.keys() {
                self.log.push(self.map.remove(&k).unwrap());
            }
        } else {
            self.map.retain(|k, _| k.starts_with("__"));
        }
    }
}
pub struct FileBackend {
    path: PathBuf,
    data: RwLock<Data>,
    load_error: Option<String>,
}
impl FileBackend {
    pub fn new(cell: &str, slot: &str) -> Self {
        let path = data_dir().join(format!("{cell}_{slot}.json"));
        let (data, load_error) = match Self::load(&path) {
            Ok(data) => (data, None),
            Err(e) => {
                note_read_error(&path.display().to_string(), &e);
                (Data::default(), Some(e))
            }
        };
        Self {
            path,
            data: RwLock::new(data),
            load_error,
        }
    }
    fn load(path: &Path) -> Result<Data, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Data::default()),
            Err(e) => return Err(e.to_string()),
        };
        let root: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;
        let map = root
            .get("map")
            .and_then(|v| v.as_object())
            .ok_or("expected an object in 'map'")?;
        let log = root
            .get("log")
            .and_then(|v| v.as_array())
            .ok_or("expected an array in 'log'")?;
        Ok(Data {
            map: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_stored(v)))
                .collect(),
            log: log.iter().map(json_to_stored).collect(),
        })
    }
    fn readable(&self) -> bool {
        if let Some(e) = &self.load_error {
            note_read_error(&self.path.display().to_string(), e);
            false
        } else {
            true
        }
    }
    fn persist(&self, data: &Data) -> std::io::Result<()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let parent = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("storage file has no parent"))?;
        std::fs::create_dir_all(parent)?;
        let mut keys: Vec<_> = data.map.keys().collect();
        keys.sort();
        let map: serde_json::Map<String, serde_json::Value> = keys
            .into_iter()
            .map(|k| (k.clone(), stored_to_json(&data.map[k])))
            .collect();
        let payload = serde_json::json!({"map":map,"log":data.log.iter().map(stored_to_json).collect::<Vec<_>>()});
        let bytes = serde_json::to_vec_pretty(&payload)?;
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = self
            .path
            .with_extension(format!("json.{}.{}.tmp", std::process::id(), id));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        let outcome = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, &self.path)
        })();
        if outcome.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        outcome
    }
    fn update<T>(&self, f: impl FnOnce(&mut Data) -> T) -> Option<T> {
        if !self.readable() || has_storage_error() {
            return None;
        }
        let mut current = self.data.write().unwrap_or_else(|e| e.into_inner());
        let mut next = current.clone();
        let value = f(&mut next);
        match self.persist(&next) {
            Ok(()) => {
                *current = next;
                Some(value)
            }
            Err(e) => {
                note_write_error(&self.path.display().to_string(), &e);
                None
            }
        }
    }
}
impl StorageBackend for FileBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        if !self.readable() {
            return None;
        }
        self.data
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .map
            .get(key)
            .cloned()
    }
    fn set(&self, key: &str, value: StoredValue) {
        self.update(|d| {
            d.map.insert(key.into(), value);
        });
    }
    fn delete(&self, key: &str) -> bool {
        self.update(|d| d.map.remove(key).is_some())
            .unwrap_or(false)
    }
    fn append(&self, value: StoredValue) {
        self.update(|d| {
            d.migrate_list();
            d.log.push(value);
        });
    }
    fn unappend(&self) {
        self.update(|d| {
            d.migrate_list();
            d.log.pop();
        });
    }
    fn replace_list(&self, items: Vec<StoredValue>) {
        self.update(|d| {
            d.map.retain(|k, _| k.starts_with("__"));
            d.log = items;
        });
    }
    fn list(&self) -> Vec<StoredValue> {
        if !self.readable() {
            return vec![];
        }
        self.data.read().unwrap_or_else(|e| e.into_inner()).list()
    }
    fn keys(&self) -> Vec<String> {
        if !self.readable() {
            return vec![];
        }
        self.data.read().unwrap_or_else(|e| e.into_inner()).keys()
    }
    fn values(&self) -> Vec<StoredValue> {
        if !self.readable() {
            return vec![];
        }
        let d = self.data.read().unwrap_or_else(|e| e.into_inner());
        d.keys().iter().map(|k| d.map[k].clone()).collect()
    }
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    fn len(&self) -> usize {
        if !self.readable() {
            return 0;
        }
        let d = self.data.read().unwrap_or_else(|e| e.into_inner());
        d.keys().len() + d.log.len()
    }
    fn backend_name(&self) -> &str {
        "file"
    }
}
