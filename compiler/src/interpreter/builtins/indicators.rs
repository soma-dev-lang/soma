use super::super::{Value, RuntimeError, map_from_pairs};
use super::{val_to_i64, map_field_i64};

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "sma" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let period = val_to_i64(&args[2]) as usize;
                    let vals: Vec<f64> = items.iter().map(|i| map_field_i64(i, &field) as f64).collect();
                    let start = if vals.len() > period { vals.len() - period } else { 0 };
                    let window = &vals[start..];
                    let avg = if window.is_empty() { 0.0 } else { window.iter().sum::<f64>() / window.len() as f64 };
                    Some(Ok(Value::Float(avg)))
                } else { Some(Ok(Value::Float(0.0))) }
            } else { Some(Err(RuntimeError::TypeError("sma(list, field, period)".to_string()))) }
        }
        "ema" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let period = val_to_i64(&args[2]) as usize;
                    let vals: Vec<f64> = items.iter().map(|i| map_field_i64(i, &field) as f64).collect();
                    if vals.is_empty() || period == 0 { return Some(Ok(Value::Float(0.0))); }
                    let k = 2.0 / (period as f64 + 1.0);
                    let mut ema = vals[0];
                    for v in &vals[1..] { ema = v * k + ema * (1.0 - k); }
                    Some(Ok(Value::Float(ema)))
                } else { Some(Ok(Value::Float(0.0))) }
            } else { Some(Err(RuntimeError::TypeError("ema(list, field, period)".to_string()))) }
        }
        "rsi" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let period = val_to_i64(&args[2]) as usize;
                    let vals: Vec<f64> = items.iter().map(|i| map_field_i64(i, &field) as f64).collect();
                    if vals.len() < 2 || period == 0 { return Some(Ok(Value::Float(50.0))); }
                    let mut gains = 0.0f64;
                    let mut losses = 0.0f64;
                    let start = if vals.len() > period + 1 { vals.len() - period - 1 } else { 0 };
                    for i in (start + 1)..vals.len() {
                        let diff = vals[i] - vals[i - 1];
                        if diff > 0.0 { gains += diff; } else { losses += -diff; }
                    }
                    let count = (vals.len() - start - 1) as f64;
                    let avg_gain = gains / count;
                    let avg_loss = losses / count;
                    let rsi = if avg_loss == 0.0 { 100.0 } else { 100.0 - 100.0 / (1.0 + avg_gain / avg_loss) };
                    Some(Ok(Value::Float(rsi)))
                } else { Some(Ok(Value::Float(50.0))) }
            } else { Some(Err(RuntimeError::TypeError("rsi(list, field, period)".to_string()))) }
        }
        "macd" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let vals: Vec<f64> = items.iter().map(|i| map_field_i64(i, &field) as f64).collect();
                    if vals.len() < 26 { return Some(Ok(map_from_pairs(vec![("macd".into(), Value::Float(0.0)), ("signal".into(), Value::Float(0.0)), ("histogram".into(), Value::Float(0.0))]))); }
                    let k12 = 2.0 / 13.0;
                    let mut ema12 = vals[0];
                    for v in &vals[1..] { ema12 = v * k12 + ema12 * (1.0 - k12); }
                    let k26 = 2.0 / 27.0;
                    let mut ema26 = vals[0];
                    for v in &vals[1..] { ema26 = v * k26 + ema26 * (1.0 - k26); }
                    let macd_val = ema12 - ema26;
                    let signal = macd_val * 0.8;
                    let histogram = macd_val - signal;
                    Some(Ok(map_from_pairs(vec![
                        ("macd".to_string(), Value::Float(macd_val)),
                        ("signal".to_string(), Value::Float(signal)),
                        ("histogram".to_string(), Value::Float(histogram)),
                    ])))
                } else { Some(Ok(Value::Float(0.0))) }
            } else { Some(Err(RuntimeError::TypeError("macd(list, field)".to_string()))) }
        }
        "bollinger" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let period = val_to_i64(&args[2]) as usize;
                    let vals: Vec<f64> = items.iter().map(|i| map_field_i64(i, &field) as f64).collect();
                    let start = if vals.len() > period { vals.len() - period } else { 0 };
                    let window = &vals[start..];
                    let mean = if window.is_empty() { 0.0 } else { window.iter().sum::<f64>() / window.len() as f64 };
                    let variance = if window.is_empty() { 0.0 } else { window.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / window.len() as f64 };
                    let std_dev = variance.sqrt();
                    Some(Ok(map_from_pairs(vec![
                        ("upper".to_string(), Value::Float(mean + 2.0 * std_dev)),
                        ("middle".to_string(), Value::Float(mean)),
                        ("lower".to_string(), Value::Float(mean - 2.0 * std_dev)),
                        ("std_dev".to_string(), Value::Float(std_dev)),
                    ])))
                } else { Some(Ok(Value::Float(0.0))) }
            } else { Some(Err(RuntimeError::TypeError("bollinger(list, field, period)".to_string()))) }
        }
        _ => None,
    }
}
