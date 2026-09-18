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

/// `--no-schedule`: HTTP handlers only, no `every` / `after` threads.
pub static NO_SCHEDULE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn cmd_serve(path: &PathBuf, port: u16, host: &str, verbose: bool, join: Option<&str>, no_check: bool, registry: &mut Registry) {
    crate::interpreter::PERSIST_MACHINES.store(true, std::sync::atomic::Ordering::Relaxed);
    crate::interpreter::IN_SERVE.store(true, std::sync::atomic::Ordering::Relaxed);
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);

    // A program that fails `soma check` does not serve: an undefined
    // function or a duplicate handler used to go live and fail per request.
    if !no_check {
        let mut chk = crate::checker::Checker::new(registry);
        chk.source = Some((file_str.clone(), source.clone()));
        chk.check(&program);
        if chk.has_errors() {
            eprintln!("{} fails `soma check` — fix these before serving (or pass --no-check):", path.display());
            for line in chk.report().lines().filter(|l| !l.starts_with("warning") && !l.starts_with("advisory") && !l.starts_with("✓")) {
                eprintln!("  {}", line);
            }
            process::exit(1);
        }
    }

    let cell = program
        .cells
        .iter()
        .find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "request" } else { false }
        }))
        .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_)))))
        .unwrap_or_else(|| {
            eprintln!("error: no cell found");
            process::exit(1);
        });
    let cell_name = cell.node.name.clone();

    let request_routes = std::sync::Arc::new(crate::checker::routes::explicit_routes(&cell.node));
    let mutating = {
        // interpolation / UFCS calls made explicit: `"{bal.set(k, 0)}"` in a
        // GET handler wrote state
        let analysis = crate::checker::desugar::expose_for_analysis(&program);
        let acell = analysis.cells.iter().find(|c| c.node.name == cell.node.name).map(|c| c.node.clone()).unwrap_or_else(|| cell.node.clone());
        std::sync::Arc::new(mutating_handlers(&acell))
    };
    {
        let mut ev: std::collections::HashSet<String> = std::collections::HashSet::new();
        fn emits(stmts: &[ast::Spanned<ast::Statement>], out: &mut std::collections::HashSet<String>) {
            for st in stmts {
                match &st.node {
                    ast::Statement::Emit { signal_name, .. } => { out.insert(signal_name.clone()); }
                    ast::Statement::If { then_body, else_body, .. } => { emits(then_body, out); emits(else_body, out); }
                    ast::Statement::For { body, .. } | ast::Statement::While { body, .. } => emits(body, out),
                    _ => {}
                }
            }
        }
        for c in &program.cells {
            for sec in &c.node.sections {
                match &sec.node {
                    ast::Section::OnSignal(on) => emits(&on.body, &mut ev),
                    ast::Section::Every(e) | ast::Section::After(e) => emits(&e.body, &mut ev),
                    _ => {}
                }
            }
        }
        let _ = EVENT_LISTENERS.set(ev);
    }
    let zero_arg_hooks: Vec<&'static str> = ["start", "init"].into_iter().filter(|h| cell.node.sections.iter().any(|s| matches!(&s.node,
        ast::Section::OnSignal(on) if on.signal_name == *h && crate::ast::accepted_arities(&on.params).contains(&0)))).collect();
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
    // The declared type of `request`'s body parameter decides its shape:
    // `body: String` gets the raw text (parse it with from_json), a Map /
    // List / Any / untyped parameter gets the parsed JSON. Before, JSON
    // arrived parsed whatever the declaration said, so `body == ""` on a
    // `String` parameter was a type error under serve only.
    let request_body_type: String = cell.node.sections.iter().find_map(|s| {
        if let ast::Section::OnSignal(ref on) = s.node {
            if on.signal_name == "request" {
                return on.params.get(2).map(|p| match &p.ty.node {
                    ast::TypeExpr::Simple(t) => t.clone(),
                    ast::TypeExpr::Generic { name, .. } => name.clone(),
                    _ => "Any".to_string(),
                });
            }
        }
        None
    }).unwrap_or_else(|| "Any".to_string());
    let handler_types: std::collections::HashMap<String, Vec<String>> = cell.node.sections.iter()
        .filter_map(|s| match &s.node {
            ast::Section::OnSignal(on) => Some((on.signal_name.clone(), on.params.iter().map(|p| match &p.ty.node {
                ast::TypeExpr::Simple(t) => t.clone(),
                ast::TypeExpr::Generic { name, .. } => name.clone(),
                _ => "Any".to_string(),
            }).collect())),
            _ => None,
        })
        .collect();

    let mut storage_slots = std::collections::HashMap::new();
    for prog_cell in &program.cells {
        if !matches!(prog_cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
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

    // ── Cluster mode ──────────────────────────────────────────────────
    // Three ways to form a cluster (in priority order):
    //   1. --join host:port          (CLI flag)
    //   2. SOMA_SEEDS=a:p,b:p,...    (env var, for containers)
    //   3. [cluster] seeds = [...]   (soma.toml)
    // If none specified, runs standalone.

    let base_dir_path = path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    let soma_toml_path = base_dir_path.join("soma.toml");

    // Extract scale section from the main cell
    let scale_section = cell.node.sections.iter().find_map(|s| {
        if let ast::Section::Scale(ref sc) = s.node { Some(sc.clone()) } else { None }
    });

    let bus_port = if (port as u32) + 2 <= 65535 { port + 2 } else { 0 };

    // Determine node_id and seeds to join
    let node_id = std::env::var("SOMA_NODE_ID")
        .unwrap_or_else(|_| format!("localhost:{}", bus_port));

    let seeds_to_join: Vec<String> = if let Some(join_addr) = join {
        // --join flag: single seed
        vec![join_addr.to_string()]
    } else if let Ok(env_seeds) = std::env::var("SOMA_SEEDS") {
        // SOMA_SEEDS env var
        env_seeds.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else if let Some(manifest) = soma_toml_path.exists().then(|| {
        std::fs::read_to_string(&soma_toml_path).ok()
            .and_then(|c| toml::from_str::<crate::pkg::manifest::Manifest>(&c).ok())
    }).flatten() {
        manifest.cluster.seeds.clone()
    } else {
        vec![]
    };

    // Load agent config from soma.toml
    let (agent_config, agent_models) = soma_toml_path.exists().then(|| {
        std::fs::read_to_string(&soma_toml_path).ok()
            .and_then(|c| toml::from_str::<crate::pkg::manifest::Manifest>(&c).ok())
            .map(|m| (Some(m.agent), m.models))
    }).flatten().unwrap_or((None, std::collections::HashMap::new()));
    // every interpreter of this process (ticks, init, bus, websocket — not
    // only requests) uses this [agent] config: a tick sent the env key to
    // api.openai.com instead of the configured url, or ignored [agent] mock
    let _ = crate::interpreter::DEFAULT_AGENT.set((agent_config.clone(), agent_models.clone()));
    let _ = BUS_ACCEPT.set(soma_toml_path.exists().then(|| std::fs::read_to_string(&soma_toml_path).ok()
        .and_then(|c| toml::from_str::<crate::pkg::manifest::Manifest>(&c).ok()).map(|m| m.bus.accept)).flatten().unwrap_or_default());

    // Cluster mode activates when: --join is specified, OR env SOMA_SEEDS, OR cell has scale { }
    let is_cluster_mode = !seeds_to_join.is_empty() || scale_section.is_some();

    let cluster_node: Option<std::sync::Arc<runtime::cluster::ClusterNode>> =
        if is_cluster_mode {
            let cluster = std::sync::Arc::new(runtime::cluster::ClusterNode::new(&node_id));

            // NOTE: join_cluster calls happen AFTER bus listener starts (see below)
            // Storage is NOT wrapped — replication uses the signal bus (EVENT protocol)
            // The interpreter intercepts storage ops and broadcasts via peer bus

            eprintln!("cluster: node '{}' ({} nodes, leader: {})",
                node_id,
                cluster.ring.read().unwrap().node_count(),
                if cluster.is_leader() { "yes" } else { "no" });

            Some(cluster)
        } else {
            None
        };

    let is_cluster_leader = cluster_node.as_ref().map_or(true, |c| c.is_leader());

    // Build sharded slots map from scale section
    let sharded_slots: std::collections::HashMap<String, bool> = scale_section.as_ref()
        .and_then(|s| s.shard.as_ref())
        .map(|shard_name| {
            let mut m = std::collections::HashMap::new();
            m.insert(shard_name.clone(), true);
            m.insert(format!("{}.{}", cell_name, shard_name), true);
            m
        })
        .unwrap_or_default();
    let sharded_slots = std::sync::Arc::new(sharded_slots);

    // Loopback by default: a fresh service is not on the network until
    // asked (--host 0.0.0.0). SO_REUSEADDR lets a wildcard bind succeed
    // beside a process that owns 127.0.0.1:port, so probe first.
    let addr = format!("{}:{}", host, port);
    let probe = if host == "0.0.0.0" { format!("127.0.0.1:{}", port) } else { addr.clone() };
    if let Ok(sa) = probe.parse::<std::net::SocketAddr>() {
        if std::net::TcpStream::connect_timeout(&sa, std::time::Duration::from_millis(200)).is_ok() {
            eprintln!("error: port {} is already in use (something answers on {}) — pick another with -p", port, probe);
            process::exit(1);
        }
    }
    let server = tiny_http::Server::http(&addr).unwrap_or_else(|e| {
        eprintln!("error: cannot start server on {}: {}", addr, e);
        process::exit(1);
    });

    // tiny_http runs one thread per open connection: a flood that reaches
    // the OS thread limit panicked a worker and poisoned its pool, and the
    // process stayed up serving NOTHING (a supervisor never restarted it).
    // A panic inside the HTTP layer now ends the process: a clean death a
    // supervisor restarts (handlers run in SQLite transactions: nothing is
    // half-written).
    {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            prev(info);
            let in_http = info.location().map_or(false, |l| l.file().contains("tiny_http"))
                || info.payload().downcast_ref::<String>().map_or(false, |m| m.contains("failed to spawn thread"))
                || info.payload().downcast_ref::<&str>().map_or(false, |m| m.contains("failed to spawn thread"));
            if in_http {
                eprintln!("serve: the HTTP layer failed (too many open connections for this machine's thread limit?) — exiting so a supervisor restarts it; cap concurrent connections in the reverse proxy");
                std::process::exit(70);
            }
        }));
    }

    eprintln!("soma serve v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("cell: {}", cell_name);
    // the public endpoints (private `_x` handlers and the router are not routed)
    let public: Vec<&String> = handler_names.iter().filter(|h| routable(&handler_names, h)).collect();
    eprintln!("endpoints: [{}]{}", public.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
        if handler_names.iter().any(|h| h == "request") { " + request router" } else { "" });
    if scale_section.is_some() {
        eprintln!("scale: replicas={} shard={} consistency={}",
            scale_section.as_ref().unwrap().replicas,
            scale_section.as_ref().unwrap().shard.as_deref().unwrap_or("none"),
            scale_section.as_ref().unwrap().consistency);
    }
    let db = crate::runtime::storage::data_dir().join("soma.db");
    eprintln!("database: {}", db.canonicalize().unwrap_or(db).display());
    let shown_host = if host == "0.0.0.0" { "0.0.0.0 (all interfaces)".to_string() } else { host.to_string() };
    eprintln!("listening on http://{}:{}", shown_host, port);
    eprintln!("dashboard: http://{}:{}/__soma/", if host == "0.0.0.0" { "localhost" } else { host }, port);
    if let Some(line) = llm_status_line(&program) { eprintln!("llm: {}", line); }
    eprintln!("---");

    // [native] handlers are compiled ONCE here and shared by every request
    // interpreter — they used to run interpreted under serve only, so
    // buffer()/hashmap() answered 400 and native/interpreted results differed
    let natives = match interpreter::native_ffi::compile_and_load_natives_with_config(
        &program, &crate::codegen::native::ParallelConfig::default()) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("error: [native] handlers do not compile — fix them or drop [native]:");
            for line in e.lines().take(40) { eprintln!("  {}", line); }
            process::exit(1);
        }
    };
    if !natives.is_empty() {
        eprintln!("native: {} handler(s) compiled", natives.len());
    }
    let natives = std::sync::Arc::new(natives);
    let program = std::sync::Arc::new(program);
    let storage_slots = std::sync::Arc::new(storage_slots);
    let handler_names = std::sync::Arc::new(handler_names);
    let handler_params = std::sync::Arc::new(handler_params);
    let request_body_type = std::sync::Arc::new(request_body_type);
    let handler_types = std::sync::Arc::new(handler_types);
    let cell_name = std::sync::Arc::new(cell_name);
    let base_dir = std::sync::Arc::new(
        path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
    );

    // Create shared event bus for SSE + WebSocket
    let event_bus = interpreter::new_event_bus();

    // Create peer bus for inter-process signal delivery
    let peer_bus = interpreter::new_peer_bus();

    // TCP bus listener: accepts incoming peer connections — only when the
    // program can use it (a cluster join, a `scale` section, or `emit`);
    // a plain service used to open an extra socket nobody asked for
    let uses_emit = source.contains("emit ");
    // a receiver that only ACCEPTS events (`[bus] accept`) needs the port too
    let bus_wanted = is_cluster_mode || uses_emit || BUS_ACCEPT.get().map_or(false, |a| !a.is_empty());
    if bus_port > 0 && !bus_wanted {
        eprintln!("bus: not started (no emit / scale / --join; port {} stays closed)", bus_port);
    }
    if bus_port > 0 && bus_wanted {
        let peer_bus_clone = peer_bus.clone();
        let event_bus_clone = event_bus.clone();
        let prog = program.clone();
        let slots = storage_slots.clone();
        let cname = cell_name.clone();
        let cluster_for_bus = cluster_node.clone();
        let sharded_for_bus = sharded_slots.clone();
        let my_node_id = if is_cluster_mode { node_id.clone() } else { String::new() };
        let bus_host = host.to_string();

        let natives = natives.clone();
        std::thread::spawn(move || {
            let listener = match std::net::TcpListener::bind(format!("{}:{}", bus_host, bus_port)) {
                Ok(l) => l,
                Err(e) => {
                    // the program emits: without its bus, events would be lost
                    // while the service looked up — like a taken HTTP port, exit 1
                    eprintln!("error: bus: cannot bind port {}: {} — another process holds it (choose another -p)", bus_port, e);
                    std::process::exit(1);
                }
            };
            eprintln!("bus: listening on :{}", bus_port);

            for stream in listener.incoming() {
                let stream = match stream { Ok(s) => s, Err(_) => continue };
                stream.set_nodelay(true).ok();
                let read_stream = match stream.try_clone() { Ok(s) => s, Err(_) => continue };

                // Register writer for this peer on the peer bus
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                if let Ok(mut senders) = peer_bus_clone.lock() {
                    senders.push(tx);
                }

                // If cluster mode, register this connection in the cluster node
                if let Some(ref cluster) = cluster_for_bus {
                    if let Ok(writer_clone) = stream.try_clone() {
                        // We'll get the peer's node_id from the CLUSTER JOIN message
                        // For now, register with the remote addr
                        let peer_addr = stream.peer_addr()
                            .map(|a| a.to_string())
                            .unwrap_or_else(|_| "unknown".to_string());
                        cluster.peers.lock().unwrap().insert(peer_addr, writer_clone);
                    }
                }

                // Writer: peer bus → TCP lines to peer
                let mut write_stream = stream;
                let natives = natives.clone();
                std::thread::spawn(move || {
                    use std::io::Write;
                    for line in rx {
                        if write_stream.write_all(line.as_bytes()).is_err() { return; }
                        let _ = write_stream.flush();
                    }
                });

                // Reader: TCP lines from peer → dispatch to handlers
                let prog2 = prog.clone();
                let slots2 = slots.clone();
                let cname2 = cname.clone();
                let ebus = event_bus_clone.clone();
                let cluster_for_reader = cluster_for_bus.clone();
                let sharded_for_reader = sharded_for_bus.clone();
                let my_nid = my_node_id.clone();
                let pbus = peer_bus_clone.clone();

                let natives = natives.clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    let reader = std::io::BufReader::new(read_stream);
                    eprintln!("bus: peer connected");
                    let mut first = true;
                    for line in crate::interpreter::bus_lines(reader) {
                        let line = match line { Ok(l) => l, Err(_) => break };
                        // a browser page can POST to the bus port (a cross-
                        // protocol request whose body carries `EVENT …`): an
                        // HTTP request line ends the connection
                        if first {
                            first = false;
                            let head = line.split(' ').next().unwrap_or("");
                            if line.contains(" HTTP/") || matches!(head, "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" | "PATCH" | "CONNECT" | "TRACE") {
                                eprintln!("bus: refused an HTTP request on the bus port");
                                break;
                            }
                        }
                        // `_private` handlers are not reachable from outside
                        // the process (only the `_cluster_*` replication ones)
                        if let Some(rest) = line.strip_prefix("EVENT ") {
                            let name = rest.split(' ').next().unwrap_or("");
                            // nor the router, the start-up hooks or `ws` (whose
                            // WebSocket origin check an EVENT bypassed)
                            if (name.starts_with('_') && !name.starts_with("_cluster_")) || matches!(name, "request" | "ws" | "start" | "init") {
                                eprintln!("bus: refused event '{}' (private handler)", name);
                                continue;
                            }
                        }

                        // Handle cluster membership protocol
                        if line.starts_with("CLUSTER ") {
                            if let Some(msg) = runtime::cluster::ClusterMsg::decode(&line) {
                                match msg {
                                    runtime::cluster::ClusterMsg::Join(peer_id) => {
                                        eprintln!("cluster: node '{}' joined", peer_id);
                                        if let Some(ref cluster) = cluster_for_reader {
                                            cluster.ring.write().unwrap().add_node(&peer_id);
                                            let nodes = cluster.ring.read().unwrap().nodes().to_vec();
                                            let reply = runtime::cluster::ClusterMsg::Members(nodes);
                                            if let Ok(peers) = pbus.lock() {
                                                let encoded = reply.encode();
                                                for tx in peers.iter() {
                                                    let _ = tx.send(encoded.clone());
                                                }
                                            }
                                            // Connect back to the peer for bidirectional EVENT routing
                                            // peer_id format is "host:bus_port" (e.g., "node2:8084")
                                            let pbus_back = pbus.clone();
                                            let pid = peer_id.clone();
                                            let natives = natives.clone();
                                            std::thread::spawn(move || {
                                                if let Ok(stream) = std::net::TcpStream::connect(&pid) {
                                                    stream.set_nodelay(true).ok();
                                                    let (tx, rx) = std::sync::mpsc::channel::<String>();
                                                    if let Ok(mut senders) = pbus_back.lock() {
                                                        senders.push(tx);
                                                    }
                                                    let mut writer = stream;
                                                    for line in rx {
                                                        use std::io::Write;
                                                        if writer.write_all(line.as_bytes()).is_err() { return; }
                                                        let _ = writer.flush();
                                                    }
                                                }
                                            });
                                            eprintln!("cluster: {} nodes, leader: {}",
                                                cluster.ring.read().unwrap().node_count(),
                                                if cluster.is_leader() { &my_nid } else { "other" });
                                        }
                                    }
                                    runtime::cluster::ClusterMsg::Members(nodes) => {
                                        if let Some(ref cluster) = cluster_for_reader {
                                            let mut ring = cluster.ring.write().unwrap();
                                            for node in &nodes {
                                                ring.add_node(node);
                                            }
                                            eprintln!("cluster: updated membership — {} nodes", ring.node_count());
                                        }
                                    }
                                    runtime::cluster::ClusterMsg::Heartbeat(peer_id) => {
                                        if let Some(ref cluster) = cluster_for_reader {
                                            cluster.record_heartbeat(&peer_id);
                                            // If this node isn't in our ring yet, add it
                                            let known = cluster.ring.read().unwrap().nodes().contains(&peer_id);
                                            if !known {
                                                cluster.ring.write().unwrap().add_node(&peer_id);
                                                eprintln!("cluster: discovered node '{}' via heartbeat", peer_id);
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }

                        // Handle all EVENT messages — both signals AND storage replication
                        // Storage events: _cluster_set, _cluster_del, _cluster_get, _cluster_get_reply, _cluster_values, _cluster_values_reply
                        if line.starts_with("EVENT ") {
                            let rest = &line[6..];
                            if let Some(space) = rest.find(' ') {
                                let event_name = &rest[..space];
                                let json_data = &rest[space+1..];

                                // Storage replication events — apply directly to local storage
                                if event_name == "_cluster_set" {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                        let slot = parsed.get("slot").and_then(|v| v.as_str()).unwrap_or("");
                                        let key = parsed.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                        let value = parsed.get("value").and_then(|v| v.as_str()).unwrap_or("");
                                        let slot_key = format!("{}.{}", cname2, slot);
                                        if let Some(backend) = slots2.get(&slot_key).or_else(|| slots2.get(slot)) {
                                            backend.set(key, runtime::storage::StoredValue::String(value.to_string()));
                                        }
                                    }
                                    continue;
                                }
                                if event_name == "_cluster_del" {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                        let slot = parsed.get("slot").and_then(|v| v.as_str()).unwrap_or("");
                                        let key = parsed.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                        let slot_key = format!("{}.{}", cname2, slot);
                                        if let Some(backend) = slots2.get(&slot_key).or_else(|| slots2.get(slot)) {
                                            backend.delete(key);
                                        }
                                    }
                                    continue;
                                }
                                if event_name == "_cluster_get" {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                        let slot = parsed.get("slot").and_then(|v| v.as_str()).unwrap_or("");
                                        let key = parsed.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                        let req_id = parsed.get("req_id").and_then(|v| v.as_str()).unwrap_or("");
                                        let slot_key = format!("{}.{}", cname2, slot);
                                        let value = slots2.get(&slot_key).or_else(|| slots2.get(slot))
                                            .and_then(|b| b.get(key))
                                            .map(|v| format!("{}", v))
                                            .unwrap_or_default();
                                        // Reply via bus
                                        let reply = format!("EVENT _cluster_get_reply {}\n",
                                            serde_json::json!({"req_id": req_id, "value": value}));
                                        if let Ok(peers) = pbus.lock() {
                                            for tx in peers.iter() {
                                                let _ = tx.send(reply.clone());
                                            }
                                        }
                                    }
                                    continue;
                                }
                                if event_name == "_cluster_get_reply" {
                                    // Route reply to pending request
                                    if let Some(ref cluster) = cluster_for_reader {
                                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                            let req_id = parsed.get("req_id").and_then(|v| v.as_str()).unwrap_or("");
                                            let value = parsed.get("value").and_then(|v| v.as_str()).unwrap_or("");
                                            if let Some(tx) = cluster.pending.lock().unwrap().remove(req_id) {
                                                let _ = tx.send(value.to_string());
                                            }
                                        }
                                    }
                                    continue;
                                }
                                if event_name == "_cluster_values" {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                        let slot = parsed.get("slot").and_then(|v| v.as_str()).unwrap_or("");
                                        let req_id = parsed.get("req_id").and_then(|v| v.as_str()).unwrap_or("");
                                        let slot_key = format!("{}.{}", cname2, slot);
                                        let vals: Vec<String> = slots2.get(&slot_key).or_else(|| slots2.get(slot))
                                            .map(|b| b.values().iter().map(|v| format!("{}", v)).collect())
                                            .unwrap_or_default();
                                        let reply = format!("EVENT _cluster_values_reply {}\n",
                                            serde_json::json!({"req_id": req_id, "values": vals}));
                                        if let Ok(peers) = pbus.lock() {
                                            for tx in peers.iter() {
                                                let _ = tx.send(reply.clone());
                                            }
                                        }
                                    }
                                    continue;
                                }
                                if event_name == "_cluster_values_reply" {
                                    if let Some(ref cluster) = cluster_for_reader {
                                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                            let req_id = parsed.get("req_id").and_then(|v| v.as_str()).unwrap_or("");
                                            let values = parsed.get("values").and_then(|v| v.as_str()).unwrap_or("[]");
                                            if let Some(tx) = cluster.pending.lock().unwrap().remove(req_id) {
                                                let _ = tx.send(values.to_string());
                                            }
                                        }
                                    }
                                    continue;
                                }

                                // Sync request: dump all local data for a slot
                                if event_name == "_cluster_sync_request" {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                                        let slot = parsed.get("slot").and_then(|v| v.as_str()).unwrap_or("");
                                        if !slot.is_empty() {
                                            let slot_key = format!("{}.{}", cname2, slot);
                                            if let Some(backend) = slots2.get(&slot_key).or_else(|| slots2.get(slot)) {
                                                let keys = backend.keys();
                                                let mut sent = 0;
                                                for key in &keys {
                                                    if let Some(val) = backend.get(key) {
                                                        let set_msg = format!("EVENT _cluster_set {}\n",
                                                            serde_json::json!({"slot": slot, "key": key, "value": format!("{}", val)}));
                                                        if let Ok(peers) = pbus.lock() {
                                                            for tx in peers.iter() {
                                                                let _ = tx.send(set_msg.clone());
                                                            }
                                                        }
                                                        sent += 1;
                                                    }
                                                }
                                                if sent > 0 {
                                                    eprintln!("cluster: synced {} keys from '{}'", sent, slot);
                                                }
                                            }
                                        }
                                    }
                                    continue;
                                }

                                // Regular signal — only an EVENT: one this program emits, or
                                // one soma.toml `[bus] accept` lists (any public 1-argument
                                // handler of any cell ran — `EVENT drain {}`)
                                let accepted = EVENT_LISTENERS.get().map_or(false, |e| e.contains(event_name))
                                    || BUS_ACCEPT.get().map_or(false, |a| a.iter().any(|x| x == event_name));
                                if !accepted {
                                    eprintln!("bus: refused event '{}' — not emitted by this program nor listed in soma.toml [bus] accept", event_name);
                                    continue;
                                }
                                let data = match serde_json::from_str::<serde_json::Value>(json_data) {
                                    // a peer cannot forge a record or a variant (HTTP refuses it too)
                                    Ok(parsed) if reserved_json_key(&parsed, false).is_some() => {
                                        eprintln!("bus: refused event '{}' (its data carries _type / _variant)", event_name);
                                        continue;
                                    }
                                    Ok(parsed) => interpreter::builtins::serde_json_to_value(&parsed),
                                    Err(_) => interpreter::Value::String(json_data.to_string()),
                                };

                                let mut interp = interpreter::Interpreter::new(&prog2);
                                interp.native_handlers = (*natives).clone();
                                interp.set_storage_raw(&slots2);
                                interp.ensure_state_machine_storage();
                                interp.event_bus = Some(ebus.clone());
                                interp.peer_bus = Some(pbus.clone());
                                if let Some(ref c) = cluster_for_reader { interp.set_cluster(c.clone(), &sharded_for_reader); }
                                // every cell with `on <event>` gets it, like an in-process
                                // emit (only the router cell was tried: Ledger.paid was lost)
                                let mut targets: Vec<String> = prog2.cells.iter()
                                    .filter(|c| c.node.sections.iter().any(|s| matches!(&s.node, ast::Section::OnSignal(on) if on.signal_name == event_name)))
                                    .map(|c| c.node.name.clone()).collect();
                                if targets.is_empty() { targets.push(cname2.to_string()); }
                                for target in targets {
                                    if let Err(e) = interp.call_signal(&target, event_name, vec![data.clone()]) {
                                        eprintln!("bus: event '{}' failed in {} (rolled back): {}", event_name, target, e);
                                    }
                                }
                            }
                        }
                    }
                    eprintln!("bus: peer disconnected");
                });
            }
        });
    }

    // ── Cluster join (after bus is listening) ──────────────────────────
    if let Some(ref cluster) = cluster_node {
        // Give bus listener thread time to bind
        std::thread::sleep(std::time::Duration::from_millis(500));
        for seed in &seeds_to_join {
            if seed == &node_id { continue; }
            eprintln!("cluster: connecting to {}...", seed);
            match cluster.join_cluster(seed) {
                Ok(()) => {
                    eprintln!("cluster: joined {}", seed);
                    // Request data sync from the seed
                    // The seed will respond with _cluster_set events for all its data
                    if let Ok(sync_stream) = std::net::TcpStream::connect(seed) {
                        sync_stream.set_nodelay(true).ok();
                        let shard_name = scale_section.as_ref().and_then(|s| s.shard.clone()).unwrap_or_default();
                        let sync_msg = format!("EVENT _cluster_sync_request {{\"slot\":\"{}\"}}\n", shard_name);
                        let mut sw = sync_stream;
                        use std::io::Write;
                        let _ = sw.write_all(sync_msg.as_bytes());
                        let _ = sw.flush();
                        // Connection will be closed after the message — that's fine
                    }
                    // Also connect our peer bus to the seed for bidirectional EVENT routing
                    // This ensures _cluster_set events flow from this node to the seed
                    if let Ok(stream) = std::net::TcpStream::connect(seed) {
                        stream.set_nodelay(true).ok();
                        let (tx, rx) = std::sync::mpsc::channel::<String>();
                        if let Ok(mut senders) = peer_bus.lock() {
                            senders.push(tx);
                        }
                        // Writer thread: peer bus → TCP
                        let mut writer = stream;
                        let natives = natives.clone();
                        std::thread::spawn(move || {
                            use std::io::Write;
                            for line in rx {
                                if writer.write_all(line.as_bytes()).is_err() { return; }
                                let _ = writer.flush();
                            }
                        });
                    }
                }
                Err(e) => eprintln!("cluster: {} (will accept incoming connections)", e),
            }
        }
        let count = cluster.ring.read().unwrap().node_count();
        if count > 1 {
            eprintln!("cluster: {} nodes active", count);
        }

        // Heartbeat thread: send heartbeat every 3s, check for dead nodes every 10s
        let hb_cluster = cluster.clone();
        let hb_bus = peer_bus.clone();
        let natives = natives.clone();
        std::thread::spawn(move || {
            let mut tick = 0u64;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                tick += 1;
                // Send heartbeat
                let msg = runtime::cluster::ClusterMsg::Heartbeat(hb_cluster.node_id.clone()).encode();
                if let Ok(senders) = hb_bus.lock() {
                    for tx in senders.iter() {
                        let _ = tx.send(msg.clone());
                    }
                }
                // Every 3rd tick (~9s), check for dead nodes
                if tick % 3 == 0 {
                    hb_cluster.check_dead_nodes(15);
                }
            }
        });
    }

    // Shared WS client output (set by ws_connect, used by ws_send across all interpreters)
    let shared_ws_out: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Sender<String>>>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));

    // Audit what .soma_data holds against THIS program (instances in
    // removed states, values an added invariant refuses) — one line each,
    // nothing changed
    {
        let mut interp = interpreter::Interpreter::new(&program);
        interp.set_storage_raw(&storage_slots);
        interp.ensure_state_machine_storage();
        for line in interp.audit_stored_data() {
            eprintln!("warning: stored data: {}", line);
        }
    }

    // Run init() handler if it exists (may call connect/ws_connect)
    // every zero-argument start-up hook runs, `start` then `init` (with
    // `start()` and `init(x)`, init() was called without its argument and
    // start() never ran)
    for init_signal in zero_arg_hooks.iter().copied() {
        let mut interp = interpreter::Interpreter::new(&program);
        interp.native_handlers = (*natives).clone();
        interp.set_storage_raw(&storage_slots);
        interp.ensure_state_machine_storage();
        interp.event_bus = Some(event_bus.clone());
        interp.peer_bus = Some(peer_bus.clone());
        if let Some(ref c) = cluster_node { interp.set_cluster(c.clone(), &sharded_slots); }
        let init_result = interp.call_signal(&cell_name, init_signal, vec![]);
        // Capture ws_out if ws_connect was called
        if let Some(ref out) = interp.ws_out {
            if let Ok(mut shared) = shared_ws_out.lock() {
                *shared = Some(out.clone());
            }
        }
        match init_result {
            Ok(_) => eprintln!("init: {}() ran", init_signal),
            Err(e) => eprintln!("init: {}() failed — {} (the service is up, the handler did nothing)", init_signal, e),
        }
    }

    // Auto-connect to peers declared in soma.toml
    {
        let soma_toml = base_dir.join("soma.toml");
        if soma_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&soma_toml) {
                if let Ok(manifest) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
                    for (peer_name, addr) in &manifest.peers {
                        eprintln!("peer: connecting to {} ({})", peer_name, addr);
                        let mut interp = interpreter::Interpreter::new(&program);
                        interp.native_handlers = (*natives).clone();
                        interp.set_storage_raw(&storage_slots);
                        interp.ensure_state_machine_storage();
                        interp.event_bus = Some(event_bus.clone());
                        interp.peer_bus = Some(peer_bus.clone());
                        if let Some(ref c) = cluster_node { interp.set_cluster(c.clone(), &sharded_slots); }
                        match interp.do_connect(addr, &cell_name) {
                            Ok(_) => eprintln!("peer: {} linked", peer_name),
                            Err(e) => eprintln!("peer: {} failed: {}", peer_name, e),
                        }
                    }
                }
            }
        }
    }

    // Check if cell has a `ws` handler
    let has_ws_handler = handler_names.contains(&"ws".to_string());
    let ws_port = if (port as u32) + 1 <= 65535 { port + 1 } else {
        eprintln!("warning: WS port {} exceeds 65535, disabled", (port as u32) + 1);
        0
    };

    // Spawn WebSocket server if `on ws(message)` handler exists
    if has_ws_handler && ws_port > 0 {
        let prog = program.clone();
        let slots = storage_slots.clone();
        let cname = cell_name.clone();
        let bus = event_bus.clone();

        eprintln!("websocket: ws://{}:{}", if host == "0.0.0.0" { "localhost" } else { host }, ws_port);
        let ws_host = host.to_string();
        let loopback_bind = matches!(host, "127.0.0.1" | "localhost" | "::1");

        let natives = natives.clone();
        std::thread::spawn(move || {
            let listener = match std::net::TcpListener::bind(format!("{}:{}", ws_host, ws_port)) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("error: websocket: cannot bind port {}: {} — another process holds it (choose another -p)", ws_port, e);
                    std::process::exit(1);
                }
            };

            // Track all WS client senders for broadcasting
            let ws_clients: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<std::sync::Mutex<tungstenite::WebSocket<std::net::TcpStream>>>>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

            // Bus → WS broadcast thread
            {
                let clients = ws_clients.clone();
                let (bus_tx, bus_rx) = std::sync::mpsc::sync_channel::<interpreter::BusEvent>(interpreter::BUS_QUEUE);
                if let Ok(mut senders) = bus.lock() {
                    senders.push(bus_tx);
                }
                let natives = natives.clone();
                std::thread::spawn(move || {
                    loop {
                        match bus_rx.recv() {
                            Ok(event) => {
                                // the data as JSON: a String payload went in unquoted (a client's
                                // `hi","event":"admin"` rewrote the envelope every client parsed)
                                let json = format!("{{\"event\":{},\"data\":{}}}", serde_json::to_string(&event.stream).unwrap_or_default(), crate::interpreter::builtins::string::to_json_string(&event.data));
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

                let natives = natives.clone();
                std::thread::spawn(move || {
                    // Clone the TCP stream BEFORE WS handshake
                    let read_stream = match stream.try_clone() {
                        Ok(s) => s,
                        Err(_) => return,
                    };

                    // a page on ANY site could open this socket and run `on ws`
                    // (cross-site WebSocket hijacking): a browser's Origin must
                    // be this machine (localhost / 127.0.0.1 / the Host header)
                    let check_origin = |req: &tungstenite::handshake::server::Request, resp: tungstenite::handshake::server::Response| {
                        let origin = req.headers().get("origin").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
                        let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("").split(':').next().unwrap_or("").to_string();
                        let ok = match origin {
                            None => true, // not a browser (a script, a peer)
                            Some(o) => {
                                let oh = o.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("").to_string();
                                let local = oh == "localhost" || oh == "127.0.0.1" || oh == "[::1]";
                                // Origin == Host only when serving beyond loopback:
                                // on 127.0.0.1 a DNS-rebinding page has Origin == Host
                                local || (!loopback_bind && !host.is_empty() && oh == host)
                            }
                        };
                        if ok { Ok(resp) } else {
                            eprintln!("ws: refused a connection from a foreign Origin");
                            let mut r = tungstenite::handshake::server::ErrorResponse::new(Some("cross-origin WebSocket refused".to_string()));
                            *r.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
                            Err(r)
                        }
                    };
                    let ws = match tungstenite::accept_hdr(stream, check_origin) {
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
                                interp.native_handlers = (*natives).clone();
                                interp.set_storage_raw(&slots);
                                interp.ensure_state_machine_storage();
                                interp.event_bus = Some(bus.clone());

                                let args = vec![interpreter::Value::String(text)];
                                let started = std::time::Instant::now();
                                match interp.call_signal(&cname, "ws", args) {
                                    Ok(val) => {
                                        eprintln!("ws: message → ok {}ms", started.elapsed().as_millis());
                                        if !matches!(val, interpreter::Value::Unit) {
                                            let response = format!("{}", val);
                                            if let Ok(mut ws_w) = ws_write.lock() {
                                                let _ = ws_w.send(tungstenite::Message::Text(response));
                                                let _ = ws_w.flush();
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // the same shape as an HTTP error: {"error", "kind"}
                                        eprintln!("ws: message → error ({}) {}", e.kind(), e);
                                        let err_msg = error_body(&format!("{}", e), &e.kind());
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
    // In cluster mode, only the leader runs `every` blocks
    let no_schedule = NO_SCHEDULE.load(std::sync::atomic::Ordering::Relaxed);
    if no_schedule { eprintln!("scheduler: disabled (--no-schedule) — every/after blocks do not run"); }
    for cell_spanned in &program.cells {
        if no_schedule { break; }
        if !matches!(cell_spanned.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
        for section in &cell_spanned.node.sections {
            if let ast::Section::Every(ref every) = section.node {
                if !is_cluster_leader {
                    eprintln!("scheduler: every {}ms (skipped — not leader)", every.interval_ms);
                    continue;
                }
                let interval = every.interval_ms;
                let body = every.body.clone();
                let prog = program.clone();
                let slots = storage_slots.clone();
                let cname = cell_spanned.node.name.clone();
                let bus = event_bus.clone();
                let pbus = peer_bus.clone();
                let ws = shared_ws_out.clone();
                let cluster_for_sched = cluster_node.clone();
                let sharded_for_sched = sharded_slots.clone();
                eprintln!("scheduler: every {}ms [{}]", interval, cname);

                let natives = natives.clone();
                std::thread::spawn(move || {
                    // Create interpreter ONCE and reuse across ticks
                    let mut interp = interpreter::Interpreter::new(&prog);
                    interp.native_handlers = (*natives).clone();
                    interp.set_storage_raw(&slots);
                    interp.ensure_state_machine_storage();
                    interp.event_bus = Some(bus.clone());
                    interp.peer_bus = Some(pbus.clone());
                    if let Some(ref c) = cluster_for_sched { interp.set_cluster(c.clone(), &sharded_for_sched); }
                    if let Ok(ws_guard) = ws.lock() {
                        interp.ws_out = ws_guard.clone();
                    }

                    // the first tick runs at start-up: a sweeper (`every 1s
                    // { expire_due() }`) must see work that became due while
                    // the server was down, not one interval later
                    let mut first = true;
                    loop {
                        if !first { std::thread::sleep(std::time::Duration::from_millis(interval)); }
                        first = false;
                        // Reset depth counter for each tick
                        interp.current_depth = 0;
                        // Pick up ws_out if it was set after init
                        if interp.ws_out.is_none() {
                            if let Ok(ws_guard) = ws.lock() {
                                interp.ws_out = ws_guard.clone();
                            }
                        }
                        let mut env = rustc_hash::FxHashMap::default();
                        match interp.exec_every(&body, &mut env, &cname) {
                            Err(e) => eprintln!("[scheduler:{}] tick error (rolled back): {}", cname, e),
                            Ok(_) if interp.last_commit_writes > 0 => eprintln!("[scheduler:{}] tick committed {} write(s)", cname, interp.last_commit_writes),
                            Ok(_) => {}
                        }
                    }
                });
            }
            if let ast::Section::After(ref after) = section.node {
                let delay = after.interval_ms;
                let body = after.body.clone();
                let prog = program.clone();
                let slots = storage_slots.clone();
                let cname = cell_spanned.node.name.clone();
                let bus = event_bus.clone();
                let pbus = peer_bus.clone();
                let ws = shared_ws_out.clone();
                let cluster_for_after = cluster_node.clone();
                let sharded_for_after = sharded_slots.clone();
                eprintln!("scheduler: after {}ms [{}]", delay, cname);

                let natives = natives.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    let mut interp = interpreter::Interpreter::new(&prog);
                    interp.native_handlers = (*natives).clone();
                    interp.set_storage_raw(&slots);
                    interp.ensure_state_machine_storage();
                    interp.event_bus = Some(bus);
                    interp.peer_bus = Some(pbus);
                    if let Some(ref c) = cluster_for_after { interp.set_cluster(c.clone(), &sharded_for_after); }
                    if let Ok(ws_guard) = ws.lock() {
                        interp.ws_out = ws_guard.clone();
                    }
                    let mut env = rustc_hash::FxHashMap::default();
                    match interp.exec_every(&body, &mut env, &cname) {
                        Err(e) => eprintln!("[after:{}] error (rolled back): {}", cname, e),
                        Ok(_) => eprintln!("[after:{}] ran, committed {} write(s)", cname, interp.last_commit_writes),
                    }
                });
            }
        }
    }

    for mut request in server.incoming_requests() {
        let program = program.clone();
        let storage_slots = storage_slots.clone();
        let handler_names = handler_names.clone();
        let mutating = mutating.clone();
        let request_routes = request_routes.clone();
        let handler_params = handler_params.clone();
        let request_body_type = request_body_type.clone();
        let handler_types = handler_types.clone();
        let cell_name = cell_name.clone();
        let base_dir = base_dir.clone();
        let event_bus = event_bus.clone();
        let peer_bus = peer_bus.clone();
        let shared_ws = shared_ws_out.clone();
        let cluster_for_http = cluster_node.clone();
        let sharded_for_http = sharded_slots.clone();
        let agent_config = agent_config.clone();
        let agent_models = agent_models.clone();

        let natives = natives.clone();
        let spawned = std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(move || {
        let method = request.method().to_string();
        // `//withdraw/…` collapsed to a handler while `request`'s match saw
        // the empty first segment: one canonical path for every router
        let url = {
            let raw = request.url().to_string();
            let (path, query) = match raw.split_once('?') { Some((p, q)) => (p.to_string(), Some(q.to_string())), None => (raw.clone(), None) };
            let mut collapsed = String::with_capacity(path.len());
            for ch in path.chars() {
                if ch == '/' && collapsed.ends_with('/') { continue; }
                collapsed.push(ch);
            }
            match query { Some(q) => format!("{}?{}", collapsed, q), None => collapsed }
        };
        // request headers, names lower-cased (the optional 5th parameter of
        // `request`: `on request(method, path, body, query: Map, headers: Map)`)
        // a repeated header is ONE entry, its values joined with ", " (HTTP
        // list semantics — the last one used to win silently)
        let req_headers: Vec<(String, interpreter::Value)> = {
            let mut acc: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
            for h in request.headers() {
                let k = h.field.as_str().as_str().to_ascii_lowercase();
                let v = h.value.as_str().to_string();
                acc.entry(k).and_modify(|e| { e.push_str(", "); e.push_str(&v); }).or_insert(v);
            }
            acc.into_iter().map(|(k, v)| (k, interpreter::Value::String(v))).collect()
        };

        // bytes first: a body that is not UTF-8 used to read as "" (a
        // `body: Map` handler then saw map() — the data silently lost)
        let mut body_bytes: Vec<u8> = Vec::new();
        let _ = request.as_reader().read_to_end(&mut body_bytes);
        let body_raw = match String::from_utf8(body_bytes) {
            Ok(t) => t,
            Err(_) => {
                let resp = tiny_http::Response::from_string(error_body("the request body is not valid UTF-8", "json"))
                    .with_status_code(400)
                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                eprintln!("{} {} → 400 0ms request body is not valid UTF-8", method, url);
                let _ = request.respond(cors(resp));
                return;
            }
        };

        // leading whitespace is JSON too (a pretty-printing client): trim
        let body_raw = body_raw.trim_start().to_string();
        // `_type` / `_variant` / `_values` anywhere in a client's JSON would
        // forge a record or a sum-type variant (a `Refund` that `Pay` does
        // not declare, stored and matched later); `_status` at the top would
        // make an echoed body an HTTP response
        let mut reserved_hit: Option<&'static str> = None;
        let body_value = if body_raw.starts_with('{') || body_raw.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body_raw) {
                reserved_hit = reserved_json_key(&parsed, true);
                // 1e400 read as Float inf: a number beyond Float range is refused
                fn out_of_range(v: &serde_json::Value) -> bool {
                    match v {
                        serde_json::Value::Number(n) => n.as_f64().map_or(false, |f| !f.is_finite()),
                        serde_json::Value::Array(xs) => xs.iter().any(out_of_range),
                        serde_json::Value::Object(m) => m.values().any(out_of_range),
                        _ => false,
                    }
                }
                if reserved_hit.is_none() && out_of_range(&parsed) { reserved_hit = Some("a number beyond Float range"); }
                Some(json_request_to_value(&parsed))
            } else {
                None
            }
        } else if !body_raw.is_empty() && !body_raw.starts_with('{') && body_raw.contains('=') {
            // Parse as form-encoded: key=value&key2=value2
            let mut form_data = Vec::new();
            for pair in body_raw.split('&') {
                if let Some((key, val)) = pair.split_once('=') {
                    let key = urlencoding_decode(key);
                    let val = urlencoding_decode(val);
                    form_data.push((key, interpreter::Value::String(val)));
                }
            }
            Some(interpreter::map_from_pairs(form_data))
        } else {
            None
        };
        let body = body_raw;

        // a returned map with `_status` IS an HTTP response (its other plain
        // keys become headers): a handler echoing a client object would let
        // the client pick the status and inject headers — refuse the key
        // the same keys in a FORM body or the QUERY string (`_type=Account`
        // made is_a(body, "Account") true)
        if reserved_hit.is_none() {
            let form_keys: Vec<String> = match &body_value {
                Some(interpreter::Value::Map(m)) => m.keys().cloned().collect(),
                _ => Vec::new(),
            };
            let query_keys: Vec<String> = url.split_once('?').map(|(_, q)| q.split('&')
                .map(|pair| urlencoding_decode(pair.split_once('=').map_or(pair, |(k, _)| k))).collect()).unwrap_or_default();
            // and header NAMES (`_type: Admin` reached a `headers: Map`)
            let header_keys: Vec<String> = req_headers.iter().map(|(k, _)| k.clone()).collect();
            for k in form_keys.iter().chain(query_keys.iter()).chain(header_keys.iter()) {
                if let Some(r) = ["_type", "_variant", "_values"].into_iter().find(|r| k == r) { reserved_hit = Some(r); break; }
            }
        }
        if let Some(key) = reserved_hit {
            let msg = if key.starts_with("a number") { "the request body holds a number beyond Float range (it would read as inf)".to_string() } else { format!("the request body may not carry `{}` (reserved: it marks {})", key,
                if key == "_status" { "a returned map as an HTTP response" } else { "a record or a sum-type variant — a client cannot forge one" }) };
            let resp = tiny_http::Response::from_string(error_body(&msg, "json"))
                .with_status_code(400)
                .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
            eprintln!("{} {} → 400 0ms body carries reserved `{}`", method, url, key);
            let _ = request.respond(cors(resp));
            return;
        }

        if method == "OPTIONS" {
            let resp = tiny_http::Response::from_string("")
                .with_status_code(204)
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, PUT, DELETE, OPTIONS"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization"[..]).unwrap())
                .with_header(tiny_http::Header::from_bytes(&b"Access-Control-Max-Age"[..], &b"86400"[..]).unwrap());
            let _ = request.respond(cors(resp));
            return;
        }

        if url.starts_with("/static/") {
            // drop any ?query (cache-busting `a.css?v=2`) before hitting disk
            let url_path = url.split('?').next().unwrap_or(&url);
            let file_path = base_dir.join(&url_path[1..]);
            // Canonicalize to prevent path traversal attacks
            let canonical = match file_path.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    let resp = tiny_http::Response::from_string("not found")
                        .with_status_code(404);
                    let _ = request.respond(cors(resp));
                    return;
                }
            };
            // Confine to <project>/static — NOT the project root: a root
            // check lets `/static/../soma.toml` (API keys), the .cell
            // sources and .soma_data out. A missing static/ dir confines
            // to a path nothing can be under.
            let static_root = base_dir.join("static");
            let base_canonical = static_root.canonicalize().unwrap_or(static_root);
            if !canonical.starts_with(&base_canonical) {
                let resp = tiny_http::Response::from_string("forbidden")
                    .with_status_code(403);
                let _ = request.respond(cors(resp));
                return;
            }
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
                    Some("txt") | Some("md") => "text/plain; charset=utf-8",
                    Some("csv") => "text/csv; charset=utf-8",
                    Some("xml") => "application/xml",
                    Some("mjs") => "application/javascript",
                    Some("htm") => "text/html; charset=utf-8",
                    Some("gif") => "image/gif",
                    Some("webp") => "image/webp",
                    Some("woff") => "font/woff",
                    Some("pdf") => "application/pdf",
                    Some("wasm") => "application/wasm",
                    _ => "application/octet-stream",
                };
                let resp = tiny_http::Response::from_data(content)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap()
                    );
                let _ = request.respond(cors(resp));
                return;
            } else {
                let resp = tiny_http::Response::from_string("not found")
                    .with_status_code(404);
                let _ = request.respond(cors(resp));
                return;
            }
        }

        // ── Verification dashboard ─────────────────────────────────────
        if url == "/__soma/" || url == "/__soma" {
            let html = super::dashboard::render_dashboard(&program);
            let resp = tiny_http::Response::from_string(html)
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap()
                );
            let _ = request.respond(cors(resp));
            return;
        }

        let mut interp = interpreter::Interpreter::new(&program);
        interp.native_handlers = (*natives).clone();
        interp.set_storage_raw(&storage_slots);
        interp.ensure_state_machine_storage();
        interp.event_bus = Some(event_bus.clone());
        interp.peer_bus = Some(peer_bus.clone());
        interp.agent_config = agent_config.clone();
        interp.agent_models = agent_models.clone();
        if let Some(ref c) = cluster_for_http { interp.set_cluster(c.clone(), &sharded_for_http); }
        if let Ok(ws_guard) = shared_ws.lock() {
            interp.ws_out = ws_guard.clone();
        }

        // a handler that changes state is not reachable by GET / HEAD: an
        // `<img src=…/put/z>` on any page (CORS is `*`) used to overwrite data
        // any method but POST/PUT/PATCH/DELETE (case-insensitive: `get`,
        // `Get`, TRACE…) is a read
        if !matches!(method.to_ascii_uppercase().as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
            let url_path = url.split('?').next().unwrap_or(&url);
            let target: Option<&str> = if let Some(sig) = url_path.strip_prefix("/signal/") {
                Some(sig)
            } else {
                let sig = url_path.trim_start_matches('/').split('/').next().unwrap_or("");
                if handler_names.iter().any(|h| h == sig) && routable(&handler_names, sig) && !request_routes.matches(url_path) { Some(sig) } else { None }
            };
            if let Some(sig) = target.filter(|s| mutating.contains(*s)) {
                let msg = format!("{}() changes state: call it with POST (a {} must not write)", sig, method);
                let resp = tiny_http::Response::from_string(error_body(&msg, "method_not_allowed"))
                    .with_status_code(405)
                    .with_header(tiny_http::Header::from_bytes(&b"Allow"[..], &b"POST"[..]).unwrap())
                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                eprintln!("{} {} → 405 0ms {}", method, url, msg);
                let _ = request.respond(cors(resp));
                return;
            }
        }

        let (signal_name, args) = if url.starts_with("/signal/") {
            let signal = url.trim_start_matches("/signal/");
            let (sig_name, query) = signal.split_once('?').unwrap_or((signal, ""));
            if !handler_names.iter().any(|h| h == sig_name) || !routable(&handler_names, sig_name) || request_routes.first_segments().iter().any(|f| f == sig_name) {
                let resp = tiny_http::Response::from_string(
                    format!("{{\"error\": \"no handler for '{}'\"}}", url)
                )
                .with_status_code(404)
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..], &b"application/json"[..]
                    ).unwrap()
                );
                let _ = request.respond(cors(resp));
                return;
            }
            let args: Vec<interpreter::Value> = if query.is_empty() {
                vec![]
            } else {
                query.split('&')
                    .filter_map(|pair| {
                        let (_, v) = pair.split_once('=')?;
                        Some(coerce_query_value(&urlencoding_decode(v)))
                    })
                    .collect()
            };
            (sig_name.to_string(), args)
        } else {
            let (url_path, query_string) = url.split_once('?').unwrap_or((&url, ""));
            let path = url_path.trim_start_matches('/');
            let (sig, rest) = path.split_once('/').unwrap_or((path, ""));
            // a path `request` matches explicitly is `request`'s, even when a
            // handler has the same name as its first segment
            // `request` itself is the router, never an endpoint: `GET
            // /request/POST/%2Fcredit/x` used to run a POST-only route
            // a handler that an explicit `request` route owns (`/withdraw/…`
            // with its auth) is reachable ONLY through that route: `/withdraw?
            // id=…&amt=…` reached it around the route's checks
            let route_owned = request_routes.first_segments().iter().any(|f| f == sig);
            if handler_names.contains(&sig.to_string()) && routable(&handler_names, sig) && !request_routes.matches(url_path) && !route_owned {
                let mut args: Vec<interpreter::Value> = if rest.is_empty() {
                    vec![]
                } else {
                    // a trailing slash is not an empty argument; `+` in a
                    // PATH is a plus sign (only a query encodes spaces so)
                    rest.trim_end_matches('/').split('/')
                        .map(|s| {
                            let decoded = urlencoding_decode(&s.replace('+', "%2B"));
                            if let Some(n) = decoded.parse::<i64>().ok().filter(|n| n.to_string() == decoded) {
                                interpreter::Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(n))
                            } else {
                                interpreter::Value::String(decoded)
                            }
                        })
                        .collect()
                };
                if !query_string.is_empty() {
                    // query values fill the parameters the path left open —
                    // by name when the key is one, else in order; a key the
                    // handler has no room for (?utm_source=…) is ignored
                    // rather than a "expected 1 argument, got 2"
                    let param_names = handler_params.get(sig).cloned().unwrap_or_default();
                    // a trailing `opts: Map` the path did not fill collects the
                    // query keys that name no parameter (`/add/1?step=7` →
                    // opts.step == 7; it used to be fed positionally)
                    let opts_idx = handler_types.get(sig).and_then(|t| {
                        let last = t.len().checked_sub(1)?;
                        (t[last] == "Map" && last >= args.len()).then_some(last)
                    });
                    let mut opts_map: Vec<(String, interpreter::Value)> = Vec::new();
                    let mut by_name: Vec<(usize, interpreter::Value)> = Vec::new();
                    let mut positional: Vec<interpreter::Value> = Vec::new();
                    for pair in query_string.split('&') {
                        if let Some((k, v)) = pair.split_once('=') {
                            let decoded = urlencoding_decode(v).replace('+', " ");
                            let key = urlencoding_decode(k);
                            match param_names.iter().position(|p| *p == key) {
                                Some(i) if i >= args.len() => by_name.push((i, coerce_query_value(&decoded))),
                                Some(_) => {}
                                None if opts_idx.is_some() => opts_map.push((key, coerce_query_value(&decoded))),
                                None => positional.push(coerce_query_value(&decoded)),
                            }
                        }
                    }
                    if let Some(oi) = opts_idx {
                        if !opts_map.is_empty() && !by_name.iter().any(|(i, _)| *i == oi) {
                            by_name.push((oi, interpreter::map_from_pairs(opts_map)));
                        }
                    }
                    let mut slots: Vec<Option<interpreter::Value>> = vec![None; param_names.len().saturating_sub(args.len())];
                    for (i, v) in by_name {
                        if let Some(slot) = slots.get_mut(i - args.len()) { *slot = Some(v); }
                    }
                    let mut positional = positional.into_iter();
                    for slot in slots.iter_mut() {
                        if slot.is_none() {
                            if let Some(v) = positional.next() { *slot = Some(v); }
                        }
                    }
                    for slot in slots.into_iter().flatten() {
                        args.push(slot);
                    }
                }
                if method == "POST" && !body.is_empty() {
                    if let Some(ref bv) = body_value {
                        if let Some(param_names) = handler_params.get(sig) {
                            if let interpreter::Value::Map(ref entries) = bv {
                                let remaining_params = &param_names[args.len()..];
                                // If there's exactly one remaining param and it doesn't
                                // match any key in the body, pass the whole Map
                                // (e.g. `on deploy(data: Map)` with body {"name":"x",...})
                                if remaining_params.len() == 1
                                    && !entries.contains_key(&remaining_params[0])
                                {
                                    args.push(bv.clone());
                                } else {
                                    for pname in remaining_params {
                                        let val = entries.get(pname)
                                            .cloned()
                                            .unwrap_or(interpreter::Value::Unit);
                                        args.push(val);
                                    }
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
                let body_arg = match request_body_type.as_str() {
                    "String" => interpreter::Value::String(body.clone()),
                    "Map" | "List" => match body_value.clone() {
                        Some(v) if matches!(v, interpreter::Value::Map(_) | interpreter::Value::List(_)) => v,
                        _ if body.trim().is_empty() => interpreter::Value::Map(Default::default()),
                        _ => {
                            let msg = if request_body_type.as_str() == "Map" {
                                "request body must be a JSON object `{…}` (the `request` handler declares body: Map)".to_string()
                            } else {
                                format!("request body must be a JSON {} (the `request` handler declares body: {})",
                                    if request_body_type.as_str() == "List" { "array `[…]`" } else { "value" }, request_body_type)
                            };
                            let resp = tiny_http::Response::from_string(
                                error_body(&msg, "json"))
                                .with_status_code(400)
                                .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                            eprintln!("{} {} → 400 0ms {}", method, url, msg);
                            let _ = request.respond(cors(resp));
                            return;
                        }
                    },
                    _ => body_value.clone().unwrap_or(interpreter::Value::String(body.clone())),
                };

                let mut req_args = vec![
                    interpreter::Value::String(method.clone()),
                    // percent-decoded per segment (`/stock/a%20b` reaches the
                    // handler as "/stock/a b"; an encoded slash stays one segment)
                    interpreter::Value::String(req_path.split('/').map(|seg| urlencoding_decode(seg).replace('/', "%2F")).collect::<Vec<_>>().join("/")),
                    body_arg,
                ];
                // The query map is the optional 4th parameter of `request`.
                // Passing it to a 3-parameter handler made ANY url with a
                // query string (?utm_source=…, a cache-buster) a 500:
                // "request() expected 3 arguments, got 4".
                // parameters 4 and 5 are bound BY NAME: one called `headers`
                // gets the headers, any other the query (a 4-parameter
                // `request(…, headers: Map)` used to receive the query — a
                // `?authorization=` could forge a header)
                let extra: Vec<String> = handler_params.get("request").map(|p| p.iter().skip(3).take(2).cloned().collect()).unwrap_or_default();
                for name in &extra {
                    if name == "headers" {
                        req_args.push(interpreter::map_from_pairs(req_headers.clone()));
                    } else {
                        req_args.push(interpreter::map_from_pairs(query_map.clone()));
                    }
                }
                (
                    "request".to_string(),
                    req_args,
                )
            } else {
                // public handlers only, as a real JSON array
                let public: Vec<&String> = handler_names.iter().filter(|h| routable(&handler_names, h)).collect();
                let resp = tiny_http::Response::from_string(
                    format!("{}", interpreter::map_from_pairs(vec![
                        ("error".to_string(), interpreter::Value::String(format!("no handler for '{}'", url))),
                        ("kind".to_string(), interpreter::Value::String("not_found".to_string())),
                        ("available".to_string(), interpreter::Value::List(public.iter().map(|h| interpreter::Value::String((*h).clone())).collect())),
                    ]))
                )
                .with_status_code(404)
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..], &b"application/json"[..]
                    ).unwrap()
                );
                let _ = request.respond(cors(resp));
                return;
            }
        };

        // path segments and query values are text: coerce them to the
        // handler's declared parameter types (Bool "true", Int "5", a JSON
        // Map/List) — a `Bool` parameter used to refuse `/decide/x/true`
        let args: Vec<interpreter::Value> = match handler_types.get(&signal_name) {
            Some(types) => {
                let mut out: Vec<interpreter::Value> = args.into_iter().enumerate()
                    .map(|(i, a)| coerce_to_type(types.get(i).map(|s| s.as_str()).unwrap_or("Any"), a)).collect();
                // a Map/List parameter that still holds text: the body was
                // not JSON → 400 kind json, before the handler (as for `request`)
                // text that is not JSON — or JSON `null` (it ran the body with ())
                if let Some(i) = out.iter().enumerate().position(|(i, v)| matches!(v, interpreter::Value::String(_) | interpreter::Value::Unit) && matches!(types.get(i).map(|s| s.as_str()), Some("Map" | "List"))) {
                    let msg = format!("{}(): parameter '{}' expects {} — the request body must be JSON", signal_name,
                        handler_params.get(&signal_name).and_then(|p| p.get(i)).cloned().unwrap_or_default(), types[i]);
                    let resp = tiny_http::Response::from_string(error_body(&msg, "json"))
                        .with_status_code(400)
                        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    eprintln!("{} {} → 400 0ms {}", method, url, msg);
                    let _ = request.respond(cors(resp));
                    return;
                }
                // an absent body for a trailing Map/List parameter is an empty one
                while out.len() < types.len() && matches!(types[out.len()].as_str(), "Map" | "List") {
                    out.push(if types[out.len()] == "Map" { interpreter::Value::Map(Default::default()) } else { interpreter::Value::List(vec![]) });
                }
                out
            }
            None => args,
        };

        if verbose {
            eprintln!("  signal: {}", signal_name);
            eprintln!("  args: {:?}", args);
        }

        let start_time = std::time::Instant::now();

        let outcome = interp.call_signal(&cell_name, &signal_name, args);
        // the trace outlives the request (one interpreter per request):
        // `trace()` in a later request sees the last 1000 steps
        interpreter::builtins::storage::serve_trace_extend(&interp.agent_trace);
        match outcome {
            Ok(val) => {
                // Check for SSE response
                let is_sse = if let interpreter::Value::Map(ref entries) = val {
                    entries.get("_sse").map(|v| matches!(v, interpreter::Value::Bool(true))).unwrap_or(false)
                        && interpreter::is_http_response(&val)
                } else {
                    false
                };

                if is_sse {
                    eprintln!("{} {} → SSE stream", method, url);
                    // only the streams this client subscribed to (every client
                    // received every publish: tenant B read tenant A's events);
                    // `sse()` with no names subscribes to all of them
                    let streams: Vec<String> = match &val {
                        interpreter::Value::Map(e) => match e.get("_streams") {
                            Some(interpreter::Value::List(xs)) => xs.iter().map(|x| format!("{}", x)).collect(),
                            _ => Vec::new(),
                        },
                        _ => Vec::new(),
                    };

                    let (tx, rx) = std::sync::mpsc::sync_channel::<interpreter::BusEvent>(interpreter::BUS_QUEUE);
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
                                if !streams.is_empty() && !streams.iter().any(|n| *n == event.stream) { continue; }
                                // JSON on ONE line: a String payload with a newline forged
                                // `event:` / `data:` lines for the other subscribers
                                let json = crate::interpreter::builtins::string::to_json_string(&event.data);
                                let msg = format!("event: {}\ndata: {}\n\n", event.stream.replace(['\n', '\r'], " "), json);
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
                    entries.get("_status").is_some() && interpreter::is_http_response(&val)
                } else {
                    false
                };
                let (status_code, body_str, content_type, extra_headers) = if is_response {
                    let entries = if let interpreter::Value::Map(ref e) = val { e } else { unreachable!() };
                    // a status outside 100–599 was wrapped mod 65536 (99999
                    // went out as 34463, 65736 as 200): answer 500 instead
                    let raw_status = entries.get("_status")
                        .and_then(|v| if let interpreter::Value::Int(si) = v { si.to_i64() } else { None });
                    let status: u16 = match raw_status {
                        Some(n) if (200..=599).contains(&n) => n as u16,
                        None => 200,
                        Some(n) => {
                            eprintln!("error: {} {}: response status {} is not an HTTP status (100–599) — answered 500", method, url, n);
                            500
                        }
                    };
                    let content_type = entries.get("_content_type")
                        .and_then(|v| if let interpreter::Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or("application/json".to_string());
                    let body_val = entries.get("_body")
                        .cloned()
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
                            interpreter::Value::Unit => "null".to_string(),
                            interpreter::Value::Map(_) | interpreter::Value::List(_) | interpreter::Value::Variant { .. } =>
                                interpreter::builtins::string::to_json_string(&body_val),
                            interpreter::Value::String(s) => {
                                if s.starts_with('{') || s.starts_with('[') { s.clone() }
                                else { serde_json::json!({ "result": s }).to_string() }
                            }
                            other => format!("{{\"result\": {}}}", other),
                        }
                    };
                    (status, body_str, content_type, headers)
                } else {
                    let body = match &val {
                        interpreter::Value::Unit => "null".to_string(),
                        interpreter::Value::List(_) | interpreter::Value::Map(_) | interpreter::Value::Variant { .. } =>
                            interpreter::builtins::string::to_json_string(&val),
                        interpreter::Value::String(s) => {
                            if s.starts_with('{') || s.starts_with('[') { s.clone() }
                            else { serde_json::json!({ "result": s }).to_string() }
                        }
                        other => format!("{{\"result\": {}}}", other),
                    };
                    (200u16, body, "application/json".to_string(), vec![])
                };

                let verbose_body = if verbose { Some(body_str.clone()) } else { None };
                // 204 / 304 carry no body (HTTP): `response(204, x)` sent one
                let body_str = if status_code == 204 || status_code == 304 { String::new() } else { body_str };
                let mut resp = tiny_http::Response::from_string(body_str)
                    .with_status_code(tiny_http::StatusCode(status_code))
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..], content_type.as_bytes()
                        ).unwrap()
                    );
                for (key, val) in &extra_headers {
                    // a header name is a token; a value has no CR/LF or other
                    // control character (`filename={name}` with %0d%0a split
                    // the response); framing headers belong to the server
                    let name_ok = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
                    let value_ok = val.bytes().all(|b| b == b'\t' || (b >= 0x20 && b != 0x7f));
                    let framing = matches!(key.to_ascii_lowercase().as_str(), "content-length" | "transfer-encoding" | "connection");
                    if !name_ok || !value_ok || framing {
                        eprintln!("warning: {} {}: response header {:?} dropped (invalid name, a control character in the value, or a framing header)", method, url, key);
                        continue;
                    }
                    if let Ok(h) = tiny_http::Header::from_bytes(key.as_bytes(), val.as_bytes()) {
                        resp.add_header(h);
                    }
                }
                // the handler's own CORS origin wins (two headers: browsers reject both)
                let resp = cors(resp);
                let elapsed = start_time.elapsed();
                eprintln!("{} {} → {} {}ms", method, url, status_code, elapsed.as_millis());
                if let Some(ref vb) = verbose_body {
                    eprintln!("  response body: {}", vb);
                }
                let _ = request.respond(cors(resp));
            }
            Err(e) => {
                let kind = e.kind();
                let status = status_for_kind(&kind);
                let body = error_body(&format!("{}", e), &kind);
                let mut resp = tiny_http::Response::from_string(body)
                    .with_status_code(status)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..], &b"application/json"[..]
                        ).unwrap()
                    );
                resp.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                let elapsed = start_time.elapsed();
                eprintln!("{} {} → {} {}ms {}", method, url, status, elapsed.as_millis(), e);
                let _ = request.respond(cors(resp));
            }
        }

        }); // end thread::spawn
        if let Err(e) = spawned {
            eprintln!("error: cannot spawn request thread: {}", e);
        }
    }
}

