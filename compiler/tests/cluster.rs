//! Real processes with distinct stores: replication, rollback, recovery and
//! the boundary between local proofs and unproved distributed properties.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE: &str = r#"
cell ClusterTest {
 memory {
  data: Map<String, Any> [persistent, consistent]
  counts: Map<String, Int> [ephemeral, local]
 }
 scale { replicas: 3 shard: data consistency: eventual tolerance: 1 }
 every 100ms { counts.set("ticks", (counts.get("ticks") ?? 0) + 1) }
 on pulse(n: Int) { counts.set("events", (counts.get("events") ?? 0) + n) }
 on request(method: String, path: String, body: String) {
  let input = from_json(body)
  if path == "/put" { data.set(input.key, input.value) return map("ok", true) }
  if path == "/delete" { data.delete(input.key) return map("ok", true) }
  if path == "/fail" { data.set("ghost", 99) data.delete("answer") return 1 / 0 }
  if path == "/oversize" {
   let half = pad_left("", 9000000, "x")
   data.set("huge", half + half)
   return map("ok", true)
  }
  if path == "/emit" { emit pulse(1) return map("ok", true) }
  if path == "/metrics" { return map("events", counts.get("events") ?? 0, "ticks", counts.get("ticks") ?? 0) }
  if path == "/typed" {
   data.set("big", 1234567890123456789012345678901234567890)
   data.set("nested", map("a", [1, 2.5, true, (), ""], "__bigint__", "literal"))
   data.set("empty", "")
   data.set("null", ())
   return map("ok", true)
  }
  if path == "/other" { return delegate("Other", "put") }
  if path == "/other-read" { return delegate("Other", "read") }
  return map("entries", data.entries(), "values", data.values(), "keys", data.keys(), "size", data.len())
 }
}
cell Other {
 memory { data: Map<String, String> [persistent, consistent] }
 scale { replicas: 3 shard: data consistency: eventual tolerance: 1 }
 on put() { data.set("answer", "other-cell") return map("ok", true) }
 on read() { return map("value", data.get("answer")) }
}
"#;

