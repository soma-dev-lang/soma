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
            let [Value::Int(ms)] = args else {
                return Some(Err(RuntimeError::TypeError("sleep(ms: Int) requires one Int duration".to_string())));
            };
            let Some(ms) = ms.to_i64().filter(|ms| (0..=86_400_000).contains(ms)) else {
                return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: "sleep(): the duration is 0 to 86400000 ms (one day)".to_string() }));
            };
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            Some(Ok(Value::Unit))
        }
        "today" => {
            Some(Ok(Value::String(format_unix_date(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            ))))
        }
        // ── dates as "YYYY-MM-DD" strings ────────────────────────────
        "parse_date" => {
            let Some(v) = args.first() else { return Some(Err(RuntimeError::TypeError("parse_date(s: String) -> Map".to_string()))) };
            Some(date_arg(v, "parse_date").map(|(y, m, d)| {
                let days = days_from_civil(y, m, d);
                // 1970-01-01 was a Thursday (4): 1 = Monday … 7 = Sunday (ISO 8601)
                let weekday = (days + 3).rem_euclid(7) + 1;
                crate::interpreter::map_from_pairs(vec![
                    ("year".to_string(), Value::Int(SomaInt::from_i64(y))),
                    ("month".to_string(), Value::Int(SomaInt::from_i64(m))),
                    ("day".to_string(), Value::Int(SomaInt::from_i64(d))),
                    ("weekday".to_string(), Value::Int(SomaInt::from_i64(weekday))),
                    ("epoch_day".to_string(), Value::Int(SomaInt::from_i64(days))),
                ])
            }))
        }
        "add_days" if args.len() == 2 => {
            let n = match date_count(&args[1], "add_days", "a number of days") { Ok(n) => n, Err(e) => return Some(Err(e)) };
            Some(date_arg(&args[0], "add_days").and_then(|(y, m, d)| {
                // a date outside 0000-01-01..9999-12-31 is not a date parse_date
                // reads back (an i64 day count wrapped to year -25252734927764529)
                let days = days_from_civil(y, m, d).checked_add(n).filter(|x| x.unsigned_abs() < 4_000_000);
                let (y2, m2, d2) = match days { Some(x) => civil_from_days(x), None => (-1, 1, 1) };
                if !(0..=9999).contains(&y2) {
                    return Err(RuntimeError::Domain { kind: "date".to_string(), message: format!("date: add_days goes outside years 0000–9999 ({} days)", n) });
                }
                Ok(Value::String(iso(y2, m2, d2)))
            }))
        }
        "add_months" if args.len() == 2 => {
            // Ruby's `Date >> n`: same day, clamped to the month's length
            let n = match date_count(&args[1], "add_months", "a number of months") { Ok(n) => n, Err(e) => return Some(Err(e)) };
            Some(date_arg(&args[0], "add_months").and_then(|(y, m, d)| {
                let idx = y * 12 + (m - 1) + n;
                let (y2, m2) = (idx.div_euclid(12), idx.rem_euclid(12) + 1);
                if !(0..=9999).contains(&y2) { return Err(out_of_years("add_months")); }
                Ok(Value::String(iso(y2, m2, d.min(days_in_month(y2, m2)))))
            }))
        }
        "days_between" if args.len() == 2 => {
            Some(date_arg(&args[0], "days_between").and_then(|a| date_arg(&args[1], "days_between").map(|b| {
                Value::Int(SomaInt::from_i64(days_from_civil(b.0, b.1, b.2) - days_from_civil(a.0, a.1, a.2)))
            })))
        }
        // java.time ChronoUnit.MONTHS.between / dateutil relativedelta:
        // whole months from a to b (negative when b < a), day-of-month aware
        "months_between" if args.len() == 2 => {
            Some(date_arg(&args[0], "months_between").and_then(|a| date_arg(&args[1], "months_between").map(|b| {
                let mut m = (b.0 - a.0) * 12 + (b.1 - a.1);
                if m > 0 && b.2 < a.2 { m -= 1; }
                if m < 0 && b.2 > a.2 { m += 1; }
                Value::Int(SomaInt::from_i64(m))
            })))
        }
        "days_in_month" if args.len() == 2 => {
            if args.iter().any(|a| !matches!(a, Value::Int(_))) {
                return Some(Err(RuntimeError::TypeError("days_in_month(year: Int, month: Int) requires Int arguments".into())));
            }
            let m = val_to_i64(&args[1]);
            if !(1..=12).contains(&m) {
                return Some(Err(RuntimeError::Domain { kind: "date".to_string(), message: format!("date: days_in_month: month {} is not 1..12", m) }));
            }
            Some(Ok(Value::Int(SomaInt::from_i64(days_in_month(val_to_i64(&args[0]), m)))))
        }
        "format_date" => {
            if let Some(ts) = args.first() {
                Some(date_arg(ts, "format_date").map(|(y, m, d)| Value::String(iso(y, m, d))))
            } else {
                Some(Ok(Value::String("".to_string())))
            }
        }
        _ => None,
    }
}

pub fn format_unix_date(secs: i64) -> String {
    // floor division: -86400 is 1969-12-31, not "1970-01-00"
    let days = secs.div_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Howard Hinnant's days_from_civil / civil_from_days (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 29 } else { 28 },
        _ => 0,
    }
}

/// "YYYY-MM-DD" → (y, m, d), strictly.
pub fn parse_iso_date(s: &str) -> Option<(i64, i64, i64)> {
    // no surrounding spaces either: " 2026-01-01" is not an ISO date
    let parts: Vec<&str> = s.split('-').collect();
    // strict YYYY-MM-DD: "2026-3-1" is not accepted
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2
        || !parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) { return None; }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) { return None; }
    Some((y, m, d))
}

/// An Int count for a date builtin: a BigInt wrapped to 0 and a Float was
/// truncated (add_days(d, 2^64 + 1) answered d itself).
fn date_count(v: &Value, what: &str, is: &str) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(si) => si.to_i64().filter(|n| n.unsigned_abs() < 1_000_000_000_000_000).ok_or_else(|| out_of_years(what)),
        other => Err(RuntimeError::TypeError(format!("{}: expected an Int ({}), got {} {}", what, is, crate::interpreter::value_type_name(other), other))),
    }
}

fn out_of_years(what: &str) -> RuntimeError {
    RuntimeError::Domain { kind: "date".to_string(), message: format!("date: {} goes outside years 0000–9999", what) }
}

fn iso(y: i64, m: i64, d: i64) -> String { format!("{:04}-{:02}-{:02}", y, m, d) }

fn date_arg(v: &Value, what: &str) -> Result<(i64, i64, i64), RuntimeError> {
    match v {
        Value::String(s) => parse_iso_date(s).ok_or_else(|| RuntimeError::Domain {
            kind: "date".to_string(), message: format!("date: {} is not a YYYY-MM-DD date: {}", what, s),
        }),
        Value::Int(_) => {
            let secs = date_count(v, what, "a Unix time in seconds")?;
            let (y, m, d) = civil_from_days(secs.div_euclid(86400));
            if !(0..=9999).contains(&y) { return Err(out_of_years(what)); }
            Ok((y, m, d))
        }
        other => Err(RuntimeError::TypeError(format!("{}: expected a \"YYYY-MM-DD\" String, got {}", what, other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimum_integer_date_count_is_a_domain_error_without_overflow() {
        // abs(i64::MIN) panics with overflow checks and remains negative without them.
        let value = Value::Int(SomaInt::from_i64(i64::MIN));
        assert!(matches!(date_count(&value, "add_days", "days"), Err(RuntimeError::Domain { kind, .. }) if kind == "date"));
    }
}