/// `{"error": …, "kind": …}` rendered like every other Soma map (same
/// spacing as a handler's own `response(404, map("error", …))`).
/// What `think()` will talk to, said once at start-up: an agent ran a
/// pipeline for an hour against the echo mock without knowing.
fn llm_status_line(program: &ast::Program) -> Option<String> {
    let uses_think = program.cells.iter().any(|c| c.node.sections.iter().any(|s| {
        let body: &[ast::Spanned<ast::Statement>] = match &s.node {
            ast::Section::OnSignal(h) => &h.body,
            ast::Section::Every(e) | ast::Section::After(e) => &e.body,
            _ => return false,
        };
        let mut found = false;
        crate::checker::literals::for_each_call(body, &mut |name, _, _| {
            if matches!(name, "think" | "think_json" | "delegate") { found = true; }
        });
        found
    }));
    if !uses_think { return None; }
    if let Ok(m) = std::env::var("SOMA_LLM_MOCK") {
        return Some(format!("MOCK `{}` (SOMA_LLM_MOCK) — think() never reaches a provider", m));
    }
    let toml = std::fs::read_to_string("soma.toml").ok()
        .and_then(|t| toml::from_str::<crate::pkg::manifest::Manifest>(&t).ok());
    let agent = toml.as_ref().map(|m| m.agent.clone());
    if let Some(a) = &agent {
        if !a.mock.is_empty() {
            return Some(format!("MOCK `{}` ([agent] mock in soma.toml) — think() never reaches a provider", a.mock));
        }
    }
    let key_env = ["SOMA_LLM_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"].iter()
        .find(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()));
    let provider = agent.as_ref().map(|a| a.provider.clone()).filter(|p| !p.is_empty());
    let model = agent.as_ref().map(|a| a.model.clone()).filter(|m| !m.is_empty());
    match (provider, model, key_env) {
        (p, m, Some(k)) => Some(format!("provider {} model {} (key from {})", p.unwrap_or_else(|| "default".into()), m.unwrap_or_else(|| "default".into()), k)),
        (p, m, None) if agent.as_ref().is_some_and(|a| !a.key.is_empty()) => Some(format!("provider {} model {} (key from soma.toml)", p.unwrap_or_else(|| "default".into()), m.unwrap_or_else(|| "default".into()))),
        _ => Some("NO KEY and no mock — every think() will raise kind `llm` (set SOMA_LLM_KEY, or SOMA_LLM_MOCK=echo)".to_string()),
    }
}

