//! Membership, quorum, fencing, and placement.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pertisk_types::{
    ClusterMemberStatus, ClusterSnapshot, ClusterStatus, HeartbeatMessage, HostConfig, NodeId,
    NodeRecord, VmSpec,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::DaemonError;

#[derive(Clone, Debug)]
pub struct NodeLoad {
    pub id: NodeId,
    pub online: bool,
    pub cpus: u32,
    pub memory_mib: u32,
    pub used_vcpus: u32,
    pub used_memory_mib: u32,
}

pub fn has_quorum(online: usize, total: usize) -> bool {
    total > 0 && online * 2 > total
}

pub fn schedule(nodes: &[NodeLoad], spec: &VmSpec, prefer: Option<NodeId>) -> Option<NodeId> {
    let fits = |n: &NodeLoad| {
        n.online
            && n.cpus.saturating_sub(n.used_vcpus) >= u32::from(spec.vcpus)
            && n.memory_mib.saturating_sub(n.used_memory_mib) >= spec.memory_mib
    };
    if let Some(id) = prefer
        && nodes.iter().any(|n| n.id == id && fits(n))
    {
        return Some(id);
    }
    nodes
        .iter()
        .filter(|n| fits(n))
        .min_by_key(|n| {
            let cpu = n.used_vcpus.saturating_mul(1_000) / n.cpus.max(1);
            let mem = n.used_memory_mib.saturating_mul(1_000) / n.memory_mib.max(1);
            (cpu + mem, n.id)
        })
        .map(|n| n.id)
}

/// Prefer nodes that already hold volume replicas; fall back to least-loaded.
pub fn schedule_storage(
    nodes: &[NodeLoad],
    spec: &VmSpec,
    prefer: Option<NodeId>,
    affinity: &[NodeId],
) -> Option<NodeId> {
    if let Some(id) = schedule(nodes, spec, prefer)
        && (affinity.is_empty() || affinity.contains(&id))
    {
        return Some(id);
    }
    let fits = |n: &NodeLoad| {
        n.online
            && n.cpus.saturating_sub(n.used_vcpus) >= u32::from(spec.vcpus)
            && n.memory_mib.saturating_sub(n.used_memory_mib) >= spec.memory_mib
    };
    let local: Vec<_> = nodes
        .iter()
        .filter(|n| fits(n) && affinity.contains(&n.id))
        .cloned()
        .collect();
    if !local.is_empty() {
        return schedule(&local, spec, prefer);
    }
    schedule(nodes, spec, prefer)
}

pub fn place_replicas(online: &[NodeId], count: u8, include: Option<NodeId>) -> Vec<NodeId> {
    let want = usize::from(count.max(1)).min(online.len().max(1));
    let mut out = Vec::new();
    if let Some(id) = include
        && online.contains(&id)
    {
        out.push(id);
    }
    for id in online {
        if out.len() >= want {
            break;
        }
        if !out.contains(id) {
            out.push(*id);
        }
    }
    if out.is_empty()
        && let Some(id) = include
    {
        out.push(id);
    }
    out
}

pub fn advertise_url(listen: &str, explicit: Option<&str>) -> String {
    if let Some(url) = explicit {
        return url.trim_end_matches('/').to_string();
    }
    let rewritten = listen
        .replace("0.0.0.0", "127.0.0.1")
        .replace("[::]", "[::1]");
    if rewritten.starts_with("http://") || rewritten.starts_with("https://") {
        rewritten
    } else {
        format!("http://{rewritten}")
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn default_cpus(override_n: Option<u32>) -> u32 {
    override_n.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4)
    })
}

