//! Experimental full-mesh replication of explicitly selected Map slots.
//!
//! Each key is a last-writer-wins register ordered by (Lamport counter,
//! node ID). Deletes retain tombstones. This is NOT consensus or physical
//! sharding; HashRing is a placement utility, not the storage router.
use super::storage::{StorageBackend, StoredValue};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, RwLock,
};
use std::time::{Duration, Instant};

const VNODES: usize = 128;
const PREFIX: &str = "CLUSTER/2 ";

pub struct HashRing {
    ring: BTreeMap<u64, String>,
    nodes: Vec<String>,
}
impl HashRing {
    pub fn new() -> Self {
        Self {
            ring: BTreeMap::new(),
            nodes: Vec::new(),
        }
    }
    pub fn add_node(&mut self, node: &str) {
        if self.nodes.iter().any(|n| n == node) {
            return;
        }
        self.nodes.push(node.to_string());
        self.nodes.sort();
        for i in 0..VNODES {
            self.ring
                .insert(hash(&format!("{node}:{i}")), node.to_string());
        }
    }
    pub fn remove_node(&mut self, node: &str) {
        self.nodes.retain(|n| n != node);
        self.ring.retain(|_, n| n != node);
    }
    pub fn get_node(&self, key: &str) -> Option<&str> {
        self.ring
            .range(hash(key)..)
            .next()
            .or_else(|| self.ring.iter().next())
            .map(|(_, n)| n.as_str())
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }
    pub fn leader(&self) -> Option<&str> {
        self.nodes.first().map(String::as_str)
    }
}
fn hash(s: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(s.as_bytes());
    u64::from_be_bytes(d[..8].try_into().unwrap())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub String);
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub slot: String,
    pub key: String,
    pub version: Version,
    pub deleted: bool,
    pub value: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Frame {
    Join(String),
    Ack { node: String, members: Vec<String> },
    Members(Vec<String>),
    Heartbeat(String),
    Update(Update),
    Signal(String),
}
impl Frame {
    pub fn encode(&self) -> String {
        format!("{}{}\n", PREFIX, serde_json::to_string(self).unwrap())
    }
    pub fn decode(line: &str) -> Option<Self> {
        serde_json::from_str(line.strip_prefix(PREFIX)?).ok()
    }
}
pub fn valid_node_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && !id.chars().any(char::is_whitespace)
        && id.rsplit_once(':').map_or(false, |(host, port)| {
            !host.is_empty() && port.parse::<u16>().map_or(false, |p| p != 0)
        })
}

