//! Virtual networks: inventory, IPv4 AM, TAP/bridge provisioning.

mod host;
mod ipam;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use pertisk_types::{CreateNetworkRequest, NetSpec, NetworkId, NetworkMode, NetworkRecord, VmId};
use serde::{Deserialize, Serialize};

pub use host::{
    delete_tap, ipv4_for_mac, ipv6_for_mac, normalize_mac, probe_bridge_neighbors, provision_nic,
};
pub use ipam::{Ipv4Net, parse_cidr, parse_ipv4};

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("network not found: {0}")]
    NotFound(NetworkId),
    #[error("network name already exists: {0}")]
    NameTaken(String),
    #[error("invalid network: {0}")]
    Invalid(String),
    #[error("address pool exhausted for {0}")]
    PoolExhausted(String),
    #[error("{0}")]
    Host(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, NetError>;

#[derive(Debug, Serialize, Deserialize, Default)]
struct Inventory {
    networks: BTreeMap<NetworkId, NetworkRecord>,
}

#[derive(Debug)]
pub struct NetworkPool {
    apply_host_links: bool,
    inner: Mutex<Inventory>,
    inventory_path: PathBuf,
}

impl NetworkPool {
    pub fn open(root: impl Into<PathBuf>, apply_host_links: bool) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let inventory_path = root.join("networks.json");
        let inner = if inventory_path.exists() {
            let text = std::fs::read_to_string(&inventory_path)?;
            if text.trim().is_empty() {
                Inventory::default()
            } else {
                serde_json::from_str(&text)?
            }
        } else {
            Inventory::default()
        };
        Ok(Self {
            apply_host_links,
            inner: Mutex::new(inner),
            inventory_path,
        })
    }

    pub fn apply_host_links(&self) -> bool {
        self.apply_host_links
    }

    pub fn list(&self) -> Result<Vec<NetworkRecord>> {
        let inner = self.inner.lock().expect("net lock");
        Ok(inner.networks.values().cloned().collect())
    }

    pub fn get(&self, id: NetworkId) -> Result<NetworkRecord> {
        self.inner
            .lock()
            .expect("net lock")
            .networks
            .get(&id)
            .cloned()
            .ok_or(NetError::NotFound(id))
    }

    pub fn create(&self, req: CreateNetworkRequest) -> Result<NetworkRecord> {
        if req.name.trim().is_empty() {
            return Err(NetError::Invalid("network name is required".into()));
        }
        {
            let inner = self.inner.lock().expect("net lock");
            if inner.networks.values().any(|n| n.name == req.name) {
                return Err(NetError::NameTaken(req.name));
            }
        }
        let id = NetworkId::new();
        let existing = inner_count(self)?;
        let bridge = req
            .bridge
            .clone()
            .unwrap_or_else(|| default_bridge(&req.name, existing));
        if !valid_ifname(&bridge) {
            return Err(NetError::Invalid(format!(
                "invalid bridge name '{bridge}' (max 15 [A-Za-z0-9_-])"
            )));
        }

        if req.mode == NetworkMode::Bridge {
            if self.apply_host_links {
                host::ensure_lan_bridge(&bridge)?;
            }
            let record = NetworkRecord {
                id,
                name: req.name,
                bridge,
                cidr: if req.cidr.trim().is_empty() {
                    "0.0.0.0/0".into()
                } else {
                    req.cidr
                },
                gateway: req.gateway,
                dhcp: req.dhcp,
                isolate: req.isolate,
                mode: NetworkMode::Bridge,
            };
            self.upsert(record.clone())?;
            return Ok(record);
        }

        let net = Ipv4Net::parse(&req.cidr)?;
        let gateway = match req.gateway {
            Some(gw) => Some(parse_ipv4(&gw)?),
            None => Some(net.nth(1)),
        };
        if self.apply_host_links && host::interface_exists(&bridge) {
            return Err(NetError::Invalid(format!(
                "bridge name '{bridge}' already exists on this host; choose a new bridge name"
            )));
        }
        if self.apply_host_links && host::overlaps_existing_ipv4(net, None)? {
            return Err(NetError::Invalid(format!(
                "network {} overlaps an IPv4 subnet already configured on this host",
                net.to_cidr_string()
            )));
        }
        let record = NetworkRecord {
            id,
            name: req.name,
            bridge,
            cidr: net.to_cidr_string(),
            gateway: gateway.map(ipam::ipv4_string),
            dhcp: req.dhcp,
            isolate: req.isolate,
            mode: NetworkMode::Nat,
        };
        if self.apply_host_links {
            let prefix = net.prefix;
            host::ensure_bridge(&record.bridge, record.gateway.as_deref(), prefix)?;
            if let Err(err) = host::ensure_ipv4_egress(&record.bridge, net) {
                let _ = host::delete_bridge(&record.bridge);
                return Err(err);
            }
        }
        self.upsert(record.clone())?;
        Ok(record)
    }

    pub fn delete(&self, id: NetworkId) -> Result<()> {
        {
            let mut inner = self.inner.lock().expect("net lock");
            inner.networks.remove(&id).ok_or(NetError::NotFound(id))?;
        }
        self.flush()
    }

    pub fn allocate_nic(
        &self,
        network_id: NetworkId,
        vm_id: VmId,
        nic_index: u8,
        requested_ip: Option<&str>,
        used_ips: &[String],
        used_macs: &[String],
    ) -> Result<NetSpec> {
        let network = self.get(network_id)?;
        let tap = tap_name(vm_id, nic_index);
        if !valid_ifname(&tap) {
            return Err(NetError::Invalid(format!("invalid tap name '{tap}'")));
        }
        let bridged = network.mode == NetworkMode::Bridge;
        let net = if bridged {
            None
        } else {
            Some(Ipv4Net::parse(&network.cidr)?)
        };
        let ip = if let Some(ip) = requested_ip {
            let addr = parse_ipv4(ip)?;
            if let Some(net) = net {
                if !net.contains(addr) {
                    return Err(NetError::Invalid(format!(
                        "{ip} is not in {}",
                        network.cidr
                    )));
                }
            }
            if network.gateway.as_deref() == Some(ip) {
                return Err(NetError::Invalid(format!(
                    "{ip} is reserved as the gateway for {}",
                    network.name
                )));
            }
            if used_ips.iter().any(|used| used == ip) {
                return Err(NetError::Invalid(format!("{ip} already in use")));
            }
            Some(ipam::ipv4_string(addr))
        } else if network.dhcp && !bridged {
            Some(ipam::ipv4_string(
                net.unwrap()
                    .allocate(network.gateway.as_deref(), used_ips)?,
            ))
        } else {
            None
        };
        let spec = NetSpec {
            network_id: Some(network_id),
            tap: Some(tap),
            mac: Some(unique_guest_mac(vm_id, nic_index, used_macs)),
            ip,
            ipv6: None,
        };
        self.ensure_host_links(&spec)?;
        Ok(spec)
    }

    /// Re-create the bridge and TAP for an already-allocated NIC. Both are kernel-only
    /// state that disappears on host reboot, and both `ip` calls are idempotent.
    pub fn ensure_host_links(&self, spec: &NetSpec) -> Result<()> {
        if !self.apply_host_links {
            return Ok(());
        }
        let (Some(network_id), Some(tap)) = (spec.network_id, spec.tap.as_deref()) else {
            return Ok(());
        };
        let network = match self.get(network_id) {
            Ok(network) => network,
            Err(NetError::NotFound(_)) => return Ok(()),
            Err(err) => return Err(err),
        };
        if network.mode == NetworkMode::Bridge {
            host::ensure_lan_bridge(&network.bridge)?;
            return host::provision_nic(&network.bridge, tap, None, 0, network.isolate);
        }
        let net = Ipv4Net::parse(&network.cidr)?;
        if host::overlaps_existing_ipv4(net, Some(&network.bridge))? {
            return Err(NetError::Invalid(format!(
                "network {} overlaps an IPv4 subnet already configured on this host",
                network.cidr
            )));
        }
        host::ensure_bridge(&network.bridge, network.gateway.as_deref(), net.prefix)?;
        host::ensure_ipv4_egress(&network.bridge, net)?;
        host::provision_nic(
            &network.bridge,
            tap,
            network.gateway.as_deref(),
            net.prefix,
            network.isolate,
        )
    }

    pub fn release_nic(&self, spec: &NetSpec) -> Result<()> {
        if self.apply_host_links
            && let Some(tap) = &spec.tap
        {
            host::delete_tap(tap)?;
        }
        Ok(())
    }

    fn upsert(&self, record: NetworkRecord) -> Result<()> {
        {
            let mut inner = self.inner.lock().expect("net lock");
            inner.networks.insert(record.id, record);
        }
        self.flush()
    }

    fn flush(&self) -> Result<()> {
        let inner = self.inner.lock().expect("net lock");
        let json = serde_json::to_vec_pretty(&*inner)?;
        let tmp = self.inventory_path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, &self.inventory_path)?;
        Ok(())
    }
}

