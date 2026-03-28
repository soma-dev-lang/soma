use crate::runtime::storage::{StorageBackend, StoredValue};

/// HTTP-based storage backend — proxies all calls to an external sidecar.
///
/// Protocol: POST JSON to 9 endpoints.
/// Any language can implement a sidecar: Python, Node, Go, Rust.
///
/// StoredValue encoding:
///   Int(42) → {"type":"int","value":42}
///   String("hi") → {"type":"string","value":"hi"}
///   Null → {"type":"null"}
pub struct HttpBackend {
    base_url: String,
    cell_name: String,
    field_name: String,
}

impl HttpBackend {
    pub fn new(base_url: &str, cell_name: &str, field_name: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            cell_name: cell_name.to_string(),
            field_name: field_name.to_string(),
        }
    }

    fn post(&self, endpoint: &str, extra: serde_json::Value) -> Option<serde_json::Value> {
        let url = format!("{}/{}", self.base_url, endpoint);
        let mut body = serde_json::json!({
            "cell": self.cell_name,
            "field": self.field_name,
        });
        if let (Some(base), Some(ext)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in ext { base.insert(k.clone(), v.clone()); }
        }

        match ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_string(&serde_json::to_string(&body).unwrap_or_default())
        {
            Ok(resp) => {
                let text = resp.into_string().unwrap_or_default();
                serde_json::from_str(&text).ok()
            }
            Err(e) => {
                eprintln!("storage provider at {} is not reachable: {} — is the sidecar running?", self.base_url, e);
                None
            }
        }
    }

    fn encode_value(v: &StoredValue) -> serde_json::Value {
        match v {
            StoredValue::Int(n) => serde_json::json!({"type": "int", "value": n}),
            StoredValue::Float(n) => serde_json::json!({"type": "float", "value": n}),
            StoredValue::String(s) => serde_json::json!({"type": "string", "value": s}),
            StoredValue::Bool(b) => serde_json::json!({"type": "bool", "value": b}),
            StoredValue::Null => serde_json::json!({"type": "null"}),
            StoredValue::List(items) => serde_json::json!({"type": "list", "value": items.iter().map(Self::encode_value).collect::<Vec<_>>()}),
            StoredValue::Map(m) => {
                let obj: serde_json::Map<String, serde_json::Value> = m.iter().map(|(k, v)| (k.clone(), Self::encode_value(v))).collect();
                serde_json::json!({"type": "map", "value": obj})
            }
        }
    }

    fn decode_value(v: &serde_json::Value) -> StoredValue {
        if v.is_null() { return StoredValue::Null; }
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let val = v.get("value");
        match typ {
            "int" => StoredValue::Int(val.and_then(|v| v.as_i64()).unwrap_or(0)),
            "float" => StoredValue::Float(val.and_then(|v| v.as_f64()).unwrap_or(0.0)),
            "string" => StoredValue::String(val.and_then(|v| v.as_str()).unwrap_or("").to_string()),
            "bool" => StoredValue::Bool(val.and_then(|v| v.as_bool()).unwrap_or(false)),
            "null" => StoredValue::Null,
            "list" => {
                let items = val.and_then(|v| v.as_array()).map(|a| a.iter().map(Self::decode_value).collect()).unwrap_or_default();
                StoredValue::List(items)
            }
            "map" => {
                let entries = val.and_then(|v| v.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), Self::decode_value(v))).collect()).unwrap_or_default();
                StoredValue::Map(entries)
            }
            // Fallback: try as plain string/number
            _ => {
                if let Some(s) = v.as_str() { StoredValue::String(s.to_string()) }
                else if let Some(n) = v.as_i64() { StoredValue::Int(n) }
                else if let Some(b) = v.as_bool() { StoredValue::Bool(b) }
                else { StoredValue::Null }
            }
        }
    }
}

impl StorageBackend for HttpBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        let resp = self.post("get", serde_json::json!({"key": key}))?;
        let val = resp.get("value")?;
        if val.is_null() { None } else { Some(Self::decode_value(val)) }
    }

    fn set(&self, key: &str, value: StoredValue) {
        self.post("set", serde_json::json!({"key": key, "value": Self::encode_value(&value)}));
    }

    fn delete(&self, key: &str) -> bool {
        self.post("delete", serde_json::json!({"key": key}))
            .and_then(|r| r.get("deleted")?.as_bool())
            .unwrap_or(false)
    }

    fn append(&self, value: StoredValue) {
        self.post("append", serde_json::json!({"value": Self::encode_value(&value)}));
    }

    fn list(&self) -> Vec<StoredValue> {
        self.post("list", serde_json::json!({}))
            .and_then(|r| r.get("items")?.as_array().map(|a| a.iter().map(Self::decode_value).collect()))
            .unwrap_or_default()
    }

    fn keys(&self) -> Vec<String> {
        self.post("keys", serde_json::json!({}))
            .and_then(|r| r.get("keys")?.as_array().map(|a| {
                a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
            }))
            .unwrap_or_default()
    }

    fn values(&self) -> Vec<StoredValue> {
        self.post("values", serde_json::json!({}))
            .and_then(|r| r.get("values")?.as_array().map(|a| a.iter().map(Self::decode_value).collect()))
            .unwrap_or_default()
    }

    fn has(&self, key: &str) -> bool {
        self.post("has", serde_json::json!({"key": key}))
            .and_then(|r| r.get("exists")?.as_bool())
            .unwrap_or(false)
    }

    fn len(&self) -> usize {
        self.post("len", serde_json::json!({}))
            .and_then(|r| r.get("len")?.as_u64())
            .unwrap_or(0) as usize
    }

    fn backend_name(&self) -> &str {
        "http"
    }
}
