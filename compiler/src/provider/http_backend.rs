use crate::runtime::storage::{
    has_storage_error, note_read_error, note_write_error, StorageBackend, StoredValue,
};
use std::io::Read;

/// HTTP storage adapter. Requests are not retried: a transport failure can
/// leave a remote write's outcome unknown. This protocol has no transactions.
pub struct HttpBackend {
    base_url: String,
    cell_name: String,
    field_name: String,
}
impl HttpBackend {
    pub fn new(base_url: &str, cell_name: &str, field_name: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').into(),
            cell_name: cell_name.into(),
            field_name: field_name.into(),
        }
    }
    fn post(&self, endpoint: &str, extra: serde_json::Value) -> Result<serde_json::Value, String> {
        const MAX_RESPONSE: u64 = 16 * 1024 * 1024;
        let mut body = serde_json::json!({"cell":self.cell_name,"field":self.field_name});
        if let (Some(base), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in extra {
                base.insert(k.clone(), v.clone());
            }
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .redirects(0)
            .build();
        let response = agent
            .post(&format!("{}/{endpoint}", self.base_url))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| e.to_string())?;
        if !(200..300).contains(&response.status()) {
            return Err(format!("unexpected HTTP status {}", response.status()));
        }
        if response
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > MAX_RESPONSE)
        {
            return Err("response exceeds 16 MiB".into());
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_RESPONSE {
            return Err("response exceeds 16 MiB".into());
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("invalid JSON response: {e}"))?;
        if !value.is_object() {
            return Err("response must be an object".into());
        }
        if value.get("error").is_some_and(|v| !v.is_null()) {
            return Err("provider returned an error".into());
        }
        Ok(value)
    }
    fn operation<T>(
        &self,
        endpoint: &str,
        write: bool,
        extra: serde_json::Value,
        decode: impl FnOnce(&serde_json::Value) -> Result<T, String>,
    ) -> Option<T> {
        if has_storage_error() {
            return None;
        }
        match self.post(endpoint, extra).and_then(|v| decode(&v)) {
            Ok(v) => Some(v),
            Err(e) => {
                let what = format!("HTTP storage {endpoint}");
                if write {
                    note_write_error(&what, &e);
                } else {
                    note_read_error(&what, &e);
                }
                None
            }
        }
    }
    fn acknowledged(value: &serde_json::Value) -> Result<(), String> {
        if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(())
        } else {
            Err("expected ok: true".into())
        }
    }
    fn encode_value(v: &StoredValue) -> serde_json::Value {
        match v {
            StoredValue::Int(n) => serde_json::json!({"type": "int", "value": n}),
            StoredValue::BigInt(d) => serde_json::json!({"type": "bigint", "value": d}),
            StoredValue::Float(n) if n.is_nan() => {
                serde_json::json!({"type":"float","value":"NaN"})
            }
            StoredValue::Float(n) if n.is_infinite() => {
                serde_json::json!({"type":"float","value":if *n > 0.0 { "inf" } else { "-inf" }})
            }
            StoredValue::Float(n) => serde_json::json!({"type": "float", "value": n}),
            StoredValue::String(s) => serde_json::json!({"type": "string", "value": s}),
            StoredValue::Bool(b) => serde_json::json!({"type": "bool", "value": b}),
            StoredValue::Null => serde_json::json!({"type": "null"}),
            StoredValue::List(items) => {
                serde_json::json!({"type": "list", "value": items.iter().map(Self::encode_value).collect::<Vec<_>>()})
            }
            StoredValue::Map(m) => {
                let obj: serde_json::Map<String, serde_json::Value> = m
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::encode_value(v)))
                    .collect();
                serde_json::json!({"type": "map", "value": obj})
            }
            StoredValue::Variant {
                type_name,
                variant,
                fields,
            } => {
                use crate::runtime::storage::StoredVariantFields;
                let (kind, encoded) = match fields {
                    StoredVariantFields::Unit => ("unit", serde_json::Value::Null),
                    StoredVariantFields::Tuple(items) => (
                        "tuple",
                        serde_json::Value::Array(items.iter().map(Self::encode_value).collect()),
                    ),
                    StoredVariantFields::Struct(entries) => (
                        "struct",
                        serde_json::Value::Array(
                            entries
                                .iter()
                                .map(|(k, v)| {
                                    serde_json::Value::Array(vec![
                                        serde_json::Value::String(k.clone()),
                                        Self::encode_value(v),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                };
                serde_json::json!({
                    "type": "variant",
                    "type_name": type_name,
                    "variant": variant,
                    "kind": kind,
                    "fields": encoded,
                })
            }
        }
    }

    fn decode_value(v: &serde_json::Value) -> Result<StoredValue, String> {
        Self::decode_at(v, 0)
    }
    fn decode_at(v: &serde_json::Value, depth: usize) -> Result<StoredValue, String> {
        use crate::runtime::storage::StoredVariantFields;
        if depth > 100 {
            return Err("value nesting exceeds 100 levels".into());
        }
        let invalid = || "invalid typed storage value".to_string();
        if v.is_null() {
            return Ok(StoredValue::Null);
        }
        // Legacy providers may send primitive values without an envelope.
        if let Some(s) = v.as_str() {
            return Ok(StoredValue::String(s.into()));
        }
        if let Some(b) = v.as_bool() {
            return Ok(StoredValue::Bool(b));
        }
        if v.is_number() {
            if let Some(n) = v.as_i64() {
                return Ok(StoredValue::Int(n));
            }
            let text = v.to_string();
            if valid_integer(&text) {
                return Ok(StoredValue::BigInt(text));
            }
            return v
                .as_f64()
                .filter(|n| n.is_finite())
                .map(StoredValue::Float)
                .ok_or_else(invalid);
        }
        let tag = v.get("type").and_then(|v| v.as_str()).ok_or_else(invalid)?;
        let value = v.get("value");
        let array = |value: Option<&serde_json::Value>| -> Result<Vec<StoredValue>, String> {
            value
                .and_then(|v| v.as_array())
                .ok_or_else(invalid)?
                .iter()
                .map(|v| Self::decode_at(v, depth + 1))
                .collect()
        };
        match tag {
            "null" => {
                if value.is_none_or(|v| v.is_null()) {
                    Ok(StoredValue::Null)
                } else {
                    Err(invalid())
                }
            }
            "int" => value
                .and_then(|v| v.as_i64())
                .map(StoredValue::Int)
                .ok_or_else(invalid),
            "bigint" => value
                .and_then(|v| v.as_str())
                .filter(|s| valid_integer(s))
                .map(|s| StoredValue::BigInt(s.into()))
                .ok_or_else(invalid),
            "float" => {
                let value = value.ok_or_else(invalid)?;
                let n = match value.as_str() {
                    Some("NaN") => Some(f64::NAN),
                    Some("inf") => Some(f64::INFINITY),
                    Some("-inf") => Some(f64::NEG_INFINITY),
                    _ => value.as_f64().filter(|n| n.is_finite()),
                };
                n.map(StoredValue::Float).ok_or_else(invalid)
            }
            "bool" => value
                .and_then(|v| v.as_bool())
                .map(StoredValue::Bool)
                .ok_or_else(invalid),
            "string" => value
                .and_then(|v| v.as_str())
                .map(|s| StoredValue::String(s.into()))
                .ok_or_else(invalid),
            "list" => array(value).map(StoredValue::List),
            "map" => {
                let map = value.and_then(|v| v.as_object()).ok_or_else(invalid)?;
                map.iter()
                    .map(|(k, v)| Ok((k.clone(), Self::decode_at(v, depth + 1)?)))
                    .collect::<Result<_, String>>()
                    .map(StoredValue::Map)
            }
            "variant" => {
                let type_name = v
                    .get("type_name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?
                    .into();
                let variant = v
                    .get("variant")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?
                    .into();
                let fields = match v.get("kind").and_then(|v| v.as_str()) {
                    Some("unit") if v.get("fields").is_none_or(|v| v.is_null()) => {
                        StoredVariantFields::Unit
                    }
                    Some("tuple") => StoredVariantFields::Tuple(array(v.get("fields"))?),
                    Some("struct") => {
                        let entries = v
                            .get("fields")
                            .and_then(|v| v.as_array())
                            .ok_or_else(invalid)?;
                        let mut seen = std::collections::HashSet::new();
                        let mut fields = Vec::new();
                        for entry in entries {
                            let pair = entry
                                .as_array()
                                .filter(|p| p.len() == 2)
                                .ok_or_else(invalid)?;
                            let name = pair[0].as_str().ok_or_else(invalid)?;
                            if !seen.insert(name) {
                                return Err("duplicate variant field".into());
                            }
                            fields.push((name.into(), Self::decode_at(&pair[1], depth + 1)?));
                        }
                        StoredVariantFields::Struct(fields)
                    }
                    _ => return Err(invalid()),
                };
                Ok(StoredValue::Variant {
                    type_name,
                    variant,
                    fields,
                })
            }
            _ => Err(format!("unknown storage type '{tag}'")),
        }
    }
    fn items(value: &serde_json::Value, field: &str) -> Result<Vec<StoredValue>, String> {
        value
            .get(field)
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("expected array '{field}'"))?
            .iter()
            .map(Self::decode_value)
            .collect()
    }
}
fn valid_integer(s: &str) -> bool {
    let d = s.strip_prefix('-').unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}
impl StorageBackend for HttpBackend {
    fn get(&self, key: &str) -> Option<StoredValue> {
        self.operation("get", false, serde_json::json!({"key":key}), |r| {
            let value = r.get("value").ok_or("missing 'value' in response")?;
            if value.is_null() {
                Ok(None)
            } else {
                Self::decode_value(value).map(Some)
            }
        })
        .flatten()
    }
    fn set(&self, key: &str, value: StoredValue) {
        self.operation(
            "set",
            true,
            serde_json::json!({"key":key,"value":Self::encode_value(&value)}),
            Self::acknowledged,
        );
    }
    fn delete(&self, key: &str) -> bool {
        self.operation("delete", true, serde_json::json!({"key":key}), |r| {
            r.get("deleted")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| "expected boolean 'deleted'".into())
        })
        .unwrap_or(false)
    }
    fn append(&self, value: StoredValue) {
        self.operation(
            "append",
            true,
            serde_json::json!({"value":Self::encode_value(&value)}),
            Self::acknowledged,
        );
    }
    fn unappend(&self) {
        self.operation("unappend", true, serde_json::json!({}), Self::acknowledged);
    }
    fn list(&self) -> Vec<StoredValue> {
        self.operation("list", false, serde_json::json!({}), |v| {
            Self::items(v, "items")
        })
        .unwrap_or_default()
    }
    fn keys(&self) -> Vec<String> {
        self.operation("keys", false, serde_json::json!({}), |r| {
            r.get("keys")
                .and_then(|v| v.as_array())
                .ok_or("expected array 'keys'")?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "key is not a String".into())
                })
                .collect()
        })
        .unwrap_or_default()
    }
    fn values(&self) -> Vec<StoredValue> {
        self.operation("values", false, serde_json::json!({}), |v| {
            Self::items(v, "values")
        })
        .unwrap_or_default()
    }
    fn has(&self, key: &str) -> bool {
        self.operation("has", false, serde_json::json!({"key":key}), |r| {
            r.get("exists")
                .and_then(|v| v.as_bool())
                .ok_or_else(|| "expected boolean 'exists'".into())
        })
        .unwrap_or(false)
    }
    fn len(&self) -> usize {
        self.operation("len", false, serde_json::json!({}), |r| {
            r.get("len")
                .and_then(|v| v.as_u64())
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| "expected non-negative machine-size 'len'".into())
        })
        .unwrap_or(0)
    }
    fn backend_name(&self) -> &str {
        "http"
    }
}
