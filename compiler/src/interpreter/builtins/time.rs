use super::super::{Value, RuntimeError};
use super::val_to_i64;
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "now" => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(Ok(Value::Int(SomaInt::from_i64(ts))))
        }
        "now_ms" => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            Some(Ok(Value::Int(SomaInt::from_i64(ts))))
        }
        "sleep" => {
            if let Some(ms) = args.first().map(|a| val_to_i64(a)) {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                Some(Ok(Value::Unit))
            } else {
                Some(Ok(Value::Unit))
            }
        }
        "today" => {
            Some(Ok(Value::String(format_unix_date(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            ))))
        }
        "format_date" => {
            if let Some(ts) = args.first() {
                let secs = val_to_i64(ts);
                Some(Ok(Value::String(format_unix_date(secs))))
            } else {
                Some(Ok(Value::String("".to_string())))
            }
        }
        _ => None,
    }
}

fn format_unix_date(secs: i64) -> String {
    let days = secs / 86400;
    let mut y = 1970i64;
    let mut remaining_days = days;
    loop {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year: i64 = if leap { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let months: [i64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1;
    for month_days in &months {
        if remaining_days < *month_days {
            break;
        }
        remaining_days -= *month_days;
        m += 1;
    }
    let d = remaining_days + 1;
    format!("{:04}-{:02}-{:02}", y, m, d)
}
