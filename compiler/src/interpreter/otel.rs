//! OpenTelemetry export (3.0, spec C7). With `SOMA_OTEL_FILE=spans.jsonl`,
//! every top-level handler invocation appends one OTLP/JSON line (the
//! format the collector's otlpjsonfile receiver reads): a span for the
//! invocation, and one span event per step the handler traced — model
//! calls (GenAI semantic conventions: `gen_ai.operation.name`,
//! `gen_ai.usage.total_tokens`), tool calls, approvals, transitions.
//!
//! Content is not exported by default: prompts, tool arguments and results
//! appear as their sizes. `SOMA_OTEL_CONTENT=1` exports the text.

use super::{Value, VariantValue};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn now_ns() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// An id of `bytes` bytes, hex: unique per process and invocation.
fn id(bytes: usize, seed: u128) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut x = (seed as u64) ^ ((std::process::id() as u64) << 32) ^ n.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03;
    let mut out = String::with_capacity(bytes * 2);
    for _ in 0..bytes {
        // xorshift64*
        x ^= x >> 12; x ^= x << 25; x ^= x >> 27;
        out.push_str(&format!("{:02x}", (x.wrapping_mul(0x2545F4914F6CDD1D) >> 56) as u8));
    }
    out
}

fn attr(key: &str, value: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"key": key, "value": value})
}
fn s(v: &str) -> serde_json::Value { serde_json::json!({"stringValue": v}) }
fn i(v: i64) -> serde_json::Value { serde_json::json!({"intValue": v.to_string()}) }

/// One traced step as a span event.
fn event(step: &Value, content: bool, fallback_ns: u128) -> Option<serde_json::Value> {
    let Value::Variant { variant, fields: VariantValue::Struct(f), .. } = step else { return None };
    let text = |k: &str| match f.get(k) { Some(Value::String(t)) => t.clone(), Some(other) => other.to_string(), None => String::new() };
    let int = |k: &str| match f.get(k) { Some(Value::Int(n)) => n.to_i64().unwrap_or(0), _ => 0 };
    // steps carry whole seconds: keep the event inside its span
    let at = match int("timestamp") { 0 => fallback_ns, secs => ((secs as u128) * 1_000_000_000).max(fallback_ns) };
    let mut attrs = Vec::new();
    let body = |attrs: &mut Vec<serde_json::Value>, key: &str, value: String| {
        if content { attrs.push(attr(key, s(&value))); } else { attrs.push(attr(&format!("{key}.chars"), i(value.chars().count() as i64))); }
    };
    let name = match variant.as_str() {
        "Think" => {
            attrs.push(attr("gen_ai.operation.name", s("chat")));
            attrs.push(attr("gen_ai.usage.total_tokens", i(int("tokens"))));
            attrs.push(attr("gen_ai.response.finish_reasons", serde_json::json!({"arrayValue": {"values": [s(&text("finish_reason"))]}})));
            attrs.push(attr("soma.think.round", i(int("iteration"))));
            attrs.push(attr("soma.tokens.total", i(int("total_tokens"))));
            body(&mut attrs, "gen_ai.prompt", text("prompt"));
            "gen_ai.chat"
        }
        "ToolCall" => {
            attrs.push(attr("gen_ai.operation.name", s("execute_tool")));
            attrs.push(attr("gen_ai.tool.name", s(&text("name"))));
            body(&mut attrs, "gen_ai.tool.call.arguments", text("args"));
            body(&mut attrs, "gen_ai.tool.call.result", text("result"));
            "gen_ai.execute_tool"
        }
        "Approval" => {
            attrs.push(attr("soma.approval.result", s(&text("result"))));
            body(&mut attrs, "soma.approval.action", text("action"));
            "soma.approval"
        }
        "Transition" => {
            attrs.push(attr("soma.transition.from", s(&text("from"))));
            attrs.push(attr("soma.transition.to", s(&text("to"))));
            body(&mut attrs, "soma.transition.instance", text("instance"));
            "soma.transition"
        }
        other => {
            attrs.push(attr("soma.step", s(other)));
            "soma.step"
        }
    };
    Some(serde_json::json!({"name": name, "timeUnixNano": at.to_string(), "attributes": attrs}))
}

/// Append the span of one top-level invocation. A failure to write is
/// reported once and never fails the handler.
pub fn export(path: &str, cell: &str, handler: &str, start_ns: u128, end_ns: u128, error_kind: Option<&str>, steps: &[Value]) {
    let content = std::env::var("SOMA_OTEL_CONTENT").map_or(false, |v| v == "1" || v == "true");
    let mut attrs = vec![
        attr("soma.cell", s(cell)),
        attr("soma.handler", s(handler)),
        attr("code.function.name", s(&format!("{cell}.{handler}"))),
    ];
    let status = match error_kind {
        Some(kind) => {
            attrs.push(attr("error.type", s(kind)));
            serde_json::json!({"code": 2, "message": kind})
        }
        None => serde_json::json!({"code": 1}),
    };
    let events: Vec<serde_json::Value> = steps.iter().filter_map(|st| event(st, content, start_ns)).collect();
    let line = serde_json::json!({"resourceSpans": [{
        "resource": {"attributes": [
            attr("service.name", s(&std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "soma".to_string()))),
            attr("telemetry.sdk.name", s("soma")),
            attr("telemetry.sdk.version", s(env!("CARGO_PKG_VERSION"))),
        ]},
        "scopeSpans": [{
            "scope": {"name": "soma", "version": env!("CARGO_PKG_VERSION")},
            "spans": [{
                "traceId": id(16, start_ns),
                "spanId": id(8, end_ns),
                "name": format!("{cell}.{handler}"),
                "kind": 2,
                "startTimeUnixNano": start_ns.to_string(),
                "endTimeUnixNano": end_ns.max(start_ns).to_string(),
                "attributes": attrs,
                "events": events,
                "status": status,
            }],
        }],
    }]});
    let written = std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| writeln!(f, "{}", line));
    if let Err(e) = written {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| eprintln!("warning: SOMA_OTEL_FILE: cannot write '{}': {} — telemetry is off for this process", path, e));
    }
}
