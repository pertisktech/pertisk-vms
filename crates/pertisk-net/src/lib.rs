//! Virtual networks: inventory, IPv4 AM, TAP/bridge provisioning.

mod host;
mod ipam;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use pertisk_types::{CreateNetworkRequest, NetSpec, NetworkId, NetworkRecord, VmId};
use serde::{Deserialize, Serialize};

pub use host::{delete_tap, provision_nic};
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
            serde_json::from_str(&std::fs::read_to_string(&inventory_path)?)?
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
        let net = Ipv4Net::parse(&req.cidr)?;
        let gateway = match req.gateway {
            Some(gw) => Some(parse_ipv4(&gw)?),
            None => Some(net.nth(1)),
        };
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
            .unwrap_or_else(|| default_bridge(&req.name, existing));
        if !valid_ifname(&bridge) {
            return Err(NetError::Invalid(format!(
                "invalid bridge name '{bridge}' (max 15 [A-Za-z0-9_-])"
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
        };
        if self.apply_host_links {
            let prefix = net.prefix;
            host::ensure_bridge(&record.bridge, record.gateway.as_deref(), prefix)?;
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
    ) -> Result<NetSpec> {
        let network = self.get(network_id)?;
        let net = Ipv4Net::parse(&network.cidr)?;
        let tap = tap_name(vm_id, nic_index);
        if !valid_ifname(&tap) {
            return Err(NetError::Invalid(format!("invalid tap name '{tap}'")));
        }
        let ip = if let Some(ip) = requested_ip {
            let addr = parse_ipv4(ip)?;
            if !net.contains(addr) {
                return Err(NetError::Invalid(format!(
                    "{ip} is not in {}",
                    network.cidr
                )));
            }
            if used_ips.iter().any(|used| used == ip) {
                return Err(NetError::Invalid(format!("{ip} already in use")));
            }
            Some(ipam::ipv4_string(addr))
        } else if network.dhcp {
            Some(ipam::ipv4_string(
                net.allocate(network.gateway.as_deref(), used_ips)?,
            ))
        } else {
            None
        };
        if self.apply_host_links {
            host::provision_nic(
                &network.bridge,
                &tap,
                network.gateway.as_deref(),
                net.prefix,
                network.isolate,
            )?;
        }
        Ok(NetSpec {
            network_id: Some(network_id),
            tap: Some(tap),
            mac: Some(guest_mac(vm_id, nic_index)),
            ip,
        })
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
            })
            .unwrap();
        let vm = VmId::new();
        let nic = pool.allocate_nic(net.id, vm, 0, None, &[]).unwrap();
        assert!(nic.tap.unwrap().starts_with('p'));
        assert_eq!(nic.ip.as_deref(), Some("10.88.0.2"));
        let nic2 = pool
            .allocate_nic(net.id, vm, 1, None, &[nic.ip.clone().unwrap()])
            .unwrap();
        assert_eq!(nic2.ip.as_deref(), Some("10.88.0.3"));
    }
}