fn inner_count(pool: &NetworkPool) -> Result<usize> {
    Ok(pool.inner.lock().expect("net lock").networks.len())
}

pub fn valid_ifname(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 15
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn default_bridge(name: &str, existing: usize) -> String {
    if valid_ifname(name) {
        name.to_string()
    } else {
        format!("vmbr{existing}")
    }
}

pub fn tap_name(vm_id: VmId, nic_index: u8) -> String {
    let hex: String = vm_id.to_string().chars().filter(|c| *c != '-').collect();
    format!("p{}{}", &hex[..8.min(hex.len())], nic_index)
}

pub fn guest_mac(vm_id: VmId, nic_index: u8) -> String {
    let b = vm_id.as_bytes();
    format!(
        "52:54:00:{:02x}:{:02x}:{:02x}",
        b[13],
        b[14],
        b[15] ^ nic_index
    )
}

/// Skip MACs already on other guests. Same VM ID (or 24-bit wrap) would otherwise
/// share a DHCP lease and the same SLAAC address, and DAD would drop SSH.
fn unique_guest_mac(vm_id: VmId, nic_index: u8, used_macs: &[String]) -> String {
    let used: std::collections::HashSet<String> = used_macs
        .iter()
        .filter_map(|mac| host::normalize_mac(mac))
        .collect();
    let mut mac = guest_mac(vm_id, nic_index);
    for _ in 0..(1 << 20) {
        let key = host::normalize_mac(&mac).unwrap_or_else(|| mac.clone());
        if !used.contains(&key) {
            return mac;
        }
        mac = bump_qemu_mac(&mac);
    }
    mac
}

fn bump_qemu_mac(mac: &str) -> String {
    let hex: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let mut bytes = [0u8; 6];
    for (i, chunk) in hex.as_bytes().chunks(2).take(6).enumerate() {
        bytes[i] = u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or("00"), 16).unwrap_or(0);
    }
    bytes[0] = 0x52;
    bytes[1] = 0x54;
    bytes[2] = 0x00;
    let mut tail = u32::from(bytes[3]) << 16 | u32::from(bytes[4]) << 8 | u32::from(bytes[5]);
    tail = (tail + 1) & 0x00ff_ffff;
    format!(
        "52:54:00:{:02x}:{:02x}:{:02x}",
        (tail >> 16) & 0xff,
        (tail >> 8) & 0xff,
        tail & 0xff
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::NetSpec;

    #[test]
    fn copies_tap_from_spec() {
        let spec = NetSpec {
            tap: Some("vmtap0".into()),
            ..NetSpec::default()
        };
        assert_eq!(spec.tap.as_deref(), Some("vmtap0"));
    }

    #[test]
    fn create_and_allocate() {
        let dir = tempfile::tempdir().unwrap();
        let pool = NetworkPool::open(dir.path(), false).unwrap();
        let net = pool
            .create(CreateNetworkRequest {
                name: "lan".into(),
                cidr: "10.88.0.0/24".into(),
                gateway: None,
                bridge: Some("vmbr0".into()),
                dhcp: true,
                isolate: true,
                mode: Default::default(),
            })
            .unwrap();
        let vm = VmId::new();
        let nic = pool.allocate_nic(net.id, vm, 0, None, &[], &[]).unwrap();
        assert!(nic.tap.unwrap().starts_with('p'));
        assert_eq!(nic.ip.as_deref(), Some("10.88.0.2"));
        let nic2 = pool
            .allocate_nic(
                net.id,
                vm,
                1,
                None,
                &[nic.ip.clone().unwrap()],
                &[nic.mac.clone().unwrap()],
            )
            .unwrap();
        assert_eq!(nic2.ip.as_deref(), Some("10.88.0.3"));
    }

    #[test]
    fn bridge_mode_allows_lan_cidr() {
        let dir = tempfile::tempdir().unwrap();
        let pool = NetworkPool::open(dir.path(), false).unwrap();
        let net = pool
            .create(CreateNetworkRequest {
                name: "lan".into(),
                cidr: "10.1.1.0/24".into(),
                gateway: None,
                bridge: Some("br0".into()),
                dhcp: false,
                isolate: false,
                mode: pertisk_types::NetworkMode::Bridge,
            })
            .unwrap();
        assert_eq!(net.mode, pertisk_types::NetworkMode::Bridge);
        assert_eq!(net.bridge, "br0");
        let vm = VmId::new();
        let nic = pool.allocate_nic(net.id, vm, 0, None, &[], &[]).unwrap();
        assert!(nic.ip.is_none());
    }

    #[test]
    fn mac_avoids_duplicate_when_vm_id_wraps_24bit() {
        let dir = tempfile::tempdir().unwrap();
        let pool = NetworkPool::open(dir.path(), false).unwrap();
        let net = pool
            .create(CreateNetworkRequest {
                name: "lan".into(),
                cidr: "10.1.1.0/24".into(),
                gateway: None,
                bridge: Some("br0".into()),
                dhcp: false,
                isolate: false,
                mode: pertisk_types::NetworkMode::Bridge,
            })
            .unwrap();
        let a: VmId = "101".parse().unwrap();
        let b: VmId = "16777317".parse().unwrap();
        assert_eq!(guest_mac(a, 0), guest_mac(b, 0));
        let nic_a = pool.allocate_nic(net.id, a, 0, None, &[], &[]).unwrap();
        let nic_b = pool
            .allocate_nic(net.id, b, 0, None, &[], &[nic_a.mac.clone().unwrap()])
            .unwrap();
        assert_eq!(nic_a.mac.as_deref(), Some("52:54:00:00:00:65"));
        assert_ne!(
            nic_a.mac, nic_b.mac,
            "duplicate MAC would share DHCP + SLAAC"
        );
    }
}