fn port() -> u16 {
    // Choose disjoint triples below the default Linux/macOS ephemeral ranges.
    // The old 22000 + (pid % 1500) * 8 could reach 33992 and overlap Linux
    // outgoing connections. Bound both the pool and the number of probes.
    const BASE: u16 = 12000;
    const COUNT: usize = 2000;
    static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicUsize> = std::sync::OnceLock::new();
    let next = NEXT
        .get_or_init(|| std::sync::atomic::AtomicUsize::new(std::process::id() as usize % COUNT));
    for _ in 0..COUNT {
        let index = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) % COUNT;
        let p = BASE + index as u16 * 8;
        let listeners: Result<Vec<_>, _> = (0..3)
            .map(|i| TcpListener::bind(("127.0.0.1", p + i)))
            .collect();
        if listeners.is_ok() {
            return p;
        }
    }
    panic!("no free HTTP/bus port triple in the cluster test pool");
}
struct Node {
    child: Option<Child>,
    port: u16,
    dir: PathBuf,
    advertised: Option<u16>,
}
impl Node {
    fn new(root: &Path, name: &str, seed: Option<u16>, schedule: bool) -> Self {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.cell"), SOURCE).unwrap();
        let mut node = Self {
            child: None,
            port: port(),
            dir,
            advertised: None,
        };
        node.start(seed, schedule);
        node
    }
    fn start(&mut self, seed: Option<u16>, schedule: bool) {
        let log = std::fs::File::create(self.dir.join("serve.log")).unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
        cmd.current_dir(&self.dir)
            .args(["serve", "app.cell", "-p", &self.port.to_string()])
            .env(
                "SOMA_NODE_ID",
                format!("127.0.0.1:{}", self.advertised.unwrap_or(self.port + 2)),
            )
            .env_remove("SOMA_SEEDS")
            .stdout(Stdio::null())
            .stderr(log);
        if !schedule {
            cmd.arg("--no-schedule");
        }
        // Different seed spelling must not create phantom members.
        if let Some(p) = seed {
            cmd.args(["--join", &format!("localhost:{}", p + 2)]);
        }
        self.child = Some(cmd.spawn().unwrap());
        wait(|| {
            if let Some(status) = self.child.as_mut().unwrap().try_wait().unwrap() {
                panic!(
                    "server exited {status}: {}",
                    std::fs::read_to_string(self.dir.join("serve.log")).unwrap()
                );
            }
            request(self.port, "/", json!({})).is_some()
        });
    }
    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    fn call(&self, path: &str, body: Value) -> Value {
        request(self.port, path, body).expect("HTTP response").1
    }
    fn entries(&self) -> Value {
        self.call("/", json!({}))["entries"].clone()
    }
}
impl Drop for Node {
    fn drop(&mut self) {
        self.stop();
    }
}
fn request(port: u16, path: &str, body: Value) -> Option<(u16, Value)> {
    let mut s = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().ok()?,
        Duration::from_millis(200),
    )
    .ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(3))).ok()?;
    let body = body.to_string();
    write!(s, "POST {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let (headers, body) = out.split_once("\r\n\r\n")?;
    Some((
        headers.split_whitespace().nth(1)?.parse().ok()?,
        serde_json::from_str(body).ok()?,
    ))
}
fn wait(mut ready: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(45);
    loop {
        if ready() {
            return;
        }
        assert!(
            Instant::now() < until,
            "cluster did not converge before deadline"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("soma_cluster_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn replicas_preserve_types_rollback_and_converge_after_seed_loss_and_restart() {
    let root = scratch("replicas");
    let mut a = Node::new(&root, "a", None, false);
    a.call("/put", json!({"key":"answer", "value":42}));
    a.call("/typed", json!({}));
    let oversized = request(a.port, "/oversize", json!({})).unwrap();
    assert_eq!(oversized.0, 400);
    assert!(oversized.1.to_string().contains("16 MiB"), "{oversized:?}");
    assert_eq!(
        request(a.port, "/delete", json!({"key":"__reserved"}))
            .unwrap()
            .0,
        400
    );
    let mut b = Node::new(&root, "b", Some(a.port), false);
    let c = Node::new(&root, "c", Some(a.port), false);
    wait(|| a.entries() == b.entries() && a.entries() == c.entries());
    for node in [&a, &b, &c] {
        let result = node.call("/", json!({}));
        assert_eq!(result["size"], 5);
        assert_eq!(result["values"].as_array().unwrap().len(), 5);
        let big = result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["key"] == "big")
            .unwrap();
        assert_eq!(
            big["value"].to_string(),
            "1234567890123456789012345678901234567890"
        );
    }
    assert_eq!(request(b.port, "/fail", json!({})).unwrap().0, 400);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(a.entries(), b.entries());
    assert_eq!(a.entries(), c.entries());
    assert!(!a.entries().to_string().contains("ghost"));
    b.call("/other", json!({}));
    wait(|| {
        [&a, &b, &c]
            .iter()
            .all(|n| n.call("/other-read", json!({}))["value"] == "other-cell")
    });
    // One broadcast must execute once per live process despite discovery aliases.
    std::thread::sleep(Duration::from_secs(1));
    b.call("/emit", json!({}));
    wait(|| {
        [&a, &b, &c]
            .iter()
            .all(|n| n.call("/metrics", json!({}))["events"] == 1)
    });
    std::thread::sleep(Duration::from_millis(500));
    for n in [&a, &b, &c] {
        assert_eq!(n.call("/metrics", json!({}))["events"], 1);
    }
    // Leaves communicate directly after their discovery seed is killed.
    a.stop();
    b.call("/put", json!({"key":"leaf", "value": [7, false]}));
    wait(|| b.entries() == c.entries());
    // A stale persistent replica must not resurrect a key deleted offline.
    b.stop();
    c.call("/delete", json!({"key":"answer"}));
    c.call("/put", json!({"key":"offline", "value":"new"}));
    b.start(Some(c.port), false);
    wait(|| b.entries() == c.entries());
    assert!(!b.call("/", json!({}))["keys"]
        .as_array()
        .unwrap()
        .contains(&json!("answer")));
    // Concurrent conflicting writes converge under one deterministic version order.
    let bp = b.port;
    let cp = c.port;
    std::thread::scope(|s| {
        s.spawn(|| {
            for i in 0..20 {
                request(
                    bp,
                    "/put",
                    json!({"key":"conflict", "value":format!("b{i}")}),
                )
                .unwrap();
            }
        });
        s.spawn(|| {
            for i in 0..20 {
                request(
                    cp,
                    "/put",
                    json!({"key":"conflict", "value":format!("c{i}")}),
                )
                .unwrap();
            }
        });
    });
    wait(|| b.entries() == c.entries());
    drop((a, b, c));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsupported_consistency_is_not_reported_as_a_proof_or_silently_served() {
    let root = scratch("guarantees");
    for consistency in ["strong", "causal"] {
        let src = SOURCE.replace(
            "consistency: eventual",
            &format!("consistency: {consistency}"),
        );
        std::fs::write(root.join("app.cell"), src).unwrap();
        for args in [
            vec!["verify", "app.cell", "--strict"],
            vec!["serve", "app.cell", "-p", "0"],
        ] {
            let out = Command::new(env!("CARGO_BIN_EXE_soma"))
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(!out.status.success());
            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(output.contains("not implemented"), "{output}");
            assert!(
                !output.contains("all reads/writes") && !output.contains("CAP: CP"),
                "{output}"
            );
        }
    }
    std::fs::write(root.join("app.cell"), SOURCE).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["verify", "app.cell", "--strict"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("UNPROVEN distributed behavior"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cluster_protocol_cannot_mutate_a_non_cluster_service() {
    let root = scratch("private");
    // Expose an ordinary bus without scale. Legacy replication used to write
    // arbitrary slots directly, without a handler or an accept declaration.
    let source = SOURCE.replace(
        " scale { replicas: 3 shard: data consistency: eventual tolerance: 1 }",
        "",
    );
    let mut n = Node {
        child: None,
        port: port(),
        dir: root.clone(),
        advertised: None,
    };
    std::fs::write(root.join("app.cell"), source).unwrap();
    std::fs::write(
        root.join("soma.toml"),
        "[package]\nname = 'private'\n[bus]\naccept = ['pulse']\n",
    )
    .unwrap();
    n.start(None, false);
    let mut bus = TcpStream::connect(("127.0.0.1", n.port + 2)).unwrap();
    writeln!(
        bus,
        "EVENT _cluster_set {{\"slot\":\"data\",\"key\":\"bad\",\"value\":\"injected\"}}"
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(n.entries(), json!([]));
    drop(n);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stale_duplicate_and_forged_updates_cannot_resurrect_or_cross_slot_boundaries() {
    let root = scratch("reorder");
    let n = Node::new(&root, "node", None, false);
    let peer = format!("127.0.0.1:{}", port());
    let mut wire = TcpStream::connect(("127.0.0.1", n.port + 2)).unwrap();
    wire.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    writeln!(wire, "CLUSTER/2 {}", json!({"Join":peer})).unwrap();
    let mut reader = std::io::BufReader::new(wire.try_clone().unwrap());
    let mut ack = String::new();
    std::io::BufRead::read_line(&mut reader, &mut ack).unwrap();
    assert!(ack.contains("Ack"));
    let mut send = |slot: &str, version: u64, deleted: bool, value: Value| {
        writeln!(wire,"CLUSTER/2 {}",json!({"Update":{"slot":slot,"key":"answer","version":[version,peer],"deleted":deleted,"value":value}})).unwrap();
    };
    send("ClusterTest.data", 10, false, json!(42));
    send("ClusterTest.data", 10, false, json!(999)); // same ID cannot apply twice
    send("ClusterTest.data", 9, false, json!(888));
    wait(|| n.entries() == json!([{"key":"answer","value":42}]));
    send("ClusterTest.counts", 999, false, json!(123)); // not a replicated slot
    send("Other.data", 999, false, json!([123])); // wrong declared value type
    send("ClusterTest.data", 11, true, Value::Null);
    send("ClusterTest.data", 10, false, json!(42)); // stale set after tombstone
    wait(|| n.entries() == json!([]));
    assert_eq!(n.call("/other-read", json!({}))["value"], Value::Null);
    let log = std::fs::read_to_string(n.dir.join("serve.log")).unwrap();
    assert!(log.contains("update rejected"), "{log}");
    drop((wire, n));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn scheduling_rechecks_membership_and_takes_over_after_a_leader_dies() {
    let root = scratch("scheduler");
    let a = Node::new(&root, "a", None, true);
    let b = Node::new(&root, "b", Some(a.port), true);
    let (mut leader, follower) =
        if format!("127.0.0.1:{}", a.port + 2) < format!("127.0.0.1:{}", b.port + 2) {
            (a, b)
        } else {
            (b, a)
        };
    // Join is asynchronous; only stable membership provides this advisory rule.
    std::thread::sleep(Duration::from_secs(2));
    let before = follower.call("/metrics", json!({}))["ticks"]
        .as_u64()
        .unwrap();
    let active = leader.call("/metrics", json!({}))["ticks"]
        .as_u64()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(follower.call("/metrics", json!({}))["ticks"], before);
    assert!(
        leader.call("/metrics", json!({}))["ticks"]
            .as_u64()
            .unwrap()
            > active
    );
    leader.stop();
    wait(|| {
        follower.call("/metrics", json!({}))["ticks"]
            .as_u64()
            .unwrap()
            > before
    });
    drop((leader, follower));
    let _ = std::fs::remove_dir_all(root);
}

/// A real TCP cut, keeping both HTTP servers alive and writable.
struct CuttableBus {
    port: u16,
    enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    sockets: std::sync::Arc<std::sync::Mutex<Vec<TcpStream>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl CuttableBus {
    fn new(target: u16) -> Self {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        };
        // Keep the socket the OS allocates: probing and then rebinding leaves
        // a race with other listeners or outgoing connections on Linux CI.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let enabled = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let sockets = Arc::new(Mutex::new(Vec::<TcpStream>::new()));
        let (on, halt, open) = (enabled.clone(), stop.clone(), sockets.clone());
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !halt.load(Ordering::SeqCst) {
                let Ok((mut incoming, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                incoming.set_nonblocking(false).unwrap();
                let mut active = open.lock().unwrap();
                if !on.load(Ordering::SeqCst) {
                    continue;
                }
                let Ok(mut outgoing) = TcpStream::connect(("127.0.0.1", target)) else {
                    continue;
                };
                active.push(incoming.try_clone().unwrap());
                active.push(outgoing.try_clone().unwrap());
                let mut i = incoming.try_clone().unwrap();
                let mut o = outgoing.try_clone().unwrap();
                workers.push(std::thread::spawn(move || {
                    let _ = std::io::copy(&mut incoming, &mut outgoing);
                    let _ = outgoing.shutdown(std::net::Shutdown::Both);
                }));
                workers.push(std::thread::spawn(move || {
                    let _ = std::io::copy(&mut o, &mut i);
                    let _ = i.shutdown(std::net::Shutdown::Both);
                }));
            }
            for s in open.lock().unwrap().drain(..) {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            for w in workers {
                let _ = w.join();
            }
        });
        Self {
            port,
            enabled,
            stop,
            sockets,
            thread: Some(thread),
        }
    }
    fn set(&self, on: bool) {
        self.enabled.store(on, std::sync::atomic::Ordering::SeqCst);
        if !on {
            for s in self.sockets.lock().unwrap().drain(..) {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}
impl Drop for CuttableBus {
    fn drop(&mut self) {
        self.set(false);
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[test]
fn partitioned_writers_converge_after_the_bus_is_restored() {
    let root = scratch("partition");
    let mut a = Node {
        child: None,
        port: port(),
        dir: root.join("a"),
        advertised: None,
    };
    let mut b = Node {
        child: None,
        port: port(),
        dir: root.join("b"),
        advertised: None,
    };
    let pa = CuttableBus::new(a.port + 2);
    let pb = CuttableBus::new(b.port + 2);
    a.advertised = Some(pa.port);
    b.advertised = Some(pb.port);
    for n in [&a, &b] {
        std::fs::create_dir_all(&n.dir).unwrap();
        std::fs::write(n.dir.join("app.cell"), SOURCE).unwrap();
    }
    a.start(None, false);
    b.start(Some(pa.port - 2), false);
    a.call("/put", json!({"key":"erase","value":"old"}));
    wait(|| a.entries() == b.entries());
    pa.set(false);
    pb.set(false);
    a.call("/delete", json!({"key":"erase"}));
    a.call("/put", json!({"key":"conflict","value":"A"}));
    b.call("/put", json!({"key":"conflict","value":"B"}));
    assert_ne!(a.entries(), b.entries());
    pa.set(true);
    pb.set(true);
    wait(|| a.entries() == b.entries());
    let keys = a.call("/", json!({}))["keys"].clone();
    assert_eq!(keys, json!(["conflict"]));
    // It must stay converged after another anti-entropy round.
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(a.entries(), b.entries());
    drop((a, b, pa, pb));
    let _ = std::fs::remove_dir_all(root);
}
