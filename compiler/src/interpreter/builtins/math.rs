use super::super::{Value, RuntimeError};
use super::val_to_i64;
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "abs" => {
            args.first().map(|arg| match arg {
                Value::Int(si) => {
                    if let Some(n) = si.to_i64() {
                        // |i64::MIN| is 2^63: a BigInt, not an error (native agrees)
                        Ok(match n.checked_abs() {
                            Some(v) => Value::Int(SomaInt::from_i64(v)),
                            None => Value::Int(SomaInt::from_rug(-rug::Integer::from(n))),
                        })
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
        // round(x, digits): keep `digits` decimals → Float (round(2.345, 2) = 2.35)
        "round" if args.len() == 2 => {
            let x = match &args[0] {
                Value::Float(f) => *f,
                Value::Int(i) => i.to_f64(),
                _ => return Some(Err(RuntimeError::TypeError("round(x, digits): x must be a number".to_string()))),
            };
            let d = match &args[1] {
                Value::Int(i) if i.to_i64().map_or(false, |d| d >= 0) => i.to_i64().unwrap_or(0).min(15) as i32,
                Value::Int(_) => return Some(Err(RuntimeError::TypeError("round(x, digits): digits must be ≥ 0 (a negative value was ignored)".to_string()))),
                _ => return Some(Err(RuntimeError::TypeError("round(x, digits): digits must be an Int".to_string()))),
            };
            Some(Ok(Value::Float(round_decimal(x, d))))
        }
        // Ruby/Python-style floored division and modulo (the result of `mod`
        // has the divisor's sign; `%` keeps the dividend's sign like C/Rust)
        // Int arguments only: mod(7.5, 2) truncated to 1 (Python: 1.5)
        "floor_div" | "mod" | "divmod" | "div_round" | "idiv" if args.iter().any(|a| !matches!(a, Value::Int(_))) => {
            let bad = args.iter().find(|a| !matches!(a, Value::Int(_))).unwrap();
            Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                "{}() takes Ints, got {} {} — for Floats use `/`, floor() and `x - floor(x / y) * y`", name, super::super::value_type_name(bad), bad) }))
        }
        "floor_div" | "mod" | "divmod" if args.len() == 2 => {
            let (a, b) = (big_of(&args[0]), big_of(&args[1]));
            if b == 0 {
                return Some(Err(RuntimeError::TypeError(format!("{}(): division by zero", name))));
            }
            let (q, r) = a.div_rem_floor(b);
            Some(Ok(match name {
                "floor_div" => Value::Int(SomaInt::from_rug(q)),
                "mod" => Value::Int(SomaInt::from_rug(r)),
                _ => Value::List(vec![Value::Int(SomaInt::from_rug(q)), Value::Int(SomaInt::from_rug(r))]),
            }))
        }
        // exact integer division rounded to nearest, half away from zero
        // (BigDecimal HALF_UP on cents: div_round(cents * bps, 120000))
        "div_round" if args.len() == 2 => {
            let (a, b) = (big_of(&args[0]), big_of(&args[1]));
            if b == 0 {
                return Some(Err(RuntimeError::TypeError("div_round(): division by zero".to_string())));
            }
            let (q, r) = a.clone().div_rem(b.clone());
            let twice: rug::Integer = rug::Integer::from(&r * 2i32).abs();
            let bump = twice >= b.clone().abs();
            let q = if bump { if (a < 0) != (b < 0) { q - 1i32 } else { q + 1i32 } } else { q };
            Some(Ok(Value::Int(SomaInt::from_rug(q))))
        }
        "to_fixed" if args.len() == 2 => {
            let x = match &args[0] { Value::Float(f) => *f, Value::Int(i) => i.to_f64(), _ => return Some(Err(RuntimeError::TypeError("to_fixed(x, digits)".to_string()))) };
            // exactly `digits` decimals (it capped at 15 and ignored negatives)
            let d = match &args[1] { Value::Int(i) => i.to_i64().unwrap_or(-1), _ => -1 };
            if !(0..=100).contains(&d) {
                return Some(Err(RuntimeError::TypeError(format!("to_fixed(x, digits): digits must be an Int 0..100, got {}", args[1]))));
            }
            let d = d as usize;
            if d > 15 { return Some(Ok(Value::String(format!("{:.*}", d, x)))); }
            Some(Ok(Value::String(fixed_string(x, d))))
        }
        "round" => {
            args.first().map(|a| match a {
                Value::Float(n) => {
                    let r = n.round();
                    if r.is_finite() && r >= i64::MIN as f64 && r < 9223372036854775808.0 {  // (i64::MAX as f64 is 2^63: it saturated to 2^63 - 1)
                        Ok(Value::Int(SomaInt::from_i64(r as i64)))
                    } else if r.is_finite() {
                        // Int is arbitrary precision: 1e300 has an exact integer value
                        Ok(Value::Int(SomaInt::from_rug(rug::Integer::from_f64(r).unwrap())))
                    } else {
                        Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("round: {} has no integer value", n) })
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
                    if r.is_finite() && r >= i64::MIN as f64 && r < 9223372036854775808.0 {  // (i64::MAX as f64 is 2^63: it saturated to 2^63 - 1)
                        Ok(Value::Int(SomaInt::from_i64(r as i64)))
                    } else if r.is_finite() {
                        // Int is arbitrary precision: 1e300 has an exact integer value
                        Ok(Value::Int(SomaInt::from_rug(rug::Integer::from_f64(r).unwrap())))
                    } else {
                        Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("floor: {} has no integer value", n) })
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
                    if r.is_finite() && r >= i64::MIN as f64 && r < 9223372036854775808.0 {  // (i64::MAX as f64 is 2^63: it saturated to 2^63 - 1)
                        Ok(Value::Int(SomaInt::from_i64(r as i64)))
                    } else if r.is_finite() {
                        // Int is arbitrary precision: 1e300 has an exact integer value
                        Ok(Value::Int(SomaInt::from_rug(rug::Integer::from_f64(r).unwrap())))
                    } else {
                        Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("ceil: {} has no integer value", n) })
                    }
                }
                Value::Int(si) => Ok(Value::Int(si.clone())),
                _ => Ok(Value::Int(SomaInt::from_i64(0))),
            })
        }
        "chr" if args.len() == 1 => {
            let n = val_to_i64(&args[0]);
            Some(char::from_u32(n as u32).map(|c| Value::String(c.to_string()))
                .ok_or_else(|| RuntimeError::TypeError(format!("chr({}): not a Unicode scalar value", n))))
        }
        "ord" if args.len() == 1 => {
            let Value::String(t) = &args[0] else { return Some(Err(RuntimeError::TypeError("ord(s: String) — the first character's code point".to_string()))) };
            Some(t.chars().next().map(|c| Value::Int(SomaInt::from_i64(c as i64)))
                .ok_or_else(|| RuntimeError::TypeError("ord(\"\"): empty string".to_string())))
        }
        "sin" | "cos" | "tan" | "atan" | "atan2" => {
            let f = |v: &Value| match v { Value::Float(n) => Some(*n), Value::Int(si) => Some(si.to_f64()), _ => None };
            let Some(x) = args.first().and_then(f) else {
                return Some(Err(RuntimeError::TypeError(format!("{}(x: Float) needs a number", name))));
            };
            Some(Ok(Value::Float(match name {
                "sin" => x.sin(), "cos" => x.cos(), "tan" => x.tan(), "atan" => x.atan(),
                _ => { let Some(y) = args.get(1).and_then(f) else { return Some(Err(RuntimeError::TypeError("atan2(y, x)".to_string()))) }; x.atan2(y) }
            })))
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
            // a non-number was 0.0, silently
            match (args.first(), args.get(1)) {
                (Some(Value::Float(_) | Value::Int(_)), Some(Value::Float(_) | Value::Int(_))) => {}
                _ => return Some(Err(RuntimeError::TypeError(format!("pow(base, exp) needs two numbers, got {}", args.iter().map(|a| crate::interpreter::value_type_name(a)).collect::<Vec<_>>().join(", "))))),
            }
            let base = match &args[0] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
            let exp = match &args[1] { Value::Float(n) => *n, Value::Int(si) => si.to_f64(), _ => 0.0 };
            Some(Ok(Value::Float(base.powf(exp))))
        }
        // exact Int power: `to_int(pow(3, 40))` was off by 33 (a Float)
        "ipow" => {
            let (Some(Value::Int(b)), Some(Value::Int(e))) = (args.first(), args.get(1)) else {
                return Some(Err(RuntimeError::TypeError("ipow(base: Int, exp: Int) needs two Ints (pow() is the Float power)".to_string())));
            };
            // |base| <= 1: any non-negative exponent, however large
            if e.to_rug() >= 0 {
                match b.to_i64() {
                    Some(0) => return Some(Ok(Value::Int(SomaInt::from_i64(if e.to_rug() == 0 { 1 } else { 0 })))),
                    Some(1) => return Some(Ok(Value::Int(SomaInt::from_i64(1)))),
                    Some(-1) => return Some(Ok(Value::Int(SomaInt::from_i64(if e.to_rug().is_even() { 1 } else { -1 })))),
                    _ => {}
                }
            }
            let Some(mut e) = e.to_i64().filter(|e| *e >= 0) else {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range: ipow exponent must be a non-negative Int, got {}", e) }));
            };
            let base_bits = b.to_rug().significant_bits() as u64;
            if base_bits > 1 && (base_bits - 1).saturating_mul(e as u64) > crate::interpreter::soma_int::SomaInt::MAX_BITS {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range: ipow({}, {}) is past the limit of {} bits", b, e, crate::interpreter::soma_int::SomaInt::MAX_BITS) }));
            }
            let mut acc = crate::interpreter::soma_int::SomaInt::from_i64(1);
            let mut sq = b.clone();
            while e > 0 {
                if e & 1 == 1 {
                    acc = match acc.checked_big_mul(sq.clone()) { Ok(v) => v, Err(m) => return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: m })) };
                }
                e >>= 1;
                if e > 0 {
                    sq = match sq.clone().checked_big_mul(sq) { Ok(v) => v, Err(m) => return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: m })) };
                }
            }
            Some(Ok(Value::Int(acc)))
        }
        "sum" => Some(numeric_reduce(args, "sum")),
        "product" => Some(numeric_reduce(args, "product")),
        "avg" => Some(numeric_reduce(args, "avg")),
        "median" | "variance" | "pvariance" | "stddev" | "pstdev" | "stdev" => Some(stats_reduce(args, name)),
        "min" if args.len() == 1 && matches!(args.first(), Some(Value::List(_))) => {
            Some(numeric_reduce(args, "min"))
        }
        "max" if args.len() == 1 && matches!(args.first(), Some(Value::List(_))) => {
            Some(numeric_reduce(args, "max"))
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
        // parse_int("42") = 42; anything that is not exactly an integer
        // ("1.5", "12abc", "", " 7") is () — to_int() is lenient and truncates.
        // parse_int(s, base): base 2..36, digits only (no 0x prefix) — a
        // sha256 hex prefix needed a hand-written digit loop
        "parse_int" if args.len() == 2 => {
            let (Some(Value::String(t)), Some(Value::Int(b))) = (args.first(), args.get(1)) else {
                return Some(Err(RuntimeError::TypeError("parse_int(s: String, base: Int)".to_string())));
            };
            let Some(base) = b.to_i64().filter(|b| (2..=36).contains(b)) else {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range: parse_int base must be 2..36, got {}", b) }));
            };
            let (neg, digits) = match t.strip_prefix('-') { Some(d) => (true, d), None => (false, t.strip_prefix('+').unwrap_or(t)) };
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(base as u32)) {
                return Some(Ok(Value::Unit));
            }
            if (digits.len() as u64).saturating_mul(6) > SomaInt::MAX_BITS {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: "range: parse_int input is past the Int size limit".to_string() }));
            }
            match rug::Integer::from_str_radix(digits, base as i32) {
                Ok(n) => { let n = if neg { -n } else { n }; Some(Ok(Value::Int(SomaInt::from_decimal_str(&n.to_string())))) }
                Err(_) => Some(Ok(Value::Unit)),
            }
        }
        "parse_int" => {
            Some(Ok(match args.first() {
                Some(Value::String(t)) => {
                    let t = t.as_str();
                    let digits = t.strip_prefix('-').or_else(|| t.strip_prefix('+')).unwrap_or(t);
                    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                        // the Int size cap holds for parsed text too (~5.05M digits)
                        if digits.len() > 5_050_446 {
                            return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range: parse_int of {} digits is past the Int limit of {} bits", digits.len(), SomaInt::MAX_BITS) }));
                        }
                        Value::Int(SomaInt::from_decimal_str(t.strip_prefix('+').unwrap_or(t)))
                    } else {
                        Value::Unit
                    }
                }
                Some(Value::Int(i)) => Value::Int(i.clone()),
                _ => Value::Unit,
            }))
        }
        "parse_float" => {
            Some(Ok(match args.first() {
                Some(Value::String(t)) if !t.trim().is_empty() && t.trim() == t.as_str() => {
                    t.parse::<f64>().ok().filter(|f| f.is_finite()).map(Value::Float).unwrap_or(Value::Unit)
                }
                Some(Value::Float(f)) => Value::Float(*f),
                Some(Value::Int(i)) => Value::Float(i.to_f64()),
                _ => Value::Unit,
            }))
        }
        "idiv" => {
            // Integer division: idiv(7, 2) = 3 (truncates toward zero)
            if let (Some(Value::Int(a)), Some(Value::Int(b))) = (args.first(), args.get(1)) {
                // Int path keeps BigInts exact (val_to_i64 would turn any
                // value beyond i64 into 0).
                if b.to_i64() == Some(0) {
                    Some(Err(RuntimeError::TypeError("division by zero".to_string())))
                } else {
                    Some(Ok(Value::Int(a.clone().div(b.clone()))))
                }
            } else if args.len() >= 2 {
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
                    (Value::Int(v), Value::Int(lo), Value::Int(hi)) => {
                        // BigInt-exact (clamp(2^70, 10, 20) was 10: the value
                        // became 0 past i64)
                        let (v, lo, hi) = (v.to_rug(), lo.to_rug(), hi.to_rug());
                        if lo > hi {
                            return Some(Err(RuntimeError::TypeError(format!("clamp: min ({}) must be <= max ({})", lo, hi))));
                        }
                        let r = if v < lo { lo } else if v > hi { hi } else { v };
                        Some(Ok(Value::Int(SomaInt::from_rug(r))))
                    }
                    _ => Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: "clamp(value, min, max) takes numbers".to_string() })),
                }
            } else {
                Some(Err(RuntimeError::TypeError("clamp expects (value, min, max)".to_string())))
            }
        }
        // random() → float 0.0..1.0
        // random(max) → int 0..max (exclusive)
        // random(min, max) → int min..max (exclusive)
        "random" => {
            // splitmix64 seeded once from the clock: full 53-bit floats (it
            // gave 6 decimals: 0.969512) and the native backend's generator
            use std::cell::Cell;
            thread_local! { static RNG: Cell<u64> = Cell::new(0); }
            let z = RNG.with(|c| {
                let mut st = c.get();
                if st == 0 {
                    st = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64 | 1;
                }
                st = st.wrapping_add(0x9E3779B97F4A7C15);
                c.set(st);
                let mut z = st;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
                z ^ (z >> 31)
            });
            let below = |n: u64| -> u64 { ((z as u128 * n as u128) >> 64) as u64 };
            if args.is_empty() {
                Some(Ok(Value::Float((z >> 11) as f64 / (1u64 << 53) as f64)))
            } else if args.len() == 1 {
                let max = val_to_i64(&args[0]);
                if max <= 0 { return Some(Ok(Value::Int(SomaInt::from_i64(0)))); }
                Some(Ok(Value::Int(SomaInt::from_i64(below(max as u64) as i64))))
            } else {
                let min = val_to_i64(&args[0]);
                let max = val_to_i64(&args[1]);
                if max <= min { return Some(Ok(Value::Int(SomaInt::from_i64(min)))); }
                Some(Ok(Value::Int(SomaInt::from_i64(min + below((max - min) as u64) as i64))))
            }
        }
        // Bit operations on Int — arbitrary precision, like Python: a bit
        // array packed into one Int keeps growing past 63 bits, and the
        // native backend agrees (i64 fast path, BigInt fallback on overflow)
        "band" | "bor" | "bxor" if args.len() >= 2 => {
            let a = big_of(&args[0]);
            let b = big_of(&args[1]);
            let r = match name { "band" => a & b, "bor" => a | b, _ => a ^ b };
            Some(Ok(Value::Int(SomaInt::from_rug(r))))
        }
        "bnot" if args.len() >= 1 => {
            Some(Ok(Value::Int(SomaInt::from_rug(!big_of(&args[0])))))
        }
        "shl" if args.len() >= 2 => {
            let b = val_to_i64(&args[1]);
            if b < 0 || b > 1 << 24 {
                return Some(Err(RuntimeError::TypeError(format!("shl(): shift count {} out of range", b))));
            }
            // the RESULT is capped too: shl(x, 2^24) of an already large x
            // in a loop grew without bound
            let a = big_of(&args[0]);
            if a.significant_bits() as u64 + b as u64 > SomaInt::MAX_BITS {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range: shl() would build an Int of {} bits, past the limit of {}", a.significant_bits() as u64 + b as u64, SomaInt::MAX_BITS) }));
            }
            Some(Ok(Value::Int(SomaInt::from_rug(a << (b as u32)))))
        }
        "shr" if args.len() >= 2 => {
            let b = val_to_i64(&args[1]);
            if b < 0 {
                return Some(Err(RuntimeError::TypeError(format!("shr(): shift count {} out of range", b))));
            }
            // shifted past every bit: the sign fill (0 or -1), like native
            if b > 1 << 24 {
                let a = big_of(&args[0]);
                return Some(Ok(Value::Int(SomaInt::from_i64(if a < 0 { -1 } else { 0 }))));
            }
            Some(Ok(Value::Int(SomaInt::from_rug(big_of(&args[0]) >> (b as u32)))))
        }
        // bit operations on the arbitrary-precision Int, like the native
        // backend (bit_set(1, 64) wrapped to bit 0 through an i64 shift)
        "bit_test" | "bit_set" | "bit_clr" | "bit_next" if args.len() >= 2 => {
            let b = val_to_i64(&args[1]);
            if b < 0 || b > 1 << 24 {
                return Some(Err(RuntimeError::TypeError(format!("{}(): bit index {} out of range", name, b))));
            }
            let mut a = big_of(&args[0]);
            let i = b as u32;
            Some(Ok(Value::Int(match name {
                "bit_test" => SomaInt::from_i64(if a.get_bit(i) { 1 } else { 0 }),
                "bit_set" => { a.set_bit(i, true); SomaInt::from_rug(a) }
                "bit_clr" => { a.set_bit(i, false); SomaInt::from_rug(a) }
                _ => SomaInt::from_i64(a.find_one(i).map_or(-1, |x| x as i64)),
            })))
        }
        "bit_len" if args.len() >= 1 => {
            // exact, BigInt included (it estimated from the decimal length:
            // 26370 vs native 26373), of the magnitude like the native backend
            match &args[0] {
                Value::Int(_) => Some(Ok(Value::Int(SomaInt::from_i64(big_of(&args[0]).significant_bits() as i64)))),
                _ => Some(Ok(Value::Int(SomaInt::from_i64(0)))),
            }
        }
        // Number theory
        "gcd" if args.len() >= 2 => {
            // on the arbitrary-precision Ints (val_to_i64 truncated BigInts;
            // gcd(i64::MIN, 0) came back negative)
            let g = big_of(&args[0]).gcd(&big_of(&args[1]));
            Some(Ok(Value::Int(SomaInt::from_rug(g))))
        }
        "sqrt_int" if args.len() >= 1 => {
            // exact integer square root, BigInt included (a 20-digit input
            // used to answer 0 through i64 truncation)
            let a = match &args[0] {
                Value::Int(si) => si.to_rug(),
                other => rug::Integer::from(val_to_i64(other)),
            };
            if a < 0 {
                Some(Err(RuntimeError::TypeError("sqrt_int: negative argument".to_string())))
            } else {
                Some(Ok(Value::Int(SomaInt::from_rug(a.sqrt()))))
            }
        }
        "str_len" if args.len() >= 1 => {
            match &args[0] {
                Value::String(s) => Some(Ok(Value::Int(SomaInt::from_i64(s.len() as i64)))),
                _ => Some(Err(RuntimeError::TypeError("str_len expects a String".to_string()))),
            }
        }
        "str_at" if args.len() >= 2 => {
            match &args[0] {
                Value::String(s) => {
                    // kind `index` and the index as written, like [native]
                    let i = val_to_i64(&args[1]);
                    if i < 0 || i as usize >= s.len() {
                        Some(Err(RuntimeError::Domain { kind: "index".to_string(), message: format!("str_at: index {} out of range for a string of {} bytes", i, s.len()) }))
                    } else {
                        Some(Ok(Value::Int(SomaInt::from_i64(s.as_bytes()[i as usize] as i64))))
                    }
                }
                _ => Some(Err(RuntimeError::TypeError("str_at expects (String, Int)".to_string()))),
            }
        }
        "str_eq" if args.len() >= 2 => {
            match (&args[0], &args[1]) {
                (Value::String(a), Value::String(b)) => Some(Ok(Value::Bool(a == b))),
                _ => Some(Err(RuntimeError::TypeError("str_eq expects two Strings".to_string()))),
            }
        }
        "pow_mod" if args.len() >= 3 && args.iter().take(3).all(|a| matches!(a, Value::Int(_))) && args.iter().take(3).any(|a| matches!(a, Value::Int(i) if i.to_i64().is_none())) => {
            // BigInt operands: exact (they were read as 0)
            let g = |i: usize| match &args[i] { Value::Int(x) => x.to_rug(), _ => rug::Integer::new() };
            let (b, e, m) = (g(0), g(1), g(2));
            if m == 0 { return Some(Err(RuntimeError::TypeError("pow_mod: modulus is zero".to_string()))); }
            if e < 0 { return Some(Err(RuntimeError::TypeError("pow_mod: negative exponent (a modular inverse is not computed)".to_string()))); }
            // bits(exp) squarings of bits(m)-sized numbers: both large froze
            // the service (a 5-byte body, 8 s; at the size cap, never ending)
            let work = e.significant_bits() as u64 * m.significant_bits() as u64;
            if work > 1 << 30 {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range: pow_mod with a {}-bit exponent and a {}-bit modulus is past the work limit (bits(exp) × bits(m) ≤ 2^30)", e.significant_bits(), m.significant_bits()) }));
            }
            let m_abs = rug::Integer::from(m.abs_ref());
            match b.pow_mod(&e, &m_abs) {
                Ok(r) => Some(Ok(Value::Int(SomaInt::from_rug(r)))),
                Err(_) => Some(Err(RuntimeError::TypeError("pow_mod: no result".to_string()))),
            }
        }
        "pow_mod" if args.len() >= 3 => {
            let base = val_to_i64(&args[0]) as i128;
            let exp = val_to_i64(&args[1]);
            let m = val_to_i64(&args[2]) as i128;
            if m == 0 {
                return Some(Err(RuntimeError::TypeError("pow_mod: modulus is zero".to_string())));
            }
            if exp < 0 {
                // it answered 1 (the loop never ran); pow_mod(3, -1, 7) is an inverse, not 1
                return Some(Err(RuntimeError::TypeError("pow_mod: negative exponent (a modular inverse is not computed)".to_string())));
            }
            let mut r: i128 = 1i128.rem_euclid(m);
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

/// Reduce a single list of numbers (or the variadic numeric args) with
/// sum / product / min / max / avg. Stays Int-exact (BigInt-safe via
/// SomaInt) when every element is an Int; promotes to Float if any
/// median / variance / stddev of a list (or variadic numbers). Population
/// variance (divide by n) like statistics.pvariance / pstdev; `median`
/// answers an Int when the list is Ints and the middle is exact, the `/`
/// rule.
fn stats_reduce(args: &[Value], op: &str) -> Result<Value, RuntimeError> {
    let items: Vec<Value> = match args.first() {
        Some(Value::List(xs)) if args.len() == 1 => xs.clone(),
        _ => args.to_vec(),
    };
    if let Some(bad) = items.iter().find(|v| !matches!(v, Value::Int(_) | Value::Float(_))) {
        return Err(RuntimeError::TypeError(format!(
            "{}() needs numbers, found {} {}", op, super::super::value_type_name(bad), bad
        )));
    }
    if items.is_empty() {
        return Err(RuntimeError::Domain { kind: "empty".to_string(), message: format!("empty: {}() of no values", op) });
    }
    let all_int = items.iter().all(|v| matches!(v, Value::Int(_)));
    let mut nums: Vec<f64> = items.iter().map(|v| match v {
        Value::Float(f) => *f,
        Value::Int(si) => si.to_f64(),
        _ => 0.0,
    }).collect();
    let n = nums.len() as f64;
    let as_value = |x: f64| -> Value {
        if all_int && x.fract() == 0.0 && x.abs() < 9.0e15 { Value::Int(SomaInt::from_i64(x as i64)) } else { Value::Float(x) }
    };
    match op {
        "median" if all_int => {
            // exact: sort the Ints themselves, the middle pair by the `/` rule
            let mut xs: Vec<rug::Integer> = items.iter().map(|v| match v { Value::Int(i) => i.to_rug(), _ => rug::Integer::new() }).collect();
            xs.sort();
            let m = xs.len() / 2;
            if xs.len() % 2 == 1 {
                Ok(Value::Int(SomaInt::from_rug(xs[m].clone())))
            } else {
                let sum = SomaInt::from_rug(rug::Integer::from(&xs[m - 1] + &xs[m]));
                crate::interpreter::int_div_value(&sum, &SomaInt::from_i64(2))
            }
        }
        "median" => {
            nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let m = nums.len() / 2;
            let med = if nums.len() % 2 == 1 { nums[m] } else { (nums[m - 1] + nums[m]) / 2.0 };
            Ok(as_value(med))
        }
        _ => {
            // Python's statistics: stdev / variance are SAMPLE (n - 1),
            // pstdev / pvariance population (n); stddev stays population
            let sample = matches!(op, "stdev" | "variance");
            if sample && nums.len() < 2 {
                return Err(RuntimeError::Domain { kind: "empty".to_string(), message: format!("{}() needs at least two values (sample statistics); use p{} for the population form", op, op) });
            }
            let mean = nums.iter().sum::<f64>() / n;
            let denom = if sample { n - 1.0 } else { n };
            let var = nums.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / denom;
            match op {
                "variance" | "pvariance" => Ok(Value::Float(var)),
                _ => Ok(Value::Float(var.sqrt())),
            }
        }
    }
}

/// element is a Float.
fn numeric_reduce(args: &[Value], op: &str) -> Result<Value, RuntimeError> {
    // accept either a single List arg or variadic numbers
    let items: Vec<Value> = match args.first() {
        Some(Value::List(xs)) if args.len() == 1 => xs.clone(),
        _ => args.to_vec(),
    };
    if items.is_empty() {
        return match op {
            "product" => Ok(Value::Int(SomaInt::from_i64(1))),
            "min" | "max" | "avg" => Ok(Value::Unit),
            _ => Ok(Value::Int(SomaInt::from_i64(0))),
        };
    }
    // a non-number in the list is an error, not a silent 0
    if let Some(bad) = items.iter().find(|v| !matches!(v, Value::Int(_) | Value::Float(_))) {
        return Err(RuntimeError::TypeError(format!(
            "{}() needs numbers, found {} {}", op, super::super::value_type_name(bad), bad
        )));
    }
    let any_float = items.iter().any(|v| matches!(v, Value::Float(_)));
    let n = items.len() as i64;
    if any_float {
        let nums: Vec<f64> = items.iter().map(|v| match v {
            Value::Float(f) => *f,
            Value::Int(si) => si.to_f64(),
            _ => 0.0,
        }).collect();
        let r = match op {
            "sum" => nums.iter().sum(),
            "product" => nums.iter().product(),
            "avg" => nums.iter().sum::<f64>() / n as f64,
            "min" => nums.iter().cloned().fold(f64::INFINITY, f64::min),
            "max" => nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            _ => 0.0,
        };
        Ok(Value::Float(r))
    } else {
        let ints: Vec<SomaInt> = items.iter().map(|v| match v {
            Value::Int(si) => si.clone(),
            _ => SomaInt::from_i64(val_to_i64(v)),
        }).collect();
        match op {
            "sum" => {
                let mut acc = SomaInt::from_i64(0);
                for x in ints { acc = acc.add(x); }
                Ok(Value::Int(acc))
            }
            "product" => {
                let mut acc = SomaInt::from_i64(1);
                for x in ints {
                    acc = acc.checked_big_mul(x).map_err(|m| RuntimeError::Domain { kind: "range".to_string(), message: m })?;
                }
                Ok(Value::Int(acc))
            }
            "avg" => {
                let mut acc = SomaInt::from_i64(0);
                for x in ints.iter() { acc = acc.add(x.clone()); }
                // same rule as `/`: avg([1, 2]) is 1.5, an exact average
                // stays an Int (it used to truncate to 1)
                let count = SomaInt::from_i64(n);
                if acc.clone().modulo(count.clone()).to_i64() == Some(0) {
                    Ok(Value::Int(acc.div(count)))
                } else {
                    Ok(Value::Float(acc.to_f64() / n as f64))
                }
            }
            "min" => Ok(Value::Int(ints.into_iter().reduce(|a, b| if a.cmp(&b) <= 0 { a } else { b }).unwrap())),
            "max" => Ok(Value::Int(ints.into_iter().reduce(|a, b| if a.cmp(&b) >= 0 { a } else { b }).unwrap())),
            _ => Ok(Value::Unit),
        }
    }
}

/// The exact integer of a value (BigInt included; Floats truncate).
fn big_of(v: &Value) -> rug::Integer {
    match v {
        Value::Int(si) => si.to_rug(),
        other => rug::Integer::from(val_to_i64(other)),
    }
}

/// Round half away from zero on the DECIMAL representation (the shortest
/// round-trip text), like Ruby and Python's decimal-aware `round`:
/// round(1.005, 2) is 1.01, not 1.0.
pub fn round_decimal(x: f64, digits: i32) -> f64 {
    if !x.is_finite() { return x; }
    let text = format!("{}", x.abs());
    let (int_part, frac) = match text.split_once('.') { Some((i, f)) => (i.to_string(), f.to_string()), None => (text.clone(), String::new()) };
    if text.contains('e') { let p = 10f64.powi(digits); return (x * p).round() / p; }
    let d = digits as usize;
    if frac.len() <= d { return x; }
    let mut digits_all: Vec<u8> = int_part.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    let keep = int_part.len() + d;
    let round_up = digits_all[keep] >= 5;
    digits_all.truncate(keep);
    if round_up {
        let mut i = keep;
        loop {
            if i == 0 { digits_all.insert(0, 1); break; }
            i -= 1;
            if digits_all[i] == 9 { digits_all[i] = 0; } else { digits_all[i] += 1; break; }
        }
    }
    let int_len = digits_all.len() - d;
    let s: String = digits_all.iter().map(|b| (b + b'0') as char).collect();
    let (ip, fp) = s.split_at(int_len);
    let rebuilt = if fp.is_empty() { ip.to_string() } else { format!("{}.{}", ip, fp) };
    let v: f64 = rebuilt.parse().unwrap_or(x.abs());
    if x < 0.0 { -v } else { v }
}

/// `x` with exactly `digits` decimals, rounded half away from zero on the
/// decimal text (printf's %.Nf, minus its banker's quirks).
pub fn fixed_string(x: f64, digits: usize) -> String {
    let r = round_decimal(x, digits as i32);
    let t = format!("{:.*}", digits, r);
    if t.starts_with("-0") && t.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') { t[1..].to_string() } else { t }
}