/// Every response carries the CORS header — the documented rule (it used to
/// be missing on `/static/*`, the dashboard and the pre-handler 400s).
fn cors<R: std::io::Read>(mut r: tiny_http::Response<R>) -> tiny_http::Response<R> {
    if !r.headers().iter().any(|h| h.field.equiv("Access-Control-Allow-Origin")) {
        r.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
    }
    r
}

fn error_body(message: &str, kind: &str) -> String {
    format!("{}", interpreter::map_from_pairs(vec![
        ("error".to_string(), interpreter::Value::String(message.to_string())),
        ("kind".to_string(), interpreter::Value::String(kind.to_string())),
    ]))
}

/// HTTP status for a handler error, by kind: refusals the program made on
/// purpose are client errors, not 500s.
pub(crate) fn status_for_kind(kind: &str) -> u16 {
    match kind {
        "not_found" => 404,
        "guard_failed" | "forbidden" | "approval_required" => 403,
        "invalid_transition" | "conflict" => 409,
        "invariant" | "ensure" => 422,
        "json" | "division_by_zero" | "type" => 400,
        "stack_overflow" | "llm" | "budget" | "undefined_variable" | "undefined_function" | "no_handler" | "response" => 500,
        _ => 400, // `require … else Tag`, fail("tag") — the program refused the request
    }
}

