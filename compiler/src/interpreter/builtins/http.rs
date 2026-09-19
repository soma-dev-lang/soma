thread_local! {
    /// set while a capability-scoped tool runs (see builtins/mod.rs)
    pub(crate) static NO_REDIRECTS: std::cell::Cell<bool> = std::cell::Cell::new(false);
}

use super::super::{Value, RuntimeError, map_from_pairs};
use super::serde_json_to_value;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "http_get" => {
            let Some(Value::String(url)) = args.first() else {
                return Some(Err(RuntimeError::TypeError("http_get(url: String, opts?: map(\"timeout\", ms, \"max_bytes\", n, \"headers\", map(...)))".to_string())));
            };
            let opts = match args.get(1) {
                None | Some(Value::Unit) => None,
                Some(Value::Map(m)) => Some(m),
                Some(other) => return Some(Err(RuntimeError::TypeError(format!("http_get(url, opts): opts must be a map(\"timeout\", ms, …), got {}", super::super::value_type_name(other))))),
            };
            Some(http_call("GET", url, None, opts))
        }
        "http_post" | "http_put" | "http_patch" | "http_delete" => {
            let method = match name { "http_post" => "POST", "http_put" => "PUT", "http_patch" => "PATCH", _ => "DELETE" };
            let Some(Value::String(url)) = args.first() else {
                return Some(Err(RuntimeError::TypeError(format!("{}(url: String, body, opts?)", name))));
            };
            if method != "DELETE" && args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(format!("{}(url, body, opts?) — the body is required (map() for none)", name))));
            }
            let (body, opts_arg) = if method == "DELETE" { (args.get(1).filter(|v| !matches!(v, Value::Map(_))), args.get(1).filter(|v| matches!(v, Value::Map(_)))) } else { (args.get(1), args.get(2)) };
            let opts = match opts_arg {
                None | Some(Value::Unit) => None,
                Some(Value::Map(m)) => Some(m),
                Some(other) => return Some(Err(RuntimeError::TypeError(format!("{}(url, body, opts): opts must be a map(\"timeout\", ms, …), got {}", name, super::super::value_type_name(other))))),
            };
            let text = body.map(|b| match b {
                Value::String(s) => s.clone(),
                other => super::string::to_json_string(other),
            });
            Some(http_call(method, url, text, opts))
        }
        "ws_connect" => {
            // ws_connect(url) — open a WS connection, return connection ID
            // For now, synchronous: connect, return a map with the connection
            // The real async handling happens via the event bus
            if let Some(Value::String(url)) = args.first() {
                match tungstenite::connect(url) {
                    Ok((mut ws, _response)) => {
                        // Read messages in a background thread, push to stdout for now
                        // In a full implementation, this would feed into the event bus
                        // For now, return success
                        let _ = ws.close(None);
                        Some(Ok(map_from_pairs(vec![
                            ("status".to_string(), Value::String("connected".to_string())),
                            ("url".to_string(), Value::String(url.clone())),
                        ])))
                    }
                    Err(e) => Some(Ok(map_from_pairs(vec![
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ])))
                }
            } else {
                Some(Err(RuntimeError::TypeError("ws_connect(url)".to_string())))
            }
        }
        _ => None,
    }
}

/// One outbound request. Never raises for the network or a status: the
/// result is the parsed body on 2xx, else a map
/// `{error, kind, status, body}` — kind `http_status` (with the status and
/// the upstream body parsed when it is JSON), `timeout`, `refused` or
/// `network`. The default timeout is 30 s (a hung upstream used to hang the
/// caller forever — and under `soma serve` every other request with it).
/// The ports `soma serve` listens on (HTTP, WebSocket, bus): a handler
/// calling its own server waits for the handler lock it holds.
pub static OWN_PORTS: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();

fn calls_own_server(url: &str) -> bool {
    let Some(ports) = OWN_PORTS.get() else { return false };
    let rest = url.split("://").nth(1).unwrap_or("");
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let auth = auth.rsplit('@').next().unwrap_or(auth);
    let (host, port) = if auth.starts_with('[') {
        let h = auth.split(']').next().unwrap_or("").trim_start_matches('[');
        (h.to_string(), auth.split("]:").nth(1).and_then(|p| p.parse::<u16>().ok()))
    } else {
        let mut it = auth.splitn(2, ':');
        (it.next().unwrap_or("").to_string(), it.next().and_then(|p| p.parse::<u16>().ok()))
    };
    let port = port.unwrap_or(if url.starts_with("https") { 443 } else { 80 });
    if !ports.contains(&port) { return false; }
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.starts_with("127.") || host == "::1" || host == "0.0.0.0" { return true; }
    // any other spelling of this machine (2130706433, 0x7f000001, 0177.0.0.1,
    // [0:0:…:1], a DNS name for 127.0.0.1): resolve it as the request would
    use std::net::ToSocketAddrs;
    (host.as_str(), port).to_socket_addrs().map_or(false, |mut addrs| addrs.any(|a| {
        // [::ffff:127.0.0.1] is 127.0.0.1 (it reached this server and
        // waited on its own handler lock)
        let ip = match a.ip() { std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().map(std::net::IpAddr::V4).unwrap_or(std::net::IpAddr::V6(v6)), other => other };
        ip.is_loopback() || ip.is_unspecified()
    }))
}

