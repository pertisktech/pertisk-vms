//! Membership, quorum, fencing, and placement.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pertisk_types::{
    ClusterMemberStatus, ClusterSnapshot, ClusterStatus, HeartbeatMessage, HostConfig, NodeId,
    NodeRecord, VmSpec, probe_host_addrs,
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

/// vCPU overcommit vs advertised host threads. Memory stays a hard cap.
const CPU_OVERCOMMIT: u32 = 8;

fn cpu_capacity(n: &NodeLoad) -> u32 {
    n.cpus.saturating_mul(CPU_OVERCOMMIT).max(n.cpus)
}

/// RAM kept for the host / VMM. Scales down on small appliances so a ~1 GiB
/// node can still run a 1024 MiB cloud guest (832–1088 MiB class boxes).
pub fn host_memory_reserve_mib(host_mib: u32) -> u32 {
    (host_mib / 20).clamp(64, 1536)
}

/// Guest start/placement budget: installed RAM minus host reserve.
pub fn guest_memory_budget_mib(host_mib: u32) -> u32 {
    host_mib.saturating_sub(host_memory_reserve_mib(host_mib))
}

/// True when a guest of `need_mib` can start given host RAM and already-running guests.
pub fn guest_start_fits(host_mib: u64, running_mib: u64, need_mib: u64) -> bool {
    let host_u32 = u32::try_from(host_mib.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
    let reserve = u64::from(host_memory_reserve_mib(host_u32));
    running_mib.saturating_add(need_mib) <= host_mib.saturating_sub(reserve)
}

fn guest_memory_capacity(n: &NodeLoad) -> u32 {
    guest_memory_budget_mib(n.memory_mib)
}

fn node_fits(n: &NodeLoad, spec: &VmSpec) -> bool {
    n.online
        && cpu_capacity(n).saturating_sub(n.used_vcpus) >= u32::from(spec.vcpus)
        && guest_memory_capacity(n).saturating_sub(n.used_memory_mib) >= spec.memory_mib
}

pub fn schedule(nodes: &[NodeLoad], spec: &VmSpec, prefer: Option<NodeId>) -> Option<NodeId> {
    let fits = |n: &NodeLoad| node_fits(n, spec);
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
            let mem_cap = guest_memory_capacity(n).max(1);
            let mem = n.used_memory_mib.saturating_mul(1_000) / mem_cap;
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
    let fits = |n: &NodeLoad| node_fits(n, spec);
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

/// Place a defined (not running) guest: any online node, prefer replica holders.
pub fn schedule_define(
    nodes: &[NodeLoad],
    prefer: Option<NodeId>,
    affinity: &[NodeId],
) -> Option<NodeId> {
    let online: Vec<_> = nodes.iter().filter(|n| n.online).cloned().collect();
    if online.is_empty() {
        return None;
    }
    let prefer_ok = |id: NodeId| online.iter().any(|n| n.id == id);
    if let Some(id) = prefer
        && prefer_ok(id)
    {
        return Some(id);
    }
    if let Some(id) = affinity.iter().copied().find(|id| prefer_ok(*id)) {
        return Some(id);
    }
    online
        .iter()
        .min_by_key(|n| (n.used_memory_mib, n.used_vcpus, n.id))
        .map(|n| n.id)
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
    advertise_peer_url(listen, None, explicit)
}

/// Cluster URL other nodes should dial. Unspecified binds (`0.0.0.0` / `::`)
/// advertise a LAN address, not loopback — otherwise join/heartbeat stay local.
pub fn advertise_peer_url(
    listen: &str,
    tls_listen: Option<&str>,
    explicit: Option<&str>,
) -> String {
    if let Some(url) = explicit {
        let url = url.trim().trim_end_matches('/');
        if !url.is_empty() {
            return url.to_string();
        }
    }
    if let Some(tls) = tls_listen.map(str::trim).filter(|s| !s.is_empty()) {
        let (host, port) = split_listen(tls);
        return format!(
            "https://{}:{port}",
            format_url_host(&resolve_advertise_host(&host))
        );
    }
    let (host, port) = split_listen(listen);
    format!(
        "http://{}:{port}",
        format_url_host(&resolve_advertise_host(&host))
    )
}

pub fn is_loopback_peer_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.contains("127.0.0.1")
        || lower.contains("localhost")
        || lower.contains("[::1]")
        || lower.contains("://[::1]")
        || lower.contains("0.0.0.0")
}

fn split_listen(listen: &str) -> (String, String) {
    let listen = listen.trim();
    if let Some(rest) = listen.strip_prefix('[')
        && let Some((host, port)) = rest.split_once("]:")
    {
        return (host.to_string(), port.to_string());
    }
    if let Some((host, port)) = listen.rsplit_once(':') {
        return (host.to_string(), port.to_string());
    }
    (listen.to_string(), "7480".into())
}

fn resolve_advertise_host(host: &str) -> String {
    let host = host.trim().trim_matches(['[', ']']);
    if host == "0.0.0.0" || host == "::" {
        return lan_advertise_host().unwrap_or_else(|| "127.0.0.1".into());
    }
    host.to_string()
}

fn lan_advertise_host() -> Option<String> {
    let addrs = probe_host_addrs();
    addrs
        .ipv4
        .into_iter()
        .next()
        .or_else(|| addrs.ipv6.into_iter().next())
}

fn format_url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn rewrite_peer_host(template: &str, host: &str) -> Option<String> {
    let url = template.trim().trim_end_matches('/');
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let scheme = if url.starts_with("https://") {
        "https"
    } else {
        "http"
    };
    let port = rest
        .rsplit_once(':')
        .map(|(_, p)| p)
        .filter(|p| !p.is_empty())?;
    Some(format!("{scheme}://{}:{port}", format_url_host(host)))
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
    if let Some(n) = override_n {
        return n;
    }
    advertised_host_memory_mib().unwrap_or(16_384)
}

fn advertised_host_memory_mib() -> Option<u32> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("MemTotal:") else {
            continue;
        };
        let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
        // Advertise installed RAM for UI / inventory. Placement keeps a small
        // host reserve via host_memory_reserve_mib().
        return Some((kb / 1024) as u32);
    }
    None
}

