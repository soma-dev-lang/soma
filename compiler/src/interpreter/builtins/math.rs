use super::super::{Value, RuntimeError};
use super::val_to_i64;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "abs" => {
            args.first().map(|arg| match arg {
                Value::Int(n) => Ok(Value::Int(n.abs())),
                Value::Float(n) => Ok(Value::Float(n.abs())),
                _ => Err(RuntimeError::TypeError("abs expects a number".to_string())),
            })
        }
        "round" => {
            args.first().map(|a| match a {
                Value::Float(n) => Ok(Value::Int(n.round() as i64)),
                Value::Int(n) => Ok(Value::Int(*n)),
                _ => Ok(Value::Int(0)),
            })
        }
        "floor" => { args.first().map(|a| Ok(Value::Int(match a { Value::Float(n) => n.floor() as i64, Value::Int(n) => *n, _ => 0 }))) }
        "ceil" => { args.first().map(|a| Ok(Value::Int(match a { Value::Float(n) => n.ceil() as i64, Value::Int(n) => *n, _ => 0 }))) }
        "sqrt" => { args.first().map(|a| Ok(Value::Float(match a { Value::Float(n) => n.sqrt(), Value::Int(n) => (*n as f64).sqrt(), _ => 0.0 }))) }
        "pow" => {
            if args.len() >= 2 {
                let base = match &args[0] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                let exp = match &args[1] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                Some(Ok(Value::Float(base.powf(exp))))
            } else { Some(Ok(Value::Float(0.0))) }
        }
        "min" => {
            if args.len() >= 2 {
                match (&args[0], &args[1]) {
                    (Value::Float(_), _) | (_, Value::Float(_)) => {
                        let a = match &args[0] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        let b = match &args[1] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        Some(Ok(Value::Float(a.min(b))))
                    }
                    _ => { let a = val_to_i64(&args[0]); let b = val_to_i64(&args[1]); Some(Ok(Value::Int(a.min(b)))) }
                }
            }
            else { args.first().map(|a| Ok(a.clone())) }
        }
        "max" => {
            if args.len() >= 2 {
                match (&args[0], &args[1]) {
                    (Value::Float(_), _) | (_, Value::Float(_)) => {
                        let a = match &args[0] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        let b = match &args[1] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        Some(Ok(Value::Float(a.max(b))))
                    }
                    _ => { let a = val_to_i64(&args[0]); let b = val_to_i64(&args[1]); Some(Ok(Value::Int(a.max(b)))) }
                }
            }
            else { args.first().map(|a| Ok(a.clone())) }
        }
        "clamp" => {
            if args.len() >= 3 {
                match (&args[0], &args[1], &args[2]) {
                    (Value::Float(_), _, _) | (_, Value::Float(_), _) | (_, _, Value::Float(_)) => {
                        let v = match &args[0] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        let lo = match &args[1] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        let hi = match &args[2] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                        if lo > hi {
                            return Some(Err(RuntimeError::TypeError(format!("clamp: min ({}) must be <= max ({})", lo, hi))));
                        }
                        Some(Ok(Value::Float(v.max(lo).min(hi))))
                    }
                    _ => {
                        let v = val_to_i64(&args[0]);
                        let lo = val_to_i64(&args[1]);
                        let hi = val_to_i64(&args[2]);
                        if lo > hi {
                            return Some(Err(RuntimeError::TypeError(format!("clamp: min ({}) must be <= max ({})", lo, hi))));
                        }
                        Some(Ok(Value::Int(v.max(lo).min(hi))))
                    }
                }
            } else {
                Some(Err(RuntimeError::TypeError("clamp expects (value, min, max)".to_string())))
            }
        }
        _ => None,
    }
}