/// Coerce a decoded query-string value to a typed Value so a handler
/// declaring `seat: Int` receives an Int, not a String. Mirrors the
/// path-segment and `soma run` CLI coercion: int, then float, then bool,
/// else string.
/// A text argument (path segment, query value, CLI token) coerced to the
/// declared parameter type; anything that does not fit is left as-is and
/// refused by the call boundary with a precise message.
pub(crate) fn coerce_to_type(ty: &str, v: interpreter::Value) -> interpreter::Value {
    use interpreter::Value;
    match (ty, &v) {
        ("Bool", Value::String(s)) if s == "true" || s == "false" => Value::Bool(s == "true"),
        ("String", Value::Int(_) | Value::Float(_) | Value::Bool(_)) => Value::String(format!("{}", v)),
        ("Float", Value::Int(i)) => Value::Float(i.to_f64()),
        // "NaN" / "inf" are not numbers a handler can compare: left a String
        ("Float", Value::String(s)) if s.parse::<f64>().map_or(false, |f| f.is_finite()) => Value::Float(s.parse().unwrap()),
        ("Int", Value::String(s)) if s.parse::<i64>().is_ok() => Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(s.parse().unwrap())),
        // Int is arbitrary precision: 99999999999999999999999 is an Int
        ("Int", Value::String(s)) if s.parse::<rug::Integer>().is_ok() => Value::Int(crate::interpreter::soma_int::SomaInt::from_rug(s.parse::<rug::Integer>().unwrap())),
        ("Int", Value::Float(f)) if f.fract() == 0.0 => Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(*f as i64)),
        ("Map" | "List", Value::String(s)) => {
            if s.trim().is_empty() {
                return if ty == "Map" { Value::Map(Default::default()) } else { Value::List(vec![]) };
            }
            match serde_json::from_str::<serde_json::Value>(s) {
                // `_type` / `_variant` would forge a record or a variant
                Ok(j) if reserved_json_key(&j, false).is_some() => v,
                Ok(j) => crate::interpreter::builtins::serde_json_to_value(&j),
                Err(_) => v,
            }
        }
        _ => v,
    }
}