fn http_call(method: &str, url: &str, body: Option<String>, opts: Option<&indexmap::IndexMap<String, Value>>) -> Result<Value, RuntimeError> {
    // a request to this very server from inside a handler waits for the
    // handler lock the caller holds — it timed out, freezing every client
    if calls_own_server(url) {
        return Ok(crate::interpreter::map_from_pairs(vec![
            ("error".to_string(), Value::String(format!("self_call: {} is this server — a handler cannot call its own endpoints over HTTP (the handler lock is held); call the handler directly", url))),
            ("kind".to_string(), Value::String("self_call".to_string())),
            ("status".to_string(), Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(0))),
            ("body".to_string(), Value::Unit),
        ]));
    }
    if let Some(m) = opts {
        for (k, v) in m.iter() {
            let ok = match (k.as_str(), v) {
                ("timeout" | "max_bytes", Value::Int(n)) => n.to_i64().is_some_and(|x| x > 0),
                ("headers", Value::Map(h)) => h.values().all(|x| matches!(x, Value::String(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_))),
                ("timeout" | "max_bytes" | "headers", _) => false,
                _ => return Err(RuntimeError::TypeError(format!("http: unknown option '{}' — the options are timeout (ms), max_bytes, headers", k))),
            };
            if !ok {
                return Err(RuntimeError::TypeError(format!(
                    "http: option '{}' must be {}, got {} {}", k,
                    if k == "headers" { "a map of header name → text" } else { "a positive Int" },
                    super::super::value_type_name(v), v)));
            }
        }
    }
    let int_opt = |k: &str| opts.and_then(|m| m.get(k)).and_then(|v| match v {
        Value::Int(n) => n.to_i64().map(|v| v as u64),
        _ => None,
    });
    let timeout_ms = int_opt("timeout").unwrap_or(30_000);
    let max_bytes = int_opt("max_bytes").map(|v| v as usize);
    // inside a capability-scoped tool a redirect is NOT followed: a 302 to
    // another host escaped the tool's URL capability
    // ureq counts the final request too: 6 lets 5 redirect hops through
    // (the documented limit; the 5th hop failed as a network error)
    let redirects = if NO_REDIRECTS.with(|c| c.get()) { 0 } else { 6 };
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_millis(timeout_ms)).redirects(redirects).build();
    let mut req = agent.request(method, url);
    if let Some(b) = &body {
        // a Map/List body was serialized to JSON; a String is sent as is
        let is_json = serde_json::from_str::<serde_json::Value>(b.trim()).is_ok();
        req = req.set("Content-Type", if is_json { "application/json" } else { "text/plain; charset=utf-8" });
    }
    if let Some(Value::Map(h)) = opts.and_then(|m| m.get("headers")) {
        for (k, v) in h {
            let v = match v { Value::String(s) => s.clone(), other => format!("{}", other) };
            req = req.set(k, &v);
        }
    }
    let result = match &body { Some(b) => req.send_string(b), None => req.call() };
    let parse = |text: String| -> Value {
        let t = text.trim_start();
        if t.starts_with('{') || t.starts_with('[') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) { return serde_json_to_value(&v); }
        }
        Value::String(text)
    };
    let err = |kind: &str, msg: String, status: i64, body: Value| -> Value {
        map_from_pairs(vec![
            ("error".to_string(), Value::String(msg)),
            ("kind".to_string(), Value::String(kind.to_string())),
            ("status".to_string(), Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(status))),
            ("body".to_string(), body),
        ])
    };
    // read the whole body (bounded by max_bytes): a body that stalls after
    // the headers is a timeout, one larger than max_bytes is `too_large` —
    // both used to come back as a (truncated or empty) success
    let read_body = |resp: ureq::Response| -> Result<String, (String, String)> {
        use std::io::Read;
        let limit = max_bytes.unwrap_or(64 * 1024 * 1024);
        let mut buf = Vec::new();
        let mut reader = resp.into_reader().take(limit as u64 + 1);
        match reader.read_to_end(&mut buf) {
            Ok(_) if buf.len() > limit => Err(("too_large".to_string(), format!("{} {}: the body is larger than max_bytes ({})", method, url, limit))),
            Ok(_) => Ok(String::from_utf8_lossy(&buf).into_owned()),
            Err(e) => {
                let m = e.to_string().to_lowercase();
                let kind = if m.contains("timed out") || m.contains("timeout") || m.contains("would block") { "timeout" } else { "network" };
                Err((kind.to_string(), format!("{} {}: reading the body failed: {}", method, url, e)))
            }
        }
    };
    Ok(match result {
        Ok(resp) => {
            let status = resp.status();
            match read_body(resp) {
                Err((kind, msg)) => err(&kind, msg, status as i64, Value::Unit),
                // a 3xx that was not followed (a 307 to a POST) is not a success
                Ok(text) if !(200..300).contains(&status) =>
                    err("http_status", format!("{} {}: status code {}", method, url, status), status as i64, parse(text)),
                Ok(text) => parse(text),
            }
        }
        Err(ureq::Error::Status(code, resp)) => {
            let b = read_body(resp).map(parse).unwrap_or(Value::Unit);
            err("http_status", format!("{} {}: status code {}", method, url, code), code as i64, b)
        }
        Err(ureq::Error::Transport(t)) => {
            let msg = format!("{} {}: {}", method, url, t);
            let lower = msg.to_lowercase();
            let kind = if lower.contains("timed out") || lower.contains("timeout") { "timeout" }
                else if lower.contains("refused") { "refused" }
                else { "network" };
            err(kind, msg, 0, Value::Unit)
        }
    })
}
