use std::fs;
use std::io::Read as IoRead;
use std::path::PathBuf;
use std::process;

use crate::ast;
use crate::interpreter;
use crate::registry::Registry;
use crate::runtime;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

pub fn cmd_serve_watch(path: &PathBuf, port: u16, _registry: &mut Registry) {
    eprintln!("soma serve --watch");
    eprintln!("watching: {}", path.display());
    eprintln!("---");

    let exe = std::env::current_exe().unwrap();
    let mut last_modified = fs::metadata(path).ok()
        .and_then(|m| m.modified().ok());

    loop {
        let mut child = std::process::Command::new(&exe)
            .args(["serve", path.to_str().unwrap(), "-p", &port.to_string()])
            .spawn()
            .expect("failed to start server");

        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            let current = fs::metadata(path).ok()
                .and_then(|m| m.modified().ok());

            if current != last_modified {
                last_modified = current;
                eprintln!("\n--- file changed, reloading... ---\n");
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}

pub fn cmd_serve(path: &PathBuf, port: u16, verbose: bool, registry: &mut Registry) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);

    let cell = program
        .cells
        .iter()
        .find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "request" } else { false }
        }))
        .or_else(|| program.cells.iter().find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_)))))
        .unwrap_or_else(|| {
            eprintln!("error: no cell found");
            process::exit(1);
        });
    let cell_name = cell.node.name.clone();

    let handler_names: Vec<String> = cell.node.sections.iter()
        .filter_map(|s| {
            if let ast::Section::OnSignal(ref on) = s.node {
                Some(on.signal_name.clone())
            } else {
                None
            }
        })
        .collect();

    let handler_params: std::collections::HashMap<String, Vec<String>> = cell.node.sections.iter()
        .filter_map(|s| {
            if let ast::Section::OnSignal(ref on) = s.node {
                Some((on.signal_name.clone(), on.params.iter().map(|p| p.name.clone()).collect()))
            } else {
                None
            }
        })
        .collect();

    let mut storage_slots = std::collections::HashMap::new();
    for prog_cell in &program.cells {
        if prog_cell.node.kind != ast::CellKind::Cell { continue; }
        for section in &prog_cell.node.sections {
            if let ast::Section::Memory(ref mem) = section.node {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string())
                        .collect();
                    let backend = runtime::storage::resolve_backend_from_registry(
                        &prog_cell.node.name, &slot.node.name, &props, registry);
                    storage_slots.insert(
                        format!("{}.{}", prog_cell.node.name, slot.node.name), backend.clone());
                    storage_slots.insert(slot.node.name.clone(), backend);
                }
            }
        }
    }

    let addr = format!("0.0.0.0:{}", port);
    let server = tiny_http::Server::http(&addr).unwrap_or_else(|e| {
        eprintln!("error: cannot start server on {}: {}", addr, e);
        process::exit(1);
    });

    eprintln!("soma serve v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("cell: {}", cell_name);
    eprintln!("handlers: [{}]", handler_names.join(", "));
    eprintln!("database: {}", std::path::Path::new(".soma_data/soma.db").canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(".soma_data/soma.db")).display());
    eprintln!("listening on http://localhost:{}", port);
    eprintln!("---");

    let program = std::sync::Arc::new(program);
    let storage_slots = std::sync::Arc::new(storage_slots);
    let handler_names = std::sync::Arc::new(handler_names);
    let handler_params = std::sync::Arc::new(handler_params);
    let cell_name = std::sync::Arc::new(cell_name);
    let base_dir = std::sync::Arc::new(
        path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
    );

    // Create shared event bus for SSE + WebSocket
    let event_bus = interpreter::new_event_bus();

    // Shared WS client output (set by ws_connect, used by ws_send across all interpreters)
    let shared_ws_out: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Sender<String>>>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));

    // Run init() handler if it exists (may call ws_connect)
    if handler_names.contains(&"init".to_string()) || handler_names.contains(&"start".to_string()) {
        let init_signal = if handler_names.contains(&"init".to_string()) { "init" } else { "start" };
        let mut interp = interpreter::Interpreter::new(&program);
        interp.set_storage_raw(&storage_slots);
        interp.ensure_state_machine_storage();
        interp.event_bus = Some(event_bus.clone());
        let _ = interp.call_signal(&cell_name, init_signal, vec![]);
        // Capture ws_out if ws_connect was called
        if let Some(ref out) = interp.ws_out {
            if let Ok(mut shared) = shared_ws_out.lock() {
                *shared = Some(out.clone());
            }
        }
        eprintln!("init: {} executed", init_signal);
    }

    // Check if cell has a `ws` handler
    let has_ws_handler = handler_names.contains(&"ws".to_string());
    let ws_port = port + 1;

    // Spawn WebSocket server if `on ws(message)` handler exists
    if has_ws_handler {
        let prog = program.clone();
        let slots = storage_slots.clone();
        let cname = cell_name.clone();
        let bus = event_bus.clone();

        eprintln!("websocket: ws://localhost:{}", ws_port);

        std::thread::spawn(move || {
            let listener = match std::net::TcpListener::bind(format!("0.0.0.0:{}", ws_port)) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("ws error: cannot bind port {}: {}", ws_port, e);
                    return;
                }
            };

            // Track all WS client senders for broadcasting
            let ws_clients: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<std::sync::Mutex<tungstenite::WebSocket<std::net::TcpStream>>>>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

            // Bus → WS broadcast thread
            {
                let clients = ws_clients.clone();
                let (bus_tx, bus_rx) = std::sync::mpsc::channel::<interpreter::BusEvent>();
                if let Ok(mut senders) = bus.lock() {
                    senders.push(bus_tx);
                }
                std::thread::spawn(move || {
                    loop {
                        match bus_rx.recv() {
                            Ok(event) => {
                                let json = format!("{{\"event\":\"{}\",\"data\":{}}}", event.stream, event.data);
                                let msg = tungstenite::Message::Text(json);
                                if let Ok(mut clients) = clients.lock() {
                                    clients.retain(|client| {
                                        if let Ok(mut ws) = client.lock() {
                                            if ws.send(msg.clone()).is_ok() {
                                                ws.flush().is_ok()
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    });
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            }

            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let prog = prog.clone();
                let slots = slots.clone();
                let cname = cname.clone();
                let bus = bus.clone();
                let clients = ws_clients.clone();

                std::thread::spawn(move || {
                    // Clone the TCP stream BEFORE WS handshake
                    let read_stream = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => return,
                    };

                    let ws = match tungstenite::accept(stream) {
                        Ok(ws) => ws,
                        Err(_) => return,
                    };

                    // The write side: used for broadcasting (Arc<Mutex>)
                    let ws_write = std::sync::Arc::new(std::sync::Mutex::new(ws));

                    // Register the write handle for broadcasting
                    if let Ok(mut c) = clients.lock() {
                        c.push(ws_write.clone());
                    }

                    eprintln!("ws: client connected");

                    // Read side: create a separate WS from the cloned stream (no handshake needed — already done)
                    let mut ws_read = tungstenite::WebSocket::from_raw_socket(
                        read_stream,
                        tungstenite::protocol::Role::Server,
                        None,
                    );

                    // Read loop: blocks on read, doesn't hold any lock
                    loop {
                        match ws_read.read() {
                            Ok(tungstenite::Message::Text(text)) => {
                                let mut interp = interpreter::Interpreter::new(&prog);
                                interp.set_storage_raw(&slots);
                                interp.ensure_state_machine_storage();
                                interp.event_bus = Some(bus.clone());

                                let args = vec![interpreter::Value::String(text)];
                                match interp.call_signal(&cname, "ws", args) {
                                    Ok(val) => {
                                        if !matches!(val, interpreter::Value::Unit) {
                                            let response = format!("{}", val);
                                            if let Ok(mut ws_w) = ws_write.lock() {
                                                let _ = ws_w.send(tungstenite::Message::Text(response));
                                                let _ = ws_w.flush();
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let err_msg = format!("{{\"error\":\"{}\"}}", e);
                                        if let Ok(mut ws_w) = ws_write.lock() {
                                            let _ = ws_w.send(tungstenite::Message::Text(err_msg));
                                        }
                                    }
                                }
                            }
                            Ok(tungstenite::Message::Close(_)) | Err(_) => {
                                eprintln!("ws: client disconnected");
                                break;
                            }
                            _ => {}
                        }
                    }

                    if let Ok(mut c) = clients.lock() {
                        c.retain(|client| !std::sync::Arc::ptr_eq(client, &ws_write));
                    }
                });
            }
        });
    }

    // Spawn scheduler threads for `every` sections
    for cell_spanned in &program.cells {
        if cell_spanned.node.kind != ast::CellKind::Cell { continue; }
        for section in &cell_spanned.node.sections {
            if let ast::Section::Every(ref every) = section.node {
                let interval = every.interval_ms;
                let body = every.body.clone();
                let prog = program.clone();
                let slots = storage_slots.clone();
                let cname = cell_name.clone();
                let bus = event_bus.clone();
                let ws = shared_ws_out.clone();
                eprintln!("scheduler: every {}ms", interval);

                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(interval));
                        let mut interp = interpreter::Interpreter::new(&prog);
                        interp.set_storage_raw(&slots);
                        interp.ensure_state_machine_storage();
                        interp.event_bus = Some(bus.clone());
                        // Inherit WS client connection if available
                        if let Ok(ws_guard) = ws.lock() {
                            interp.ws_out = ws_guard.clone();
                        }
                        let mut env = std::collections::HashMap::new();
                        let _ = interp.exec_every(&body, &mut env, &cname);
                    }
                });
            }
        }
    }

    for mut request in server.incoming_requests() {
        let program = program.clone();
        let storage_slots = storage_slots.clone();
        let handler_names = handler_names.clone();
        let handler_params = handler_params.clone();
        let cell_name = cell_name.clone();
        let base_dir = base_dir.clone();
        let event_bus = event_bus.clone();
        let shared_ws = shared_ws_out.clone();

        std::thread::spawn(move || {
        let method = request.method().to_string();
        let url = request.url().to_string();

        let mut body_raw = String::new();
        let _ = request.as_reader().read_to_string(&mut body_raw);

        let body_value = if body_raw.starts_with('{') || body_raw.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body_raw) {
                Some(json_request_to_value(&parsed))
            } else {
                None
            }
        } else {
            None
        };
        let body = body_raw;

        if method == "OPTIONS" {
            let resp = tiny_http::Response::from_string("")
                .with_status_code(204)
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, PUT, DELETE, OPTIONS"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Max-Age"[..], &b"86400"[..]).unwrap());
            let _ = request.respond(resp);
            return;
        }

        if url.starts_with("/static/") {
            let file_path = base_dir.join(&url[1..]);
            if file_path.exists() && file_path.is_file() {
                let content = std::fs::read(&file_path).unwrap_or_default();
                let mime = match file_path.extension().and_then(|e| e.to_str()) {
                    Some("css") => "text/css",
                    Some("js") => "application/javascript",
                    Some("html") => "text/html; charset=utf-8",
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("svg") => "image/svg+xml",
                    Some("ico") => "image/x-icon",
                    Some("woff2") => "font/woff2",
                    Some("json") => "application/json",
                    _ => "application/octet-stream",
                };
                let resp = tiny_http::Response::from_data(content)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap()
                    );
                let _ = request.respond(resp);
                return;
            } else {
                let resp = tiny_http::Response::from_string("not found")
                    .with_status_code(404);
                let _ = request.respond(resp);
                return;
            }
        }

        let mut interp = interpreter::Interpreter::new(&program);
        interp.set_storage_raw(&storage_slots);
        interp.ensure_state_machine_storage();
        interp.event_bus = Some(event_bus.clone());
        if let Ok(ws_guard) = shared_ws.lock() {
            interp.ws_out = ws_guard.clone();
        }

        let (signal_name, args) = if url.starts_with("/signal/") {
            let signal = url.trim_start_matches("/signal/");
            let (sig_name, query) = signal.split_once('?').unwrap_or((signal, ""));
            let args: Vec<interpreter::Value> = if query.is_empty() {
                vec![]
            } else {
                query.split('&')
                    .filter_map(|pair| {
                        let (_, v) = pair.split_once('=')?;
                        Some(interpreter::Value::String(
                            urlencoding_decode(v)
                        ))
                    })
                    .collect()
            };
            (sig_name.to_string(), args)
        } else {
            let (url_path, query_string) = url.split_once('?').unwrap_or((&url, ""));
            let path = url_path.trim_start_matches('/');
            let (sig, rest) = path.split_once('/').unwrap_or((path, ""));
            if handler_names.contains(&sig.to_string()) {
                let mut args: Vec<interpreter::Value> = if rest.is_empty() {
                    vec![]
                } else {
                    rest.split('/')
                        .map(|s| {
                            let decoded = urlencoding_decode(s);
                            if let Ok(n) = decoded.parse::<i64>() {
                                interpreter::Value::Int(n)
                            } else {
                                interpreter::Value::String(decoded)
                            }
                        })
                        .collect()
                };
                if !query_string.is_empty() {
                    for pair in query_string.split('&') {
                        if let Some((_, v)) = pair.split_once('=') {
                            let decoded = urlencoding_decode(v).replace('+', " ");
                            args.push(interpreter::Value::String(decoded));
                        }
                    }
                }
                if method == "POST" && !body.is_empty() {
                    if let Some(ref bv) = body_value {
                        if let Some(param_names) = handler_params.get(sig) {
                            if let interpreter::Value::Map(ref entries) = bv {
                                let remaining_params = &param_names[args.len()..];
                                for pname in remaining_params {
                                    let val = entries.iter()
                                        .find(|(k, _)| k == pname)
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or(interpreter::Value::Unit);
                                    args.push(val);
                                }
                            } else {
                                args.push(bv.clone());
                            }
                        } else {
                            args.push(bv.clone());
                        }
                    } else {
                        args.push(interpreter::Value::String(body.clone()));
                    }
                }
                (sig.to_string(), args)
            } else if handler_names.contains(&"request".to_string()) {
                let (req_path, req_query) = url.split_once('?').unwrap_or((&url, ""));
                let query_map: Vec<(String, interpreter::Value)> = if req_query.is_empty() {
                    vec![]
                } else {
                    req_query.split('&')
                        .filter_map(|pair| {
                            let (k, v) = pair.split_once('=')?;
                            Some((
                                urlencoding_decode(k),
                                interpreter::Value::String(urlencoding_decode(v).replace('+', " ")),
                            ))
                        })
                        .collect()
                };
                let body_arg = body_value.clone()
                    .unwrap_or(interpreter::Value::String(body.clone()));

                let mut req_args = vec![
                    interpreter::Value::String(method.clone()),
                    interpreter::Value::String(req_path.to_string()),
                    body_arg,
                ];
                if !query_map.is_empty() {
                    req_args.push(interpreter::Value::Map(query_map));
                }
                (
                    "request".to_string(),
                    req_args,
                )
            } else {
                let resp = tiny_http::Response::from_string(
                    format!("{{\"error\": \"no handler for '{}'\", \"available\": [{:?}]}}",
                        url, handler_names.join(", "))
                )
                .with_status_code(404)
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..], &b"application/json"[..]
                    ).unwrap()
                );
                let _ = request.respond(resp);
                return;
            }
        };

        if verbose {
            eprintln!("  signal: {}", signal_name);
            eprintln!("  args: {:?}", args);
        }

        let start_time = std::time::Instant::now();

        match interp.call_signal(&cell_name, &signal_name, args) {
            Ok(val) => {
                // Check for SSE response
                let is_sse = if let interpreter::Value::Map(ref entries) = val {
                    entries.iter().any(|(k, v)| k == "_sse" && matches!(v, interpreter::Value::Bool(true)))
                } else {
                    false
                };

                if is_sse {
                    eprintln!("{} {} → SSE stream", method, url);

                    let (tx, rx) = std::sync::mpsc::channel::<interpreter::BusEvent>();
                    if let Ok(mut senders) = event_bus.lock() {
                        senders.push(tx);
                    }

                    // Get raw TCP stream from tiny_http request
                    let mut writer = request.into_writer();

                    // Write HTTP headers directly
                    use std::io::Write;
                    let _ = write!(writer, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: keep-alive\r\n\r\n");
                    let _ = writer.flush();

                    // Send initial event
                    let _ = write!(writer, "event: connected\ndata: {{\"status\":\"connected\"}}\n\n");
                    let _ = writer.flush();

                    // Stream events until client disconnects
                    loop {
                        match rx.recv_timeout(std::time::Duration::from_secs(15)) {
                            Ok(event) => {
                                let json = format!("{}", event.data);
                                let msg = format!("event: {}\ndata: {}\n\n", event.stream, json);
                                if write!(writer, "{}", msg).is_err() { break; }
                                if writer.flush().is_err() { break; }
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                if write!(writer, ": keepalive\n\n").is_err() { break; }
                                if writer.flush().is_err() { break; }
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    return;
                }

                let is_response = if let interpreter::Value::Map(ref entries) = val {
                    entries.iter().any(|(k, _)| k == "_status")
                } else {
                    false
                };
                let (status_code, body_str, content_type, extra_headers) = if is_response {
                    let entries = if let interpreter::Value::Map(ref e) = val { e } else { unreachable!() };
                    let status = entries.iter()
                        .find(|(k, _)| k == "_status")
                        .and_then(|(_, v)| if let interpreter::Value::Int(n) = v { Some(*n as u16) } else { None })
                        .unwrap_or(200);
                    let content_type = entries.iter()
                        .find(|(k, _)| k == "_content_type")
                        .and_then(|(_, v)| if let interpreter::Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or("application/json".to_string());
                    let body_val = entries.iter()
                        .find(|(k, _)| k == "_body")
                        .map(|(_, v)| v.clone())
                        .unwrap_or(interpreter::Value::Unit);
                    let headers: Vec<(String, String)> = entries.iter()
                        .filter(|(k, _)| !k.starts_with('_'))
                        .map(|(k, v)| (k.clone(), format!("{}", v)))
                        .collect();
                    let is_html = content_type.contains("html");
                    let body_str = if is_html {
                        match &body_val {
                            interpreter::Value::String(s) => s.clone(),
                            interpreter::Value::Unit => String::new(),
                            other => format!("{}", other),
                        }
                    } else {
                        match &body_val {
                            interpreter::Value::Unit => "{}".to_string(),
                            interpreter::Value::Map(_) | interpreter::Value::List(_) => format!("{}", body_val),
                            interpreter::Value::String(s) => {
                                if s.starts_with('{') || s.starts_with('[') { s.clone() }
                                else { format!("{{\"result\": \"{}\"}}", s) }
                            }
                            other => format!("{{\"result\": {}}}", other),
                        }
                    };
                    (status, body_str, content_type, headers)
                } else {
                    let body = match &val {
                        interpreter::Value::Unit => "{}".to_string(),
                        interpreter::Value::List(_) | interpreter::Value::Map(_) => format!("{}", val),
                        interpreter::Value::String(s) => {
                            if s.starts_with('{') || s.starts_with('[') { s.clone() }
                            else { format!("{{\"result\": \"{}\"}}", s) }
                        }
                        other => format!("{{\"result\": {}}}", other),
                    };
                    (200u16, body, "application/json".to_string(), vec![])
                };

                let verbose_body = if verbose { Some(body_str.clone()) } else { None };
                let mut resp = tiny_http::Response::from_string(body_str)
                    .with_status_code(tiny_http::StatusCode(status_code))
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..], content_type.as_bytes()
                        ).unwrap()
                    );
                for (key, val) in &extra_headers {
                    if let Ok(h) = tiny_http::Header::from_bytes(key.as_bytes(), val.as_bytes()) {
                        resp.add_header(h);
                    }
                }
                resp.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                let elapsed = start_time.elapsed();
                eprintln!("{} {} → {} {}ms", method, url, status_code, elapsed.as_millis());
                if let Some(ref vb) = verbose_body {
                    eprintln!("  response body: {}", vb);
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let body = format!("{{\"error\": \"{}\"}}", e);
                let mut resp = tiny_http::Response::from_string(body)
                    .with_status_code(500)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..], &b"application/json"[..]
                        ).unwrap()
                    );
                resp.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                let elapsed = start_time.elapsed();
                eprintln!("{} {} → 500 {}ms {}", method, url, elapsed.as_millis(), e);
                let _ = request.respond(resp);
            }
        }

        }); // end thread::spawn
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'%' => {
                let hi = chars.next().unwrap_or(b'0');
                let lo = chars.next().unwrap_or(b'0');
                let byte = (hex_val(hi) << 4) | hex_val(lo);
                result.push(byte as char);
            }
            b'+' => result.push(' '),
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn json_request_to_value(v: &serde_json::Value) -> interpreter::Value {
    match v {
        serde_json::Value::Null => interpreter::Value::Unit,
        serde_json::Value::Bool(b) => interpreter::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                interpreter::Value::Int(i)
            } else {
                interpreter::Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => interpreter::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            interpreter::Value::List(arr.iter().map(json_request_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            interpreter::Value::Map(
                obj.iter().map(|(k, v)| (k.clone(), json_request_to_value(v))).collect()
            )
        }
    }
}