fn coerce_query_value(decoded: &str) -> interpreter::Value {
    // a number only in its canonical spelling: "007", "1e3", "+7", "-0" are
    // text (an account id "00123" was stored as "123" in a String parameter);
    // a declared Int / Float parameter still converts them
    if let Some(n) = decoded.parse::<i64>().ok().filter(|n| n.to_string() == decoded) {
        interpreter::Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(n))
    } else if let Some(f) = decoded.parse::<f64>().ok().filter(|f| f.is_finite() && format!("{}", f) == decoded) {
        interpreter::Value::Float(f)
    } else if decoded == "true" {
        interpreter::Value::Bool(true)
    } else if decoded == "false" {
        interpreter::Value::Bool(false)
    } else {
        interpreter::Value::String(decoded.to_string())
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            // an invalid escape (`%ZZ`, a trailing `%`) stays literal — it
            // decoded to a NUL character
            b'%' if i + 2 < raw.len() && raw[i + 1].is_ascii_hexdigit() && raw[i + 2].is_ascii_hexdigit() => {
                bytes.push((hex_val(raw[i + 1]) << 4) | hex_val(raw[i + 2]));
                i += 3;
                continue;
            }
            b'+' => bytes.push(b' '),
            b => bytes.push(b),
        }
        i += 1;
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// One JSON → Value conversion for the whole toolchain (big integers stay
/// exact, 1e400 is infinity, never a silent 0.0) — this file had its own
/// copy that lost both.
fn reserved_json_key(v: &serde_json::Value, top: bool) -> Option<&'static str> {
    match v {
        serde_json::Value::Object(m) => {
            for k in ["_type", "_variant", "_values"] {
                if m.contains_key(k) { return Some(k); }
            }
            m.values().find_map(|x| reserved_json_key(x, false))
        }
        serde_json::Value::Array(xs) => xs.iter().find_map(|x| reserved_json_key(x, false)),
        _ => None,
    }
}

