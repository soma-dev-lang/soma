use super::super::{Value, RuntimeError, Interpreter};

pub fn call_builtin(interp: &Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    match name {
        "next_id" => {
            let counter_key = "__next_id";
            let slot = interp.storage.iter()
                .find(|(k, _)| k.starts_with(&format!("{}.", cell_name)))
                .or_else(|| interp.storage.iter().next())
                .map(|(_, v)| v);
            if let Some(backend) = slot {
                let current = backend.get(counter_key)
                    .and_then(|v| match v {
                        crate::runtime::storage::StoredValue::Int(n) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(0);
                let next = current + 1;
                backend.set(counter_key, crate::runtime::storage::StoredValue::Int(next));
                Some(Ok(Value::Int(next)))
            } else {
                Some(Ok(Value::Int(1)))
            }
        }
        "transition" => {
            if args.len() >= 2 {
                let id = format!("{}", args[0]);
                let target = format!("{}", args[1]);
                Some(interp.do_transition(&id, &target))
            } else {
                Some(Err(RuntimeError::TypeError("transition(id, target_state) requires 2 args".to_string())))
            }
        }
        "get_status" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(interp.do_get_status(&id_str))
            } else {
                Some(Err(RuntimeError::TypeError("get_status(id) requires 1 arg".to_string())))
            }
        }
        "valid_transitions" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(Ok(interp.do_valid_transitions(&id_str)))
            } else {
                Some(Err(RuntimeError::TypeError("valid_transitions(id) requires 1 arg".to_string())))
            }
        }
        _ => None,
    }
}
