use super::super::{Value, RuntimeError};
use super::val_to_i64;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "now" | "timestamp" => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(Ok(Value::Int(ts)))
        }
        "now_ms" => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            Some(Ok(Value::Int(ts)))
        }
        "sleep" | "wait" => {
            if let Some(ms) = args.first().map(|a| val_to_i64(a)) {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                Some(Ok(Value::Unit))
            } else {
                Some(Ok(Value::Unit))
            }
        }
        _ => None,
    }
}