fn json_request_to_value(v: &serde_json::Value) -> interpreter::Value {
    crate::interpreter::builtins::serde_json_to_value(v)
}

/// The start-up hook serve runs once (`init`, else `start`): not an HTTP
/// endpoint — anyone could re-run it (reset state, reconnect) with a GET.
fn lifecycle_hook(names: &[String]) -> Option<&'static str> {
    if names.iter().any(|n| n == "init") { Some("init") }
    else if names.iter().any(|n| n == "start") { Some("start") }
    else { None }
}

/// A handler reachable over HTTP as `/<name>/…`: not private (`_x`), not the
/// `request` router, not the start-up hook.
/// Handlers that are the target of an `emit` somewhere in the program: event
/// listeners, not endpoints (`POST /moved` forged the event)
use crate::interpreter::{EVENT_LISTENERS, BUS_ACCEPT};

fn routable(names: &[String], h: &str) -> bool {
    // an event listener — emitted here or accepted from other processes — is
    // not an HTTP endpoint (POST /ping forged the peer's event)
    if EVENT_LISTENERS.get().map_or(false, |e| e.contains(h)) { return false; }
    if BUS_ACCEPT.get().map_or(false, |a| a.iter().any(|x| x == h)) { return false; }
    // `start` and `init` are both start-up names: neither is an endpoint
    // (with both declared, `start` was served and re-ran on every POST)
    let _ = lifecycle_hook(names);
    // `ws` answers WebSocket frames on port+1, not HTTP requests
    !h.starts_with('_') && h != "request" && h != "init" && h != "start" && h != "ws"
}

