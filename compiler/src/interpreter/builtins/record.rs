use super::super::{Value, RuntimeError};

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "is_type" | "is_a" => {
            if args.len() >= 2 {
                if let (Value::Map(entries), Value::String(expected)) = (&args[0], &args[1]) {
                    let actual = entries.get("_type")
                        .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None });
                    Some(Ok(Value::Bool(actual.as_deref() == Some(expected.as_str()))))
                } else {
                    Some(Ok(Value::Bool(false)))
                }
            } else {
                Some(Err(RuntimeError::TypeError("is_type expects (value, type_name)".to_string())))
            }
        }
        _ => None,
    }
}
