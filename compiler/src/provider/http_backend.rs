use std::sync::Arc;
use crate::runtime::storage::{StorageBackend, StoredValue};

/// HTTP-based storage backend — calls an external provider sidecar.
/// Protocol: simple JSON over HTTP.
///
/// Endpoints:
///   POST /get     { "key": "k" }              → { "value": "v" } or { "value": null }
///   POST /set     { "key": "k", "value": "v" } → { "ok": true }
///   POST /delete  { "key": "k" }              → { "ok": true }
///   POST /keys    {}                           → { "keys": ["a", "b"] }
///   POST /len     {}                           → { "len": 42 }
pub struct HttpBackend {
    base_url: String,
    table: String,
}

impl HttpBackend {
    pub fn new(base_url: &str, cell_name: &str, field_name: &str) -> Self {
        let table = format!("{}_{}", cell_name, field_name);
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            table,
        }
    }

    fn post(&self, endpoint: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
        let url = format!("{}/{}", self.base_url, endpoint);
        let mut payload = body.clone();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("table".to_string(), serde_json::Value::String(self.table.clone()));
        }

        let client = std::net::TcpStream::connect_timeout(
            &self.base_url.replace("http://", "").parse().ok()?,
            std::time::Duration::from_secs(5),
        ).ok()?;
        drop(client);

        // Use ureq-like minimal HTTP POST
        let body_str = serde_json::to_string(&payload).ok()?;
        let host = self.base_url.replace("http://", "");

        let request = format!(
            "POST /{} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            endpoint, host, body_str.len(), body_str
        );

        let mut stream = std::net::TcpStream::connect(&host).ok()?;
        use std::io::{Write, Read};
        stream.write_all(request.as_bytes()).ok()?;

        let mut response = String::new();
        stream.read_to_string(&mut response).ok()?;

        // Parse HTTP response — find the JSON body after \r\n\r\n
        let body_start = response.find("\r\n\r\n").map(|i| i + 4)?;
        let json_str = &response[body_start..];
        serde_json::from_str(json_str).ok()
    }
}

impl StorageBackend for HttpBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        let body = serde_json::json!({ "key": key });
        let resp = self.post("get", &body)?;
        let val = resp.get("value")?;
        if val.is_null() {
            None
        } else {
            Some(StoredValue::String(val.as_str().unwrap_or("").to_string()))
        }
    }

    fn set(&self, key: &str, value: StoredValue) {
        let val_str = match &value {
            StoredValue::String(s) => s.clone(),
            StoredValue::Int(n) => n.to_string(),
            StoredValue::Float(n) => n.to_string(),
            StoredValue::Bool(b) => b.to_string(),
            _ => format!("{}", value),
        };
        let body = serde_json::json!({ "key": key, "value": val_str });
        self.post("set", &body);
    }

    fn delete(&self, key: &str) -> bool {
        let body = serde_json::json!({ "key": key });
        self.post("delete", &body).is_some()
    }

    fn append(&self, value: StoredValue) {
        let val_str = format!("{}", value);
        let body = serde_json::json!({ "value": val_str });
        self.post("append", &body);
    }

    fn list(&self) -> Vec<StoredValue> {
        let resp = self.post("list", &serde_json::json!({}));
        if let Some(resp) = resp {
            if let Some(arr) = resp.get("values").and_then(|v| v.as_array()) {
                return arr.iter()
                    .map(|v| StoredValue::String(v.as_str().unwrap_or("").to_string()))
                    .collect();
            }
        }
        vec![]
    }

    fn keys(&self) -> Vec<String> {
        let resp = self.post("keys", &serde_json::json!({}));
        if let Some(resp) = resp {
            if let Some(arr) = resp.get("keys").and_then(|v| v.as_array()) {
                return arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .filter(|k| !k.starts_with("__"))
                    .collect();
            }
        }
        vec![]
    }

    fn values(&self) -> Vec<StoredValue> {
        self.list()
    }

    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    fn len(&self) -> usize {
        let resp = self.post("len", &serde_json::json!({}));
        resp.and_then(|r| r.get("len")?.as_u64()).unwrap_or(0) as usize
    }

    fn backend_name(&self) -> &str {
        "http"
    }
}