pub struct ClusterNode {
    pub node_id: String,
    pub ring: RwLock<HashRing>,
    heartbeats: Mutex<HashMap<String, Instant>>,
    known: Mutex<HashSet<String>>,
    links: Mutex<HashMap<String, std::sync::mpsc::SyncSender<String>>>,
    clock: AtomicU64,
    /// Same SQLite transaction as the data, or memory for ephemeral slots.
    pub metadata: HashMap<String, Arc<dyn StorageBackend>>,
}
impl ClusterNode {
    pub fn new(node_id: &str) -> Self {
        let mut ring = HashRing::new();
        ring.add_node(node_id);
        Self {
            node_id: node_id.into(),
            ring: RwLock::new(ring),
            heartbeats: Mutex::new(HashMap::new()),
            known: Mutex::new(HashSet::new()),
            links: Mutex::new(HashMap::new()),
            clock: AtomicU64::new(0),
            metadata: HashMap::new(),
        }
    }
    pub fn configure_slot(
        &mut self,
        name: &str,
        data: &Arc<dyn StorageBackend>,
        persistent: bool,
    ) -> Result<(), String> {
        if persistent && data.backend_name() != "sqlite" {
            return Err(format!("persistent cluster slot {name} requires the SQLite backend, so data and versions commit together"));
        }
        let meta: Arc<dyn StorageBackend> = if persistent {
            Arc::new(super::storage::SqliteBackend::new("_soma_cluster_v2", name))
        } else {
            Arc::new(super::storage::MemoryBackend::new())
        };
        for key in meta.keys() {
            let update = Self::read_update(&*meta, &key)
                .ok_or_else(|| format!("invalid cluster metadata for {name}/{key}"))?;
            if Frame::Update(update.clone()).encode().len() as u64
                > crate::interpreter::BUS_MAX_LINE
            {
                return Err(format!(
                    "cluster entry {name}/{key} exceeds the 16 MiB bus line limit"
                ));
            }
            self.observe(&update.version);
        }
        // Upgrade existing local data once. Metadata is never regenerated on
        // reconnect: doing so resurrected deleted keys on stale replicas.
        for key in data.keys() {
            if !meta.has(&key) {
                if let Some(value) = data.get(&key) {
                    let update = Update {
                        slot: name.into(),
                        key: key.clone(),
                        version: self.next_version()?,
                        deleted: false,
                        value: super::storage::stored_to_json(&value),
                    };
                    if Frame::Update(update.clone()).encode().len() as u64
                        > crate::interpreter::BUS_MAX_LINE
                    {
                        return Err(format!(
                            "cluster entry {name}/{key} exceeds the 16 MiB bus line limit"
                        ));
                    }
                    meta.set(
                        &key,
                        StoredValue::String(serde_json::to_string(&update).unwrap()),
                    );
                }
            }
        }
        self.metadata.insert(name.into(), meta);
        Ok(())
    }
    pub fn read_update(meta: &dyn StorageBackend, key: &str) -> Option<Update> {
        match meta.get(key)? {
            StoredValue::String(s) => serde_json::from_str(&s).ok(),
            _ => None,
        }
    }
    pub fn next_version(&self) -> Result<Version, String> {
        let n = self
            .clock
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .map_err(|_| "cluster logical clock exhausted".to_string())?
            + 1;
        Ok(Version(n, self.node_id.clone()))
    }
    pub fn observe(&self, version: &Version) {
        self.clock.fetch_max(version.0, Ordering::SeqCst);
    }
    pub fn snapshot(&self) -> Vec<Update> {
        // Never expose a handler's tentative data or metadata to a sync.
        crate::interpreter::with_committed_storage(|| {
            let mut updates = Vec::new();
            for meta in self.metadata.values() {
                for key in meta.keys() {
                    if let Some(u) = Self::read_update(&**meta, &key) {
                        updates.push(u);
                    }
                }
            }
            updates
        })
    }
    pub fn broadcast(&self, frame: Frame) {
        let line = frame.encode();
        let is_signal = matches!(frame, Frame::Signal(_));
        let mut links = self.links.lock().unwrap();
        if is_signal {
            for node in self.known.lock().unwrap().iter() {
                if !links.contains_key(node) {
                    // A seed alias can differ from its canonical node ID; the
                    // aggregate message below is authoritative when none is up.
                    if links.is_empty() {
                        eprintln!("cluster: signal NOT delivered (no connected peer)");
                        break;
                    }
                }
            }
        }
        links.retain(|node, tx| match tx.try_send(line.clone()) {
            Ok(()) => true,
            Err(_) => { eprintln!("cluster: link to {node} dropped (queue closed/full); data will resync, signals are not replayed"); false }
        });
        // A full/disconnected link is re-established by its supervisor. Data
        // catches up through snapshots; signals remain fire-and-forget.
    }
    pub fn record_heartbeat(&self, node: &str) {
        if node == self.node_id || !valid_node_id(node) {
            return;
        }
        let mut heartbeats = self.heartbeats.lock().unwrap();
        heartbeats.insert(node.into(), Instant::now());
        self.ring.write().unwrap().add_node(node);
    }
    pub fn check_dead_nodes(&self, timeout_secs: u64) -> Vec<String> {
        let mut heartbeats = self.heartbeats.lock().unwrap();
        let dead: Vec<_> = heartbeats
            .iter()
            .filter(|(_, at)| at.elapsed() > Duration::from_secs(timeout_secs))
            .map(|(n, _)| n.clone())
            .collect();
        for n in &dead {
            heartbeats.remove(n);
            self.ring.write().unwrap().remove_node(n);
        }
        dead
    }
    pub fn is_leader(&self) -> bool {
        self.ring.read().unwrap().leader() == Some(self.node_id.as_str())
    }
    pub fn node_count(&self) -> usize {
        self.ring.read().unwrap().node_count()
    }
    pub fn members(&self) -> Vec<String> {
        self.ring.read().unwrap().nodes().to_vec()
    }