/// Handlers that change state: a slot write, a transition, an emit, a call
/// into another cell — or a call to a sibling that does (transitively).
fn mutating_handlers(cell: &ast::CellDef) -> std::collections::HashSet<String> {
    use ast::{Expr, Section, Statement};
    use std::collections::{HashMap, HashSet};
    let slots: HashSet<String> = cell.sections.iter().filter_map(|s| match &s.node {
        Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()),
        _ => None,
    }).flatten().collect();
    const WRITES: [&str; 8] = ["set", "push", "append", "delete", "remove", "clear", "update", "pop"];
    let mut direct: HashSet<String> = HashSet::new();
    let mut calls: HashMap<String, Vec<String>> = HashMap::new();
    for sec in &cell.sections {
        let Section::OnSignal(on) = &sec.node else { continue };
        let mut writes = false;
        let mut callees = Vec::new();
        crate::checker::literals::for_each_expr(&on.body, &mut |e| match e {
            Expr::FnCall { name, .. } => {
                if matches!(name.as_str(), "transition" | "remember" | "delegate" | "publish" | "write_file" | "next_id" | "think" | "think_json" | "http_post" | "http_put" | "http_delete") { writes = true; }
                callees.push(name.clone());
            }
            Expr::MethodCall { target, method, .. } => {
                if let Expr::Ident(t) = &target.node {
                    if slots.contains(t) && WRITES.contains(&method.as_str()) { writes = true; }
                    // `Other.h(…)`: another cell may write
                    if t.chars().next().map_or(false, |c| c.is_uppercase()) && !slots.contains(t) { writes = true; }
                }
            }
            _ => {}
        });
        fn stmts_write(stmts: &[ast::Spanned<Statement>], slots: &HashSet<String>) -> bool {
            stmts.iter().any(|st| match &st.node {
                Statement::MethodCall { target, method, .. } => (slots.contains(target) && WRITES.contains(&method.as_str()))
                    || target.chars().next().map_or(false, |c| c.is_uppercase()),
                Statement::IndexSet { name, .. } => slots.contains(name),
                Statement::Emit { .. } => true,
                Statement::If { then_body, else_body, .. } => stmts_write(then_body, slots) || stmts_write(else_body, slots),
                Statement::For { body, .. } | Statement::While { body, .. } => stmts_write(body, slots),
                _ => false,
            })
        }
        if writes || stmts_write(&on.body, &slots) { direct.insert(on.signal_name.clone()); }
        calls.insert(on.signal_name.clone(), callees);
    }
    loop {
        let before = direct.len();
        for (h, cs) in &calls {
            if !direct.contains(h) && cs.iter().any(|c| direct.contains(c)) { direct.insert(h.clone()); }
        }
        if direct.len() == before { break; }
    }
    direct
}
