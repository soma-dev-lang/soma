use super::super::{Value, RuntimeError};
use super::val_to_i64;
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "abs" => {
            args.first().map(|arg| match arg {
                Value::Int(si) => {
                    if let Some(n) = si.to_i64() {
                        n.checked_abs()
                            .map(|v| Value::Int(SomaInt::from_i64(v)))
                            .ok_or_else(|| RuntimeError::TypeError("abs: integer overflow (i64::MIN has no positive equivalent)".to_string()))
                    } else {
                        // Big int: negate if negative
                        let s = format!("{}", si);
                        if s.starts_with('-') {
                            Ok(Value::Int(SomaInt::from_i64(0).sub(si.clone()).mul(SomaInt::from_i64(-1)).mul(SomaInt::from_i64(-1))))
                        } else {
                            Ok(Value::Int(si.clone()))
                        }
                    }
                }
                Value::Float(n) => Ok(Value::Float(n.abs())),
                _ => Err(RuntimeError::TypeError("abs expects a number".to_string())),
            })
        }
        "round" => {
            args.first().map(|a| match a {
                Value::Float(n) => {
                    let r = n.round();
                    if r.is_finite() && r >= i64::MIN as f64 && r <= i64::MAX as f64 {
                        Ok(Value::Int(SomaInt::from_i64(r as i64)))
                    } else {
                        Err(RuntimeError::TypeError(format!("round: {} is out of integer range", n)))
                    }
                }
                Value::Int(si) => Ok(Value::Int(si.clone())),
                _ => Ok(Value::Int(SomaInt::from_i64(0))),
            })
        }
        "floor" => {
            args.first().map(|a| match a {
                Value::Float(n) => {
                    let r = n.floor();
                    if r.is_finite() && r >= i64::MIN as f64 && r <= i64::MAX as f64 {
                        Ok(Value::Int(SomaInt::from_i64(r as i64)))
                    } else {
                        Err(RuntimeError::TypeError(format!("floor: {} is out of integer range", n)))
                    }
                }
                Value::Int(si) => Ok(Value::Int(si.clone())),
                _ => Ok(Value::Int(SomaInt::from_i64(0))),
            })
        }
        "ceil" => {
            args.first().map(|a| match a {
                Value::Float(n) => {
                    let r = n.ceil();
                    if r.is_finite() && r >= i64::MIN as f64 && r <= i64::MAX as f64 {
                        Ok(Value::Int(SomaInt::from_i64(r as i64)))
                    } else {
                        Err(RuntimeError::TypeError(format!("ceil: {} is out of integer range", n)))
                    }
                }
                Value::Int(si) => Ok(Value::Int(si.clone())),
                _ => Ok(Value::Int(SomaInt::from_i64(0))),
            })
        }
        "sqrt" => { args.first().map(|a| Ok(Value::Float(match a { Value::Float(n) => n.sqrt(), Value::Int(si) => si.to_f64().sqrt(), _ => 0.0 }))) }
        "log" | "ln" => {
            args.first().map(|a| {
                let n = match a { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                Ok(Value::Float(n.ln()))
            })
        }
        "exp" => {
            args.first().map(|a| {
                let n = match a { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                Ok(Value::Float(n.exp()))
            })
        }
        "log10" => {
            args.first().map(|a| {
                let n = match a { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                Ok(Value::Float(n.log10()))
            })
        }
        "pow" => {
            if args.len() >= 2 {
                let base = match &args[0] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                let exp = match &args[1] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                Some(Ok(Value::Float(base.powf(exp))))
            } else { Some(Ok(Value::Float(0.0))) }
        }
        "min" => {
            if args.len() >= 2 {
                match (&args[0], &args[1]) {
                    (Value::Float(_), _) | (_, Value::Float(_)) => {
                        let a = match &args[0] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                        let b = match &args[1] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                        Some(Ok(Value::Float(a.min(b))))
                    }
                    (Value::Int(a), Value::Int(b)) => {
                        if a.cmp(b) <= 0 { Some(Ok(Value::Int(a.clone()))) } else { Some(Ok(Value::Int(b.clone()))) }
                    }
                    _ => { let a = val_to_i64(&args[0]); let b = val_to_i64(&args[1]); Some(Ok(Value::Int(SomaInt::from_i64(a.min(b))))) }
                }
            }
            else { args.first().map(|a| Ok(a.clone())) }
        }
        "max" => {
            if args.len() >= 2 {
                match (&args[0], &args[1]) {
                    (Value::Float(_), _) | (_, Value::Float(_)) => {
                        let a = match &args[0] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                        let b = match &args[1] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                        Some(Ok(Value::Float(a.max(b))))
                    }
                    (Value::Int(a), Value::Int(b)) => {
                        if a.cmp(b) >= 0 { Some(Ok(Value::Int(a.clone()))) } else { Some(Ok(Value::Int(b.clone()))) }
                    }
                    _ => { let a = val_to_i64(&args[0]); let b = val_to_i64(&args[1]); Some(Ok(Value::Int(SomaInt::from_i64(a.max(b))))) }
                }
            }
            else { args.first().map(|a| Ok(a.clone())) }
        }
        "idiv" => {
            // Integer division: idiv(7, 2) = 3 (truncates toward zero)
            if args.len() >= 2 {
                let a = val_to_i64(&args[0]);
                let b = val_to_i64(&args[1]);
                if b == 0 {
                    Some(Err(RuntimeError::TypeError("division by zero".to_string())))
                } else {
                    Some(Ok(Value::Int(SomaInt::from_i64(a / b))))
                }
            } else {
                Some(Err(RuntimeError::TypeError("idiv(a, b) requires 2 args".to_string())))
            }
        }
        "clamp" => {
            if args.len() >= 3 {
                match (&args[0], &args[1], &args[2]) {
                    (Value::Float(_), _, _) | (_, Value::Float(_), _) | (_, _, Value::Float(_)) => {
                        let v = match &args[0] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                        let lo = match &args[1] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
                        let hi = match &args[2] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
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
                        Some(Ok(Value::Int(SomaInt::from_i64(v.max(lo).min(hi)))))
                    }
                }
            } else {
                Some(Err(RuntimeError::TypeError("clamp expects (value, min, max)".to_string())))
            }
        }
        // random() → float 0.0..1.0
        // random(max) → int 0..max (exclusive)
        // random(min, max) → int min..max (exclusive)
        "random" | "rand" => {
            use std::cell::Cell;
            use std::time::{SystemTime, UNIX_EPOCH};

            thread_local! {
                static RAND_COUNTER: Cell<u64> = Cell::new(0);
            }

            // Simple PRNG: use system time nanos as seed, mixed with a
            // per-thread counter so rapid successive calls differ.
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
                .unwrap_or_default().subsec_nanos() as u64;

            let mut x = RAND_COUNTER.with(|c| {
                let count = c.get();
                c.set(count.wrapping_add(1));
                nanos ^ count
            });

            // xorshift-style mixing
            x = x ^ (x >> 7) ^ (x << 13);
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);

            if args.is_empty() {
                // random() → float 0.0..1.0
                let f = (x % 1_000_000) as f64 / 1_000_000.0;
                Some(Ok(Value::Float(f)))
            } else if args.len() == 1 {
                // random(max) → int 0..max
                let max = val_to_i64(&args[0]);
                if max <= 0 { return Some(Ok(Value::Int(SomaInt::from_i64(0)))); }
                Some(Ok(Value::Int(SomaInt::from_i64((x % max as u64) as i64))))
            } else {
                // random(min, max) → int min..max
                let min = val_to_i64(&args[0]);
                let max = val_to_i64(&args[1]);
                if max <= min { return Some(Ok(Value::Int(SomaInt::from_i64(min)))); }
                let range = (max - min) as u64;
                Some(Ok(Value::Int(SomaInt::from_i64(min + (x % range) as i64))))
            }
        }
        // Bit operations on Int
        "band" | "bor" | "bxor" if args.len() >= 2 => {
            let a = val_to_i64(&args[0]);
            let b = val_to_i64(&args[1]);
            let r = match name { "band" => a & b, "bor" => a | b, _ => a ^ b };
            Some(Ok(Value::Int(SomaInt::from_i64(r))))
        }
        "bnot" if args.len() >= 1 => {
            let a = val_to_i64(&args[0]);
            Some(Ok(Value::Int(SomaInt::from_i64(!a))))
        }
        "shl" if args.len() >= 2 => {
            let a = val_to_i64(&args[0]);
            let b = val_to_i64(&args[1]);
            Some(Ok(Value::Int(SomaInt::from_i64(a.wrapping_shl(b as u32)))))
        }
        "shr" if args.len() >= 2 => {
            let a = val_to_i64(&args[0]);
            let b = val_to_i64(&args[1]);
            Some(Ok(Value::Int(SomaInt::from_i64(a.wrapping_shr(b as u32)))))
        }
        // Number theory
        "gcd" if args.len() >= 2 => {
            let mut a = val_to_i64(&args[0]).unsigned_abs();
            let mut b = val_to_i64(&args[1]).unsigned_abs();
            while b != 0 { let t = b; b = a % b; a = t; }
            Some(Ok(Value::Int(SomaInt::from_i64(a as i64))))
        }
        "sqrt_int" if args.len() >= 1 => {
            let a = val_to_i64(&args[0]);
            if a < 0 {
                Some(Err(RuntimeError::TypeError("sqrt_int: negative argument".to_string())))
            } else {
                Some(Ok(Value::Int(SomaInt::from_i64((a as f64).sqrt() as i64))))
            }
        }
        "pow_mod" if args.len() >= 3 => {
            let base = val_to_i64(&args[0]) as i128;
            let exp = val_to_i64(&args[1]);
            let m = val_to_i64(&args[2]) as i128;
            if m == 0 {
                return Some(Err(RuntimeError::TypeError("pow_mod: modulus is zero".to_string())));
            }
            let mut r: i128 = 1;
            let mut b = base.rem_euclid(m);
            let mut e = exp;
            while e > 0 {
                if e & 1 == 1 { r = (r * b).rem_euclid(m); }
                e >>= 1;
                b = (b * b).rem_euclid(m);
            }
            Some(Ok(Value::Int(SomaInt::from_i64(r as i64))))
        }
        _ => None,
    }
}