fn default_memory_mib(override_n: Option<u32>) -> u32 {
    override_n.unwrap_or(16_384)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Persisted {
    self_id: NodeId,
    name: String,
    secret: String,
    generation: u64,
    members: Vec<NodeRecord>,
}

struct MemberState {
    record: NodeRecord,
    last_seen_ms: u64,
}

struct Inner {
    self_id: NodeId,
    name: String,
    secret: String,
    generation: u64,
    members: BTreeMap<NodeId, MemberState>,
    fenced: bool,
}

pub struct Cluster {
    path: PathBuf,
    heartbeat_ms: u64,
    offline_after_ms: u64,
    inner: Mutex<Inner>,
}

impl Cluster {
    pub fn open(
        path: impl AsRef<Path>,
        config: &HostConfig,
        listen: &str,
    ) -> Result<Self, DaemonError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let peer_url = advertise_url(listen, config.cluster.peer_url.as_deref());
        let node_name = config
            .cluster
            .node_name
            .clone()
            .unwrap_or_else(|| "node".into());
        let cpus = default_cpus(config.cluster.cpus);
        let memory_mib = default_memory_mib(config.cluster.memory_mib);
        let now = now_ms();
        let inner = if path.exists() {
            let persisted: Persisted = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
            let mut members = BTreeMap::new();
            for record in persisted.members {
                let last_seen_ms = if record.id == persisted.self_id {
                    now
                } else {
                    0
                };
                members.insert(
                    record.id,
                    MemberState {
                        record,
                        last_seen_ms,
                    },
                );
            }
            if let Some(self_member) = members.get_mut(&persisted.self_id) {
                self_member.record.peer_url = peer_url;
                self_member.record.name = node_name;
                self_member.record.cpus = cpus;
                self_member.record.memory_mib = memory_mib;
                self_member.last_seen_ms = now;
            }
            Inner {
                self_id: persisted.self_id,
                name: persisted.name,
                secret: persisted.secret,
                generation: persisted.generation,
                members,
                fenced: false,
            }
        } else {
            let self_id = NodeId::new();
            let record = NodeRecord {
                id: self_id,
                name: node_name,
                peer_url,
                cpus,
                memory_mib,
            };
            let mut members = BTreeMap::new();
            members.insert(
                self_id,
                MemberState {
                    record,
                    last_seen_ms: now,
                },
            );
            Inner {
                self_id,
                name: config.cluster.name.clone(),
                secret: format!(
                    "{}{}",
                    Uuid::new_v4().as_simple(),
                    Uuid::new_v4().as_simple()
                ),
                generation: 1,
                members,
                fenced: false,
            }
        };
        let cluster = Self {
            path,
            heartbeat_ms: config.cluster.heartbeat_ms.max(50),
            offline_after_ms: config.cluster.offline_after_ms.max(100),
            inner: Mutex::new(inner),
        };
        cluster.persist()?;
        Ok(cluster)
    }

    pub fn heartbeat_ms(&self) -> u64 {
        self.heartbeat_ms
    }

    pub fn self_id(&self) -> NodeId {
        self.inner.lock().expect("cluster lock").self_id
    }

    pub fn secret(&self) -> String {
        self.inner.lock().expect("cluster lock").secret.clone()
    }

    pub fn check_secret(&self, got: &str) -> bool {
        self.inner.lock().expect("cluster lock").secret == got
    }

    pub fn self_record(&self) -> NodeRecord {
        let inner = self.inner.lock().expect("cluster lock");
        inner
            .members
            .get(&inner.self_id)
            .map(|m| m.record.clone())
            .unwrap_or_else(|| NodeRecord {
                id: inner.self_id,
                name: "node".into(),
                peer_url: String::new(),
                cpus: 4,
                memory_mib: 16_384,
            })
    }

    pub fn set_peer_url(&self, url: String) -> Result<(), DaemonError> {
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            let id = inner.self_id;
            if let Some(member) = inner.members.get_mut(&id) {
                member.record.peer_url = url;
            }
        }
        self.persist()
    }

    pub fn generation(&self) -> u64 {
        self.inner.lock().expect("cluster lock").generation
    }

    pub fn bump(&self) -> Result<(), DaemonError> {
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            inner.generation += 1;
        }
        self.persist()
    }

    pub fn peer_urls_except_self(&self) -> Vec<(NodeId, String)> {
        let inner = self.inner.lock().expect("cluster lock");
        inner
            .members
            .values()
            .filter(|m| m.record.id != inner.self_id)
            .map(|m| (m.record.id, m.record.peer_url.clone()))
            .collect()
    }

    pub fn member_url(&self, id: NodeId) -> Option<String> {
        let inner = self.inner.lock().expect("cluster lock");
        inner.members.get(&id).map(|m| m.record.peer_url.clone())
    }

    pub fn touch(&self, id: NodeId, record: Option<NodeRecord>) {
        let mut inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        if let Some(record) = record {
            inner.members.insert(
                id,
                MemberState {
                    record,
                    last_seen_ms: now,
                },
            );
        } else if let Some(member) = inner.members.get_mut(&id) {
            member.last_seen_ms = now;
        }
    }

    pub fn touch_self(&self) {
        let id = self.self_id();
        self.touch(id, None);
    }

    pub fn online_ids(&self) -> Vec<NodeId> {
        let inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        let timeout = self.offline_after_ms;
        inner
            .members
            .values()
            .filter(|m| now.saturating_sub(m.last_seen_ms) <= timeout)
            .map(|m| m.record.id)
            .collect()
    }

    pub fn has_quorum(&self) -> bool {
        let inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        let online = inner
            .members
            .values()
            .filter(|m| now.saturating_sub(m.last_seen_ms) <= self.offline_after_ms)
            .count();
        has_quorum(online, inner.members.len())
    }

    pub fn is_leader(&self) -> bool {
        let online = self.online_ids();
        match online.iter().min().copied() {
            Some(id) => id == self.self_id(),
            None => false,
        }
    }

    pub fn leader_id(&self) -> Option<NodeId> {
        self.online_ids().into_iter().min()
    }

    pub fn is_fenced(&self) -> bool {
        self.inner.lock().expect("cluster lock").fenced
    }

    /// Returns true if we just entered the fenced state.
    pub fn set_fenced(&self, fenced: bool) -> bool {
        let mut inner = self.inner.lock().expect("cluster lock");
        let entered = fenced && !inner.fenced;
        inner.fenced = fenced;
        entered
    }

    pub fn add_member(&self, record: NodeRecord) -> Result<(), DaemonError> {
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            inner.members.insert(
                record.id,
                MemberState {
                    record,
                    last_seen_ms: now_ms(),
                },
            );
            inner.generation += 1;
        }
        self.persist()
    }

    pub fn reset_solo(&self) -> Result<(), DaemonError> {
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            let self_id = inner.self_id;
            inner.members.retain(|id, _| *id == self_id);
            inner.generation += 1;
            inner.fenced = false;
        }
        self.persist()
    }

    pub fn apply_membership(&self, snap: &ClusterSnapshot) -> Result<(), DaemonError> {
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            if snap.generation < inner.generation {
                return Ok(());
            }
            inner.name = snap.name.clone();
            inner.secret = snap.secret.clone();
            inner.generation = snap.generation;
            let now = now_ms();
            let mut next = BTreeMap::new();
            for record in &snap.members {
                let last_seen_ms = inner
                    .members
                    .get(&record.id)
                    .map(|m| m.last_seen_ms)
                    .unwrap_or(0);
                let last_seen_ms = if record.id == inner.self_id {
                    now
                } else {
                    last_seen_ms
                };
                next.insert(
                    record.id,
                    MemberState {
                        record: record.clone(),
                        last_seen_ms,
                    },
                );
            }
            if !next.contains_key(&inner.self_id) {
                let self_id = inner.self_id;
                if let Some(self_member) = inner.members.remove(&self_id) {
                    next.insert(self_id, self_member);
                }
            }
            inner.members = next;
        }
        self.persist()
    }

    pub fn membership_snapshot(&self) -> ClusterSnapshot {
        let inner = self.inner.lock().expect("cluster lock");
        ClusterSnapshot {
            name: inner.name.clone(),
            secret: inner.secret.clone(),
            generation: inner.generation,
            members: inner.members.values().map(|m| m.record.clone()).collect(),
            vms: vec![],
            volumes: vec![],
        }
    }

    pub fn heartbeat_out(&self, include_snapshot: bool) -> HeartbeatMessage {
        let member = self.self_record();
        let inner = self.inner.lock().expect("cluster lock");
        HeartbeatMessage {
            from: inner.self_id,
            generation: inner.generation,
            member,
            snapshot: include_snapshot.then(|| ClusterSnapshot {
                name: inner.name.clone(),
                secret: inner.secret.clone(),
                generation: inner.generation,
                members: inner.members.values().map(|m| m.record.clone()).collect(),
                vms: vec![],
                volumes: vec![],
            }),
        }
    }

    pub fn status(&self, loads: &[NodeLoad]) -> ClusterStatus {
        let inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        let members: Vec<ClusterMemberStatus> = inner
            .members
            .values()
            .map(|m| {
                let online = now.saturating_sub(m.last_seen_ms) <= self.offline_after_ms;
                let load = loads.iter().find(|l| l.id == m.record.id);
                ClusterMemberStatus {
                    id: m.record.id,
                    name: m.record.name.clone(),
                    peer_url: m.record.peer_url.clone(),
                    online,
                    cpus: m.record.cpus,
                    memory_mib: m.record.memory_mib,
                    used_vcpus: load.map(|l| l.used_vcpus).unwrap_or(0),
                    used_memory_mib: load.map(|l| l.used_memory_mib).unwrap_or(0),
                }
            })
            .collect();
        let online = members.iter().filter(|m| m.online).count();
        let quorum = has_quorum(online, members.len());
        let leader_id = members.iter().filter(|m| m.online).map(|m| m.id).min();
        ClusterStatus {
            name: inner.name.clone(),
            generation: inner.generation,
            self_id: inner.self_id,
            leader_id,
            quorum,
            fenced: inner.fenced || !quorum,
            members,
        }
    }

    fn persist(&self) -> Result<(), DaemonError> {
        let inner = self.inner.lock().expect("cluster lock");
        let persisted = Persisted {
            self_id: inner.self_id,
            name: inner.name.clone(),
            secret: inner.secret.clone(),
            generation: inner.generation,
            members: inner.members.values().map(|m| m.record.clone()).collect(),
        };
        drop(inner);
        let json = serde_json::to_vec_pretty(&persisted)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::VmSpec;

    fn spec() -> VmSpec {
        VmSpec {
            name: "vm".into(),
            vcpus: 2,
            memory_mib: 1024,
            kernel: None,
            cmdline: None,
            initramfs: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
            ha: true,
        }
    }

    fn load(_id: u8, used_vcpus: u32) -> NodeLoad {
        NodeLoad {
            id: NodeId::new(),
            online: true,
            cpus: 8,
            memory_mib: 16_384,
            used_vcpus,
            used_memory_mib: used_vcpus * 512,
        }
    }

    #[test]
    fn majority_quorum() {
        assert!(has_quorum(1, 1));
        assert!(!has_quorum(1, 2));
        assert!(has_quorum(2, 3));
        assert!(!has_quorum(1, 3));
        assert!(has_quorum(3, 4));
        assert!(!has_quorum(2, 4));
    }

    #[test]
    fn least_loaded_fits() {
        let a = load(1, 6);
        let b = load(2, 1);
        let id_a = a.id;
        let id_b = b.id;
        let picked = schedule(&[a, b], &spec(), None).unwrap();
        assert_eq!(picked, id_b);
        let _ = id_a;
    }

    #[test]
    fn replica_placement_includes_self() {
        let a = NodeId::new();
        let b = NodeId::new();
        let c = NodeId::new();
        let placed = place_replicas(&[a, b, c], 2, Some(a));
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0], a);
    }

    #[test]
    fn persist_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = HostConfig::default_for(dir.path());
        config.cluster.node_name = Some("alpha".into());
        config.cluster.cpus = Some(8);
        let path = dir.path().join("cluster.json");
        let cluster = Cluster::open(&path, &config, "127.0.0.1:7480").unwrap();
        let id = cluster.self_id();
        drop(cluster);
        let reopened = Cluster::open(&path, &config, "127.0.0.1:7480").unwrap();
        assert_eq!(reopened.self_id(), id);
        assert_eq!(reopened.self_record().name, "alpha");
        assert!(reopened.has_quorum());
        assert!(reopened.is_leader());
    }
}
