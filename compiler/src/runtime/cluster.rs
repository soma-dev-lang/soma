//! Cluster runtime — consistent hash ring, node discovery, leader election.
//!
//! The same cell code runs on 1 machine or 20,000.
//! Memory slots ARE cells. Signals ARE replication.
//!
//! When `--join` connects two nodes:
//!   trades.set(key, val) → local write + EVENT trades_set {key, val} on bus
//!   trades.get(key)      → local read or EVENT trades_get {key, req_id} + wait reply
//!   trades.values()      → fan-out EVENT trades_values {req_id} + merge replies
//!
//! No custom protocol. The signal bus IS the replication protocol.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, RwLock};

// ── Consistent Hash Ring ─────────────────────────────────────────────

const VNODES: usize = 128;

pub struct HashRing {
    ring: BTreeMap<u64, String>,
    nodes: Vec<String>,
}

impl HashRing {
    pub fn new() -> Self {
        Self { ring: BTreeMap::new(), nodes: Vec::new() }
    }

    pub fn add_node(&mut self, node: &str) {
        if self.nodes.contains(&node.to_string()) { return; }
        self.nodes.push(node.to_string());
        for i in 0..VNODES {
            let key = format!("{}:{}", node, i);
            let hash = fnv_hash(&key);
            self.ring.insert(hash, node.to_string());
        }
    }

    pub fn remove_node(&mut self, node: &str) {
        self.nodes.retain(|n| n != node);
        self.ring.retain(|_, v| v != node);
    }

    /// Find the primary node responsible for a key
    pub fn get_node(&self, key: &str) -> Option<&str> {
        if self.ring.is_empty() { return None; }
        let hash = fnv_hash(key);
        let node = self.ring.range(hash..)
            .next()
            .or_else(|| self.ring.iter().next())
            .map(|(_, v)| v.as_str());
        node
    }

    pub fn node_count(&self) -> usize { self.nodes.len() }
    pub fn nodes(&self) -> &[String] { &self.nodes }

    /// Leader = lowest node ID (deterministic, no election needed)
    pub fn leader(&self) -> Option<&str> {
        self.nodes.iter().min().map(|s| s.as_str())
    }
}

fn fnv_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ── Cluster Protocol (minimal — only JOIN/MEMBERS) ───────────────────
// Everything else uses the standard EVENT bus.

#[derive(Debug, Clone)]
pub enum ClusterMsg {
    Join(String),
    Members(Vec<String>),
}

impl ClusterMsg {
    pub fn encode(&self) -> String {
        match self {
            ClusterMsg::Join(id) => format!("CLUSTER JOIN {}\n", id),
            ClusterMsg::Members(nodes) => format!("CLUSTER MEMBERS {}\n", nodes.join(",")),
        }
    }

    pub fn decode(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() < 3 || parts[0] != "CLUSTER" { return None; }
        match parts[1] {
            "JOIN" => Some(ClusterMsg::Join(parts[2].trim().to_string())),
            "MEMBERS" => {
                let nodes = parts[2].trim().split(',').map(|s| s.to_string()).collect();
                Some(ClusterMsg::Members(nodes))
            }
            _ => None,
        }
    }
}

// ── Cluster Node ─────────────────────────────────────────────────────

pub struct ClusterNode {
    pub node_id: String,
    pub ring: Arc<RwLock<HashRing>>,
    pub peers: Arc<Mutex<HashMap<String, TcpStream>>>,
    /// Pending get/values requests: req_id → sender
    pub pending: Arc<Mutex<HashMap<String, std::sync::mpsc::Sender<String>>>>,
    request_counter: Arc<Mutex<u64>>,
}

impl ClusterNode {
    pub fn new(node_id: &str) -> Self {
        let mut ring = HashRing::new();
        ring.add_node(node_id);
        Self {
            node_id: node_id.to_string(),
            ring: Arc::new(RwLock::new(ring)),
            peers: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            request_counter: Arc::new(Mutex::new(0)),
        }
    }

    pub fn next_req_id(&self) -> String {
        let mut c = self.request_counter.lock().unwrap();
        *c += 1;
        format!("{}_{}", self.node_id, c)
    }

    /// Connect to a seed node and join the cluster.
    pub fn join_cluster(&self, seed: &str) -> Result<(), String> {
        use std::net::ToSocketAddrs;
        // Resolve hostname (handles localhost → 127.0.0.1 + ::1)
        let stream = TcpStream::connect(seed)
            .map_err(|e| format!("cannot connect to {}: {}", seed, e))?;
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok();

        // Send JOIN
        let mut writer = stream.try_clone().map_err(|e| format!("{}", e))?;
        let msg = ClusterMsg::Join(self.node_id.clone()).encode();
        writer.write_all(msg.as_bytes()).map_err(|e| format!("{}", e))?;
        writer.flush().ok();

        // Wait briefly for MEMBERS response
        let reader = std::io::BufReader::new(stream.try_clone().map_err(|e| format!("{}", e))?);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if let Some(ClusterMsg::Members(nodes)) = ClusterMsg::decode(&line) {
                        let mut ring = self.ring.write().unwrap();
                        for node in &nodes {
                            ring.add_node(node);
                        }
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // Add seed to ring
        self.ring.write().unwrap().add_node(seed);

        // Store peer connection for future use
        self.peers.lock().unwrap().insert(seed.to_string(), writer);

        Ok(())
    }

    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        let ring = self.ring.read().unwrap();
        ring.leader() == Some(&self.node_id)
    }

    /// Check if a key belongs to this node
    pub fn owns_key(&self, key: &str) -> bool {
        let ring = self.ring.read().unwrap();
        ring.get_node(key) == Some(&self.node_id)
    }

    /// Number of nodes in the cluster
    pub fn node_count(&self) -> usize {
        self.ring.read().unwrap().node_count()
    }
}