    /// One outgoing stream per canonical peer, with bounded queues and retries.
    /// Incoming streams are receive-only: no duplicate EVENT broadcast paths.
    pub fn discover(self: &Arc<Self>, node: &str) {
        if node == self.node_id || !valid_node_id(node) {
            return;
        }
        let mut known = self.known.lock().unwrap();
        if known.len() >= 256 || !known.insert(node.into()) {
            return;
        }
        drop(known);
        let cluster = self.clone();
        let address = node.to_string();
        crate::interpreter::spawn_handler_thread(move || {
            let mut delay = 1;
            loop {
                match cluster.connect(&address) {
                    Ok(()) => return, // duplicate alias or a link to ourselves
                    Err(e) => eprintln!("cluster: {address}: {e}; retry in {delay}s"),
                }
                std::thread::sleep(Duration::from_secs(delay));
                delay = (delay * 2).min(30);
            }
        });
    }
    fn connect(self: &Arc<Self>, address: &str) -> Result<(), String> {
        let stream = address
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .find_map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(2)).ok())
            .ok_or_else(|| "cannot connect".to_string())?;
        stream.set_nodelay(true).ok();
        stream.set_write_timeout(Some(Duration::from_secs(2))).ok();
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        writer
            .write_all(Frame::Join(self.node_id.clone()).encode().as_bytes())
            .map_err(|e| e.to_string())?;
        let mut lines = crate::interpreter::bus_lines(std::io::BufReader::new(stream));
        let reply = lines
            .next()
            .ok_or("seed closed before cluster acknowledgement")?
            .map_err(|e| e.to_string())?;
        let Some(Frame::Ack { node, members }) = Frame::decode(&reply) else {
            return Err("seed did not acknowledge cluster protocol v2".into());
        };
        if !valid_node_id(&node) {
            return Err("invalid advertised node ID".into());
        }
        if node == self.node_id {
            return Ok(());
        }
        // Reserve canonical identity, so localhost/IP seed aliases never
        // count as extra members or create duplicate signal delivery paths.
        let (tx, rx) = std::sync::mpsc::sync_channel(crate::interpreter::BUS_QUEUE);
        {
            let mut links = self.links.lock().unwrap();
            if links.contains_key(&node) {
                return Ok(());
            }
            links.insert(node.clone(), tx);
        }
        self.record_heartbeat(&node);
        self.discover(&node);
        for n in members {
            self.discover(&n);
        }
        eprintln!(
            "cluster: connected to {node} ({} members)",
            self.node_count()
        );
        let result = (|| -> std::io::Result<()> {
            let mut sync_at: Option<Instant> = None;
            loop {
                if sync_at.map_or(true, |at| at.elapsed() >= Duration::from_secs(3)) {
                    writer.write_all(Frame::Heartbeat(self.node_id.clone()).encode().as_bytes())?;
                    writer.write_all(Frame::Members(self.members()).encode().as_bytes())?;
                    for update in self.snapshot() {
                        writer.write_all(Frame::Update(update).encode().as_bytes())?;
                    }
                    sync_at = Some(Instant::now());
                }
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(line) => writer.write_all(line.as_bytes())?,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            Ok(())
        })();
        self.links.lock().unwrap().remove(&node);
        let _ = writer.shutdown(std::net::Shutdown::Both);
        Err(result
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "outgoing queue disconnected".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_balances_and_moves_only_keys_owned_by_added_or_removed_node() {
        let mut a = HashRing::new();
        let mut b = HashRing::new();
        for node in ["a:8002", "b:8002", "c:8002"] {
            a.add_node(node);
        }
        for node in ["c:8002", "a:8002", "b:8002"] {
            b.add_node(node);
        }
        let mut counts = HashMap::new();
        for i in 0..30000 {
            let k = format!("key-{i}");
            assert_eq!(a.get_node(&k), b.get_node(&k));
            *counts
                .entry(a.get_node(&k).unwrap().to_string())
                .or_insert(0) += 1;
        }
        assert!(
            counts.values().all(|n| (7000..13000).contains(n)),
            "{counts:?}"
        );
        b.add_node("d:8002");
        for i in 0..30000 {
            let k = i.to_string();
            assert!(a.get_node(&k) == b.get_node(&k) || b.get_node(&k) == Some("d:8002"));
        }
        b.remove_node("d:8002");
        for i in 0..1000 {
            assert_eq!(a.get_node(&i.to_string()), b.get_node(&i.to_string()));
        }
    }
    #[test]
    fn membership_expires_monotonically_and_protocol_is_versioned() {
        let c = ClusterNode::new("a:1");
        c.record_heartbeat("b:2");
        c.heartbeats
            .lock()
            .unwrap()
            .insert("b:2".into(), Instant::now() - Duration::from_secs(20));
        assert_eq!(c.check_dead_nodes(15), vec!["b:2"]);
        assert_eq!(c.node_count(), 1);
        assert!(Frame::decode("CLUSTER JOIN a:1").is_none());
        assert!(!valid_node_id("a:0"));
        assert!(!valid_node_id("a:1\nEVENT bad"));
    }
}