fn member_is_online(member: &MemberState, self_id: NodeId, now: u64, timeout: u64) -> bool {
    member.record.id == self_id || now.saturating_sub(member.last_seen_ms) <= timeout
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
        let inner = if path.exists() && !std::fs::read_to_string(&path)?.trim().is_empty() {
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
                ipv4: Vec::new(),
                ipv6: Vec::new(),
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
                ipv4: Vec::new(),
                ipv6: Vec::new(),
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

    pub fn set_member_peer_url(&self, id: NodeId, url: String) -> Result<(), DaemonError> {
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            if let Some(member) = inner.members.get_mut(&id) {
                member.record.peer_url = url;
            }
        }
        self.persist()
    }

    /// When this node has a LAN URL, rewrite other members still advertising loopback
    /// to `scheme://<their ipv4>:port` so a joined cluster can recover after upgrade.
    pub fn heal_remote_peer_urls(&self) -> Result<bool, DaemonError> {
        let self_url = self.self_record().peer_url;
        if is_loopback_peer_url(&self_url) {
            return Ok(false);
        }
        let mut changed = false;
        {
            let mut inner = self.inner.lock().expect("cluster lock");
            let self_id = inner.self_id;
            for (id, member) in inner.members.iter_mut() {
                if *id == self_id || !is_loopback_peer_url(&member.record.peer_url) {
                    continue;
                }
                let host = member
                    .record
                    .ipv4
                    .first()
                    .cloned()
                    .or_else(|| member.record.ipv6.first().cloned());
                let Some(host) = host else {
                    continue;
                };
                let Some(url) = rewrite_peer_host(&self_url, &host) else {
                    continue;
                };
                member.record.peer_url = url;
                changed = true;
            }
        }
        if changed {
            self.persist()?;
        }
        Ok(changed)
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

    pub fn peer_urls_online_except_self(&self) -> Vec<(NodeId, String)> {
        let online = self.online_ids();
        self.peer_urls_except_self()
            .into_iter()
            .filter(|(id, _)| online.contains(id))
            .collect()
    }

    pub fn member_url(&self, id: NodeId) -> Option<String> {
        let inner = self.inner.lock().expect("cluster lock");
        inner.members.get(&id).map(|m| m.record.peer_url.clone())
    }

    pub fn touch(&self, id: NodeId, record: Option<NodeRecord>) {
        let mut inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        if let Some(mut record) = record {
            if is_loopback_peer_url(&record.peer_url)
                && let Some(existing) = inner.members.get(&id)
                && !is_loopback_peer_url(&existing.record.peer_url)
            {
                record.peer_url = existing.record.peer_url.clone();
            }
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
            .filter(|m| member_is_online(m, inner.self_id, now, timeout))
            .map(|m| m.record.id)
            .collect()
    }

    pub fn has_quorum(&self) -> bool {
        let inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        let online = inner
            .members
            .values()
            .filter(|m| member_is_online(m, inner.self_id, now, self.offline_after_ms))
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
            for (id, member) in &inner.members {
                next.entry(*id).or_insert_with(|| MemberState {
                    record: member.record.clone(),
                    last_seen_ms: member.last_seen_ms,
                });
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

    fn refresh_self_addrs(&self) {
        let addrs = probe_host_addrs();
        let mut inner = self.inner.lock().expect("cluster lock");
        let self_id = inner.self_id;
        if let Some(member) = inner.members.get_mut(&self_id) {
            member.record.ipv4 = addrs.ipv4;
            member.record.ipv6 = addrs.ipv6;
        }
    }

    pub fn heartbeat_out(&self, include_snapshot: bool) -> HeartbeatMessage {
        self.refresh_self_addrs();
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
        self.refresh_self_addrs();
        let inner = self.inner.lock().expect("cluster lock");
        let now = now_ms();
        let members: Vec<ClusterMemberStatus> = inner
            .members
            .values()
            .map(|m| {
                let online = member_is_online(m, inner.self_id, now, self.offline_after_ms);
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
                    ipv4: m.record.ipv4.clone(),
                    ipv6: m.record.ipv6.clone(),
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
            firmware: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
            console_type: Default::default(),
            ha: true,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
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
    fn overcommits_vcpus_on_small_hosts() {
        let node = NodeLoad {
            id: NodeId::new(),
            online: true,
            cpus: 1,
            memory_mib: 4096,
            used_vcpus: 1,
            used_memory_mib: 512,
        };
        let id = node.id;
        assert_eq!(schedule(&[node], &spec_small(), None), Some(id));
    }

    fn spec_small() -> VmSpec {
        let mut spec = spec();
        spec.vcpus = 1;
        spec.memory_mib = 512;
        spec
    }

    #[test]
    fn rejects_when_vcpu_overcommit_exhausted() {
        let node = NodeLoad {
            id: NodeId::new(),
            online: true,
            cpus: 1,
            memory_mib: 16_384,
            used_vcpus: 8,
            used_memory_mib: 4096,
        };
        assert_eq!(schedule(&[node], &spec_small(), None), None);
    }

    #[test]
    fn advertises_full_memtotal_not_three_quarters() {
        let Some(advertised) = advertised_host_memory_mib() else {
            return;
        };
        let text = std::fs::read_to_string("/proc/meminfo").unwrap();
        let mut total_kb = 0u64;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kb = rest.split_whitespace().next().unwrap().parse().unwrap();
                break;
            }
        }
        let full = (total_kb / 1024) as u32;
        assert_eq!(
            advertised, full,
            "cluster must advertise installed RAM, not 3/4 headroom"
        );
        assert_ne!(advertised, full.saturating_mul(3) / 4);
    }

    #[test]
    fn guest_memory_capacity_fits_1024_on_1088() {
        let n = NodeLoad {
            id: NodeId::new(),
            online: true,
            cpus: 4,
            memory_mib: 1088,
            used_vcpus: 0,
            used_memory_mib: 0,
        };
        assert_eq!(host_memory_reserve_mib(1088), 64);
        assert_eq!(guest_memory_capacity(&n), 1024);
        let spec = VmSpec {
            name: "cloud".into(),
            vcpus: 1,
            memory_mib: 1024,
            kernel: None,
            cmdline: None,
            initramfs: None,
            firmware: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
            console_type: Default::default(),
            ha: false,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
        };
        assert!(node_fits(&n, &spec));
        assert!(guest_start_fits(1088, 0, 1024));
        assert!(!guest_start_fits(1088, 0, 1025));
        assert!(!guest_start_fits(1088, 832, 1024));
    }

    #[test]
    fn schedule_define_ignores_guest_ram() {
        let n = NodeLoad {
            id: NodeId::new(),
            online: true,
            cpus: 2,
            memory_mib: 1088,
            used_vcpus: 0,
            used_memory_mib: 0,
        };
        let id = n.id;
        let spec = VmSpec {
            name: "cloud".into(),
            vcpus: 1,
            memory_mib: 4096,
            kernel: None,
            cmdline: None,
            initramfs: None,
            firmware: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
            console_type: Default::default(),
            ha: false,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
        };
        assert_eq!(schedule(&[n.clone()], &spec, None), None);
        assert_eq!(schedule_define(&[n], None, &[]), Some(id));
    }

    #[test]
    fn guest_memory_capacity_caps_reserve_on_large_hosts() {
        let n = NodeLoad {
            id: NodeId::new(),
            online: true,
            cpus: 8,
            memory_mib: 32_768,
            used_vcpus: 0,
            used_memory_mib: 0,
        };
        assert_eq!(host_memory_reserve_mib(32_768), 1536);
        assert_eq!(guest_memory_capacity(&n), 31_232);
    }

    #[test]
    fn advertise_url_defaults_to_http() {
        assert_eq!(
            advertise_url("127.0.0.1:7480", None),
            "http://127.0.0.1:7480"
        );
        let public = advertise_url("0.0.0.0:7480", None);
        assert!(public.starts_with("http://"), "{public}");
        assert!(public.ends_with(":7480"), "{public}");
        assert!(!public.contains("0.0.0.0"), "{public}");
        assert_eq!(
            advertise_url("10.1.1.144:7443", Some("https://10.1.1.144:7443")),
            "https://10.1.1.144:7443"
        );
        assert_eq!(
            advertise_peer_url("0.0.0.0:7480", Some("0.0.0.0:7443"), None)
                .split("://")
                .next(),
            Some("https")
        );
    }

    #[test]
    fn loopback_peer_url_detection() {
        assert!(is_loopback_peer_url("http://127.0.0.1:7480"));
        assert!(is_loopback_peer_url("https://localhost:7443"));
        assert!(!is_loopback_peer_url("https://10.1.1.144:7443"));
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
        let status = reopened.status(&[]);
        assert_eq!(status.members.len(), 1);
        assert!(reopened.has_quorum());
        assert!(reopened.is_leader());
    }

    #[test]
    fn self_stays_schedulable_after_heartbeat_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = HostConfig::default_for(dir.path());
        config.cluster.offline_after_ms = 50;
        config.cluster.cpus = Some(1);
        let path = dir.path().join("cluster.json");
        let cluster = Cluster::open(&path, &config, "127.0.0.1:7480").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(cluster.has_quorum());
        let status = cluster.status(&[]);
        assert!(status.members.iter().all(|m| m.online));
    }
}
