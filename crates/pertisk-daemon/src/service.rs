use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pertisk_net::{NetError, NetworkPool};
use pertisk_storage::{Rbd, StorageError, VolumePool};
use pertisk_types::{
    AddRepositoryRequest, AptActionResult, AptRepository, AttachDiskRequest, AttachIsoRequest,
    AttachNicRequest, CloneVmRequest, CloneVolumeRequest, CloudInitIsoRequest, CloudInitNetwork,
    ClusterMetrics, ConsoleInfo, ConsoleType, CreateNetworkRequest, CreateTemplateRequest,
    CreateVolumeRequest, DiskSpec, DriverKind, HostConfig, HostInfo, HostPowerResult,
    ImportIsoRequest, IsoRecord, NetworkId, NetworkMode, NetworkRecord, NodeMetrics,
    ResizeVolumeRequest, SerialChunk, SetRepositoryRequest, SnapshotRequest, StorageBackend,
    UpdateVmRequest, UpdatesStatus, VmId, VmMetrics, VmRecord, VmSpec, VmState, VolumeFormat,
    VolumeId, VolumeRecord, default_cloud_user, is_guest_ipv4, probe_host, probe_host_addrs,
};
use pertisk_vmm::VmmBackend;
use thiserror::Error;

use crate::Store;
use crate::cluster::{self, Cluster, NodeLoad};
use crate::console::ConsoleHub;
use crate::control::{AuthUser, ControlError, ControlStore};
use crate::metrics::{self, MetricsCache};

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("vm not found: {0}")]
    NotFound(VmId),
    #[error("vm name already exists: {0}")]
    NameTaken(String),
    #[error("vm id already exists: {0}")]
    IdTaken(VmId),
    #[error("vm {0} must be stopped to {1}")]
    MustBeStopped(VmId, &'static str),
    #[error("vm {0} is a template; cannot {1}")]
    IsTemplate(VmId, &'static str),
    #[error("volume {0} is attached to a vm")]
    VolumeBusy(VolumeId),
    #[error("iso {0} is attached to a vm")]
    IsoBusy(String),
    #[error("network {0} is attached to a vm")]
    NetworkBusy(NetworkId),
    #[error("no cluster quorum")]
    NoQuorum,
    #[error("node is fenced (lost quorum)")]
    Fenced,
    #[error("insufficient host capacity: {0}")]
    Capacity(String),
    #[error("no node has capacity for this vm ({0})")]
    Unschedulable(String),
    #[error("cluster peer: {0}")]
    Peer(String),
    #[error("apt: {0}")]
    Apt(String),
    #[error("host power: {0}")]
    HostPower(String),
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error(transparent)]
    Types(#[from] pertisk_types::TypesError),
    #[error(transparent)]
    Vmm(#[from] pertisk_vmm::VmmError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Net(#[from] NetError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
}

#[derive(Clone)]
pub struct Service {
    vmm: Arc<VmmBackend>,
    store: Arc<Store>,
    volumes: Arc<VolumePool>,
    networks: Arc<NetworkPool>,
    control: Arc<ControlStore>,
    cluster: Arc<Cluster>,
    console: ConsoleHub,
    http: reqwest::Client,
    rebuild: Arc<tokio::sync::Mutex<()>>,
    config: HostConfig,
    data_dir: std::path::PathBuf,
    started_at: Instant,
    autostarted: Arc<Mutex<HashSet<VmId>>>,
    created_this_boot: Arc<Mutex<HashSet<VmId>>>,
    metrics: Arc<MetricsCache>,
}

impl Service {
    pub fn new(
        vmm: VmmBackend,
        store: Store,
        volumes: VolumePool,
        networks: NetworkPool,
        control: ControlStore,
        config: HostConfig,
        data_dir: std::path::PathBuf,
    ) -> Self {
        let listen = config.daemon.listen.clone();
        let cluster = Cluster::open(data_dir.join("state/cluster.json"), &config, &listen)
            .expect("open cluster state");
        Self {
            vmm: Arc::new(vmm),
            store: Arc::new(store),
            volumes: Arc::new(volumes),
            networks: Arc::new(networks),
            control: Arc::new(control),
            cluster: Arc::new(cluster),
            console: ConsoleHub::new(),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(15))
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            rebuild: Arc::new(tokio::sync::Mutex::new(())),
            config,
            data_dir,
            started_at: Instant::now(),
            autostarted: Arc::new(Mutex::new(HashSet::new())),
            created_this_boot: Arc::new(Mutex::new(HashSet::new())),
            metrics: Arc::new(MetricsCache::new()),
        }
    }

    pub fn configured_peer_url(&self) -> Option<&str> {
        self.config.cluster.peer_url.as_deref()
    }

    pub fn driver(&self) -> DriverKind {
        self.vmm.kind()
    }

    pub fn authenticate(&self, token: &str) -> Result<AuthUser, DaemonError> {
        Ok(self.control.authenticate(token)?)
    }

    pub fn login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<pertisk_api::TokenResponse, DaemonError> {
        Ok(self.control.login(username, password)?)
    }

    pub fn begin_task(
        &self,
        actor: &str,
        kind: &str,
        target: Option<&str>,
    ) -> Result<pertisk_api::TaskRecord, DaemonError> {
        Ok(self.control.begin_task(actor, kind, target)?)
    }

    pub fn finish_task(
        &self,
        id: &str,
        result: Result<(), String>,
    ) -> Result<pertisk_api::TaskRecord, DaemonError> {
        Ok(self.control.finish_task(id, result)?)
    }

    pub fn list_tasks(&self) -> Result<Vec<pertisk_api::TaskRecord>, DaemonError> {
        Ok(self.control.list_tasks()?)
    }

    pub fn list_audit(&self) -> Result<Vec<pertisk_api::AuditEvent>, DaemonError> {
        Ok(self.control.list_audit()?)
    }

    pub fn audit(
        &self,
        actor: &str,
        action: &str,
        target: Option<&str>,
    ) -> Result<(), DaemonError> {
        Ok(self.control.audit(actor, action, target)?)
    }

    pub fn list_users(&self) -> Result<Vec<pertisk_api::UserRecord>, DaemonError> {
        Ok(self.control.list_users()?)
    }

    pub fn create_user(
        &self,
        req: pertisk_api::CreateUserRequest,
    ) -> Result<pertisk_api::UserRecord, DaemonError> {
        Ok(self.control.create_user(req)?)
    }

    pub fn delete_user(&self, id: &str) -> Result<(), DaemonError> {
        Ok(self.control.delete_user(id)?)
    }

    pub fn change_own_password(
        &self,
        user: &AuthUser,
        current_password: &str,
        new_password: &str,
        keep_token: &str,
    ) -> Result<(), DaemonError> {
        self.control
            .change_password(&user.id, current_password, new_password, Some(keep_token))?;
        let _ = self
            .control
            .audit(&user.username, "user.password", Some(&user.username));
        Ok(())
    }

    pub fn set_user_password(&self, id: &str, new_password: &str) -> Result<(), DaemonError> {
        self.control.set_password(id, new_password, None)?;
        Ok(())
    }

    pub fn host_info(&self) -> HostInfo {
        let mut info = probe_host(&self.config, self.data_dir.clone());
        info.node_id = Some(self.cluster.self_id());
        info.quorum = self.cluster.has_quorum();
        info.ssh_authorized_keys = pertisk_storage::operator_ssh_keys();
        info
    }

    pub async fn list_updates(&self) -> Result<UpdatesStatus, DaemonError> {
        tokio::task::spawn_blocking(crate::updates::list_updates)
            .await
            .map_err(|err| DaemonError::Apt(err.to_string()))?
            .map_err(DaemonError::Apt)
    }

    pub async fn refresh_updates(&self) -> Result<AptActionResult, DaemonError> {
        tokio::task::spawn_blocking(crate::updates::refresh)
            .await
            .map_err(|err| DaemonError::Apt(err.to_string()))?
            .map_err(DaemonError::Apt)
    }

    pub async fn upgrade_updates(&self) -> Result<AptActionResult, DaemonError> {
        tokio::task::spawn_blocking(crate::updates::upgrade)
            .await
            .map_err(|err| DaemonError::Apt(err.to_string()))?
            .map_err(DaemonError::Apt)
    }

    pub fn list_repositories(&self) -> Result<Vec<AptRepository>, DaemonError> {
        crate::updates::list_repos().map_err(DaemonError::Apt)
    }

    pub fn add_repository(&self, req: AddRepositoryRequest) -> Result<AptRepository, DaemonError> {
        crate::updates::add_repo(req).map_err(DaemonError::Apt)
    }

    pub fn set_repository(&self, req: SetRepositoryRequest) -> Result<AptRepository, DaemonError> {
        crate::updates::set_repo(req).map_err(DaemonError::Apt)
    }

    pub fn node_metrics(&self) -> Result<NodeMetrics, DaemonError> {
        let live = metrics::sample_host(&self.metrics, &self.config.storage.root);
        let self_id = self.cluster.self_id();
        let status = self.cluster_status()?;
        let me = status.members.iter().find(|m| m.id == self_id).cloned();
        let vms = self.store.list()?;
        let running: Vec<_> = vms
            .iter()
            .filter(|vm| vm.node_id == Some(self_id) && vm.state == VmState::Running)
            .collect();
        Ok(NodeMetrics {
            node_id: self_id,
            name: me.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| {
                self.config
                    .cluster
                    .node_name
                    .clone()
                    .unwrap_or_else(|| "local".into())
            }),
            live,
            allocated_vcpus: running.iter().map(|vm| u32::from(vm.spec.vcpus)).sum(),
            allocated_memory_mib: running.iter().map(|vm| vm.spec.memory_mib).sum(),
            running_vms: running.len() as u32,
        })
    }

    pub fn vm_metrics(&self, id: VmId) -> Result<VmMetrics, DaemonError> {
        let vm = self.store.get(id)?;
        let volumes = self.list_volumes().unwrap_or_default();
        let live = metrics::sample_vm(&self.metrics, &vm, &volumes);
        Ok(VmMetrics {
            id: vm.id,
            state: vm.state,
            live,
            vcpus: vm.spec.vcpus,
            memory_mib: vm.spec.memory_mib,
        })
    }

    pub fn cluster_metrics(&self) -> Result<ClusterMetrics, DaemonError> {
        let node = self.node_metrics()?;
        let vms = self.store.list()?;
        let running_vms = vms.iter().filter(|vm| vm.state == VmState::Running).count() as u32;
        Ok(ClusterMetrics {
            live: node.live.clone(),
            nodes: vec![node],
            running_vms,
            total_vms: vms.iter().filter(|vm| !vm.template).count() as u32,
        })
    }

    pub fn cluster_status(&self) -> Result<pertisk_types::ClusterStatus, DaemonError> {
        Ok(self.cluster.status(&self.loads()?))
    }

    pub fn set_peer_url(&self, url: String) -> Result<(), DaemonError> {
        self.cluster.set_peer_url(url)
    }

    pub fn join_peer(&self) -> Option<String> {
        self.config.cluster.join.clone()
    }

    pub fn heartbeat_period(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.cluster.heartbeat_ms())
    }

    pub fn peer_secret_ok(&self, secret: &str) -> bool {
        self.cluster.check_secret(secret)
    }

    pub fn list(&self) -> Result<Vec<VmRecord>, DaemonError> {
        let mut vms = self.store.list()?;
        let run_dir = &self.config.vmm.run_dir;
        if vms.iter().any(vm_needs_ip_probe) {
            pertisk_net::probe_bridge_neighbors();
        }
        for vm in &mut vms {
            if enrich_observed_ips(vm, run_dir) {
                // Keep discovered DHCP addresses so Summary stays filled when ARP goes cold.
                let _ = self.store.upsert(vm.clone());
            }
        }
        Ok(vms)
    }

    pub fn get(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(id)?;
        if vm_needs_ip_probe(&vm) {
            pertisk_net::probe_bridge_neighbors();
        }
        if enrich_observed_ips(&mut vm, &self.config.vmm.run_dir) {
            let _ = self.store.upsert(vm.clone());
        }
        Ok(vm)
    }

    pub async fn create(&self, id: VmId, spec: VmSpec) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        spec.validate()?;
        if self.store.contains(id) {
            return Err(DaemonError::IdTaken(id));
        }
        if self.store.name_taken(&spec.name, None)? {
            return Err(DaemonError::NameTaken(spec.name));
        }
        let mut spec = spec;
        if spec.serial_log.is_none() {
            spec.serial_log = Some(self.config.vmm.run_dir.join(format!("{id}.serial")));
        }
        let dest = self.pick_node_define(&spec, None)?;
        let serial_log = spec.serial_log.clone();
        let record = VmRecord {
            id,
            spec,
            state: VmState::Created,
            pid: None,
            api_socket: None,
            serial_log,
            console_socket: None,
            graphics_socket: None,
            last_error: None,
            node_id: Some(dest),
            template: false,
        };
        self.store.upsert(record.clone())?;
        self.created_this_boot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(id);
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub async fn update(&self, id: VmId, req: UpdateVmRequest) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let mut vm = self.store.get(id)?;
        if req.vcpus.is_some() || req.memory_mib.is_some() {
            self.require_stopped(&vm, "resize cpu or memory")?;
        }
        if let Some(name) = req.name {
            let name = name.trim().to_string();
            if self.store.name_taken(&name, Some(id))? {
                return Err(DaemonError::NameTaken(name));
            }
            vm.spec.name = name;
        }
        if let Some(vcpus) = req.vcpus {
            vm.spec.vcpus = vcpus;
        }
        if let Some(memory_mib) = req.memory_mib {
            vm.spec.memory_mib = memory_mib;
        }
        if let Some(ha) = req.ha {
            vm.spec.ha = ha;
        }
        if let Some(autostart) = req.autostart {
            vm.spec.autostart = autostart;
        }
        if let Some(autostart_delay) = req.autostart_delay {
            vm.spec.autostart_delay = autostart_delay;
        }
        if let Some(autostart_order) = req.autostart_order {
            vm.spec.autostart_order = autostart_order;
        }
        vm.spec.validate()?;
        self.store.upsert(vm.clone())?;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(vm)
    }

    pub fn list_templates(&self) -> Result<Vec<VmRecord>, DaemonError> {
        Ok(self.list()?.into_iter().filter(|vm| vm.template).collect())
    }

    pub async fn convert_to_template(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let mut vm = self.store.get(id)?;
        if vm.template {
            return Ok(vm);
        }
        self.require_stopped(&vm, "convert to template")?;
        vm.template = true;
        vm.spec.autostart = false;
        vm.spec.ha = false;
        self.store.upsert(vm.clone())?;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(vm)
    }

    pub async fn create_template(
        &self,
        req: CreateTemplateRequest,
    ) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let name = req.name.trim().to_string();
        if name.is_empty() {
            return Err(pertisk_types::TypesError::InvalidSpec("name is required".into()).into());
        }
        let spec = VmSpec {
            name,
            vcpus: req.vcpus.unwrap_or(1),
            memory_mib: req.memory_mib.unwrap_or(1024),
            kernel: None,
            cmdline: None,
            initramfs: None,
            firmware: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
            console_type: req.console_type.unwrap_or(ConsoleType::Serial),
            ha: false,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
        };
        spec.validate()?;
        let id = match req.id {
            Some(id) if self.store.contains(id) => return Err(DaemonError::IdTaken(id)),
            Some(id) => id,
            None => self.next_numeric_vm_id()?,
        };
        let record = self.create(id, spec).await?;
        if let Err(err) = self.attach_disk(
            id,
            AttachDiskRequest {
                volume_id: req.volume_id,
            },
        ) {
            let _ = self.destroy(id).await;
            return Err(err);
        }
        self.convert_to_template(record.id).await
    }

    pub async fn clone_vm(&self, id: VmId, req: CloneVmRequest) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let source = self.store.get(id)?;
        self.require_stopped(&source, "clone")?;
        let name = req.name.trim().to_string();
        if name.is_empty() {
            return Err(pertisk_types::TypesError::InvalidSpec("name is required".into()).into());
        }
        if self.store.name_taken(&name, None)? {
            return Err(DaemonError::NameTaken(name));
        }
        let new_id = match req.id {
            Some(new_id) if self.store.contains(new_id) => {
                return Err(DaemonError::IdTaken(new_id));
            }
            Some(new_id) => new_id,
            None => self.next_numeric_vm_id()?,
        };
        let mut spec = source.spec.clone();
        spec.name = name.clone();
        spec.disks = Vec::new();
        spec.nets = Vec::new();
        spec.serial_log = None;
        spec.autostart = req.autostart.unwrap_or(false);
        spec.ha = req.ha.unwrap_or(true);
        spec.autostart_delay = 0;
        spec.autostart_order = 0;
        if let Some(vcpus) = req.vcpus {
            spec.vcpus = vcpus;
        }
        if let Some(memory_mib) = req.memory_mib {
            spec.memory_mib = memory_mib;
        }
        if req.start {
            if let Some(budget) = self.start_memory_budget_mib() {
                if spec.memory_mib > budget && budget >= 64 {
                    spec.memory_mib = budget;
                }
            }
        }
        spec.validate()?;

        let created = match self.create(new_id, spec).await {
            Ok(record) => record,
            Err(err) => return Err(err),
        };
        if let Err(err) = self.finish_clone(&source, created.id, &name, &req).await {
            let _ = self.destroy(created.id).await;
            return Err(err);
        }
        self.cluster.bump()?;
        self.replicate().await;
        if req.start {
            match self.start(created.id).await {
                Ok(started) => Ok(started),
                Err(err @ (DaemonError::Capacity(_) | DaemonError::Unschedulable(_))) => {
                    let mut record = self.get(created.id)?;
                    record.last_error = Some(err.to_string());
                    self.store.upsert(record.clone())?;
                    Ok(record)
                }
                Err(err) => Err(err),
            }
        } else {
            self.get(created.id)
        }
    }

    async fn finish_clone(
        &self,
        source: &VmRecord,
        new_id: VmId,
        name: &str,
        req: &CloneVmRequest,
    ) -> Result<(), DaemonError> {
        let mut grow = req.disk_size_bytes;
        let mut os_disk: Option<std::path::PathBuf> = None;
        for disk in &source.spec.disks {
            if disk.cdrom {
                continue;
            }
            let Some(volume_id) = disk.volume_id else {
                continue;
            };
            let source_vol = self.volumes.get_volume(volume_id)?;
            let vol_name = self.unique_volume_name(&format!("{name}-{}", source_vol.name))?;
            let linked = req.linked && self.driver() != DriverKind::CloudHypervisor;
            let cloned = match self
                .clone_volume(
                    volume_id,
                    CloneVolumeRequest {
                        name: vol_name.clone(),
                        linked,
                    },
                )
                .await
            {
                Err(DaemonError::Storage(StorageError::LinkedRequiresQemu)) if linked => {
                    self.clone_volume(
                        volume_id,
                        CloneVolumeRequest {
                            name: vol_name,
                            linked: false,
                        },
                    )
                    .await?
                }
                other => other?,
            };
            let cloned = if let Some(size) = grow.take() {
                if size > cloned.size_bytes {
                    self.resize_volume(cloned.id, ResizeVolumeRequest { size_bytes: size })
                        .await?
                } else {
                    cloned
                }
            } else {
                cloned
            };
            if os_disk.is_none() {
                os_disk = Some(cloned.path.clone());
            }
            self.attach_disk(
                new_id,
                AttachDiskRequest {
                    volume_id: cloned.id,
                },
            )?;
        }
        let network_id = req
            .network_id
            .or_else(|| source.spec.nets.first().and_then(|nic| nic.network_id))
            .or_else(|| self.default_clone_network_id());
        if let Some(network_id) = network_id {
            self.attach_nic(
                new_id,
                AttachNicRequest {
                    network_id,
                    ip: req.ip.clone(),
                },
            )?;
        }
        if let Some(ci) = &req.cloud_init {
            let hostname = ci
                .hostname
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(name)
                .to_string();
            let user = ci
                .user
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    let mut hints = vec![source.spec.name.clone(), name.to_string()];
                    for disk in &source.spec.disks {
                        if let Some(id) = disk.volume_id {
                            if let Ok(vol) = self.volumes.get_volume(id) {
                                hints.push(vol.name);
                            }
                        }
                    }
                    default_cloud_user(hints.iter().map(|s| s.as_str())).to_string()
                });
            if let Some(path) = os_disk {
                let hostname_i = hostname.clone();
                let user_i = user.clone();
                let password_i = ci.password.clone();
                let keys_i = ci.ssh_authorized_keys.clone();
                let net = self.cloudinit_network_for(new_id);
                let mac_i = net.as_ref().and_then(|n| n.mac.clone());
                let ipv4_i = net.as_ref().and_then(|n| n.ipv4.clone());
                let gw_i = net.as_ref().and_then(|n| n.gateway.clone());
                let prefix_i = net.as_ref().and_then(|n| n.prefix);
                tokio::task::spawn_blocking(move || {
                    pertisk_storage::inject_guest_identity(
                        &path,
                        &pertisk_storage::GuestIdentity {
                            hostname: &hostname_i,
                            user: &user_i,
                            password: password_i
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty()),
                            ssh_authorized_keys: &keys_i,
                            mac: mac_i.as_deref(),
                            ipv4: ipv4_i.as_deref(),
                            gateway: gw_i.as_deref(),
                            prefix: prefix_i,
                        },
                    )
                })
                .await
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))??;
            }
            let iso = self.create_cloudinit_iso(CloudInitIsoRequest {
                name: format!("{name}-{new_id}-cidata.iso"),
                hostname: Some(hostname),
                user: Some(user),
                password: ci.password.clone(),
                ssh_authorized_keys: ci.ssh_authorized_keys.clone(),
                userdata: ci.userdata.clone(),
                network: self.cloudinit_network_for(new_id),
            })?;
            self.attach_iso(new_id, AttachIsoRequest { iso: iso.name })?;
        }
        Ok(())
    }

    fn cloudinit_network_for(&self, vm_id: VmId) -> Option<CloudInitNetwork> {
        let vm = self.store.get(vm_id).ok()?;
        let nic = vm.spec.nets.first()?;
        let net = nic.network_id.and_then(|id| self.networks.get(id).ok());
        let prefix = net.as_ref().and_then(|n| {
            n.cidr
                .split_once('/')
                .and_then(|(_, p)| p.parse::<u8>().ok())
        });
        Some(CloudInitNetwork {
            mac: nic.mac.clone(),
            ipv4: nic.ip.clone(),
            gateway: net.and_then(|n| n.gateway),
            prefix,
        })
    }

    pub async fn start(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let mut record = self.store.get(id)?;
        self.require_not_template(&record, "start")?;
        let affinity = self.volume_affinity(&record.spec);
        let dest = match record.node_id {
            Some(current) if affinity.is_empty() || affinity.contains(&current) => current,
            _ => self.pick_node(&record.spec, affinity.first().copied())?,
        };
        if record.node_id != Some(dest) {
            record.node_id = Some(dest);
            self.store.upsert(record.clone())?;
        }
        if dest != self.cluster.self_id() {
            let started = self.peer_run(dest, record).await?;
            self.store.upsert(started.clone())?;
            self.cluster.bump()?;
            self.replicate().await;
            return Ok(started);
        }
        self.start_local(id).await
    }

    pub async fn start_local(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let mut record = self.store.get(id)?;
        self.require_not_template(&record, "start")?;
        self.localize_disks(&mut record)?;
        self.store.upsert(record.clone())?;
        match record.state {
            VmState::Created | VmState::Stopped | VmState::Failed => {}
            state => {
                return Err(pertisk_vmm::VmmError::InvalidState { state, op: "start" }.into());
            }
        }
        self.ensure_start_capacity(&record)?;
        let boot_spec = self.prefer_disk_boot_spec(&self.iso_linux_boot_spec(&record.spec)?);
        for nic in &record.spec.nets {
            self.networks.ensure_host_links(nic)?;
        }
        let socket_missing = record
            .api_socket
            .as_ref()
            .map(|p| !p.exists())
            .unwrap_or(true);
        let needs_vmm = matches!(
            self.driver(),
            DriverKind::CloudHypervisor | DriverKind::Qemu
        );
        let vmm_alive = if needs_vmm {
            self.vmm.is_running(&record).await
        } else {
            true
        };
        let recreate = matches!(record.state, VmState::Created | VmState::Failed)
            || (needs_vmm && (socket_missing || !vmm_alive));
        if recreate {
            match self.vmm.create(record.id, &boot_spec).await {
                Ok(created) => {
                    record.pid = created.pid;
                    record.api_socket = created.api_socket;
                    if created.serial_log.is_some() {
                        record.serial_log = created.serial_log;
                    }
                    if created.console_socket.is_some() {
                        record.console_socket = created.console_socket;
                    }
                    if created.graphics_socket.is_some() {
                        record.graphics_socket = created.graphics_socket;
                    }
                    self.store.upsert(record.clone())?;
                }
                Err(err) => {
                    record.state = VmState::Failed;
                    record.last_error = Some(err.to_string());
                    self.store.upsert(record)?;
                    return Err(err.into());
                }
            }
        }
        match self.vmm.start(&record).await {
            Ok(started) => {
                record.state = VmState::Running;
                if started.pid.is_some() {
                    record.pid = started.pid;
                }
                record.last_error = None;
                if record.node_id.is_none() {
                    record.node_id = Some(self.cluster.self_id());
                }
                self.store.upsert(record.clone())?;
                self.attach_console(&record).await;
                self.spawn_exit_watch(&record);
                self.cluster.bump()?;
                self.replicate().await;
                Ok(record)
            }
            Err(err) => {
                record.state = VmState::Failed;
                record.last_error = Some(err.to_string());
                self.store.upsert(record)?;
                Err(err.into())
            }
        }
    }

    pub async fn stop(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let record = self.store.get(id)?;
        self.require_not_template(&record, "stop")?;
        if let Some(dest) = record.node_id
            && dest != self.cluster.self_id()
        {
            return self.peer_stop(dest, record).await;
        }
        self.stop_local(id).await
    }

    pub async fn stop_local(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let mut record = self.store.get(id)?;
        self.console.drop_vm(id).await;
        if !self.vmm.is_running(&record).await {
            record.state = VmState::Stopped;
            record.pid = None;
            record.last_error = None;
            self.store.upsert(record.clone())?;
            self.cluster.bump()?;
            self.replicate().await;
            self.sync_vm_volumes(&record).await;
            return Ok(record);
        }
        self.vmm.stop(&record).await?;
        record.state = VmState::Stopped;
        record.pid = None;
        record.last_error = None;
        self.store.upsert(record.clone())?;
        self.cluster.bump()?;
        self.replicate().await;
        self.sync_vm_volumes(&record).await;
        Ok(record)
    }

    pub async fn shutdown(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let record = self.store.get(id)?;
        self.require_not_template(&record, "shutdown")?;
        if let Some(dest) = record.node_id
            && dest != self.cluster.self_id()
        {
            return self.peer_shutdown(dest, record).await;
        }
        self.shutdown_local(id).await
    }

    pub async fn shutdown_local(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let mut record = self.store.get(id)?;
        if record.state != VmState::Running {
            return Err(pertisk_vmm::VmmError::InvalidState {
                state: record.state,
                op: "shutdown",
            }
            .into());
        }
        self.console.drop_vm(id).await;
        if !self.vmm.is_running(&record).await {
            record.state = VmState::Stopped;
            record.pid = None;
            record.last_error = None;
            self.store.upsert(record.clone())?;
            self.cluster.bump()?;
            self.replicate().await;
            self.sync_vm_volumes(&record).await;
            return Ok(record);
        }
        self.vmm.shutdown(&record).await?;
        record.state = VmState::Stopped;
        record.pid = None;
        record.last_error = None;
        self.store.upsert(record.clone())?;
        self.cluster.bump()?;
        self.replicate().await;
        self.sync_vm_volumes(&record).await;
        Ok(record)
    }

    pub async fn restart(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let record = self.store.get(id)?;
        self.require_not_template(&record, "restart")?;
        if let Some(dest) = record.node_id
            && dest != self.cluster.self_id()
        {
            return self.peer_restart(dest, record).await;
        }
        self.restart_local(id).await
    }

    pub async fn restart_local(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let record = self.store.get(id)?;
        if record.state != VmState::Running {
            return Err(pertisk_vmm::VmmError::InvalidState {
                state: record.state,
                op: "restart",
            }
            .into());
        }
        match self.driver() {
            DriverKind::Qemu | DriverKind::Mock => {
                if self.driver() == DriverKind::Qemu && self.prefer_disk_recreate(&record) {
                    tracing::info!(vm = %id, "restart: disk installed — stop+start without installer ISO");
                    self.shutdown_local(id).await?;
                    return self.start_local(id).await;
                }
                self.vmm.restart(&record).await?;
                self.attach_console(&record).await;
                Ok(record)
            }
            DriverKind::CloudHypervisor => {
                self.shutdown_local(id).await?;
                self.start_local(id).await
            }
        }
    }

    pub async fn destroy(&self, id: VmId) -> Result<(), DaemonError> {
        self.require_quorum()?;
        let record = self.store.get(id)?;
        let disks: Vec<VolumeId> = record
            .spec
            .disks
            .iter()
            .filter_map(|disk| disk.volume_id)
            .collect();
        let cidata: Vec<String> = record
            .spec
            .disks
            .iter()
            .filter(|disk| disk.cdrom)
            .filter_map(|disk| disk.iso_name.clone())
            .filter(|name| name.to_ascii_lowercase().contains("cidata"))
            .collect();
        if let Some(dest) = record.node_id
            && dest != self.cluster.self_id()
        {
            let _ = self.peer_drop(dest, &record).await;
        } else {
            match self.vmm.destroy(&record).await {
                Ok(()) => {}
                Err(pertisk_vmm::VmmError::NotFound(_)) => {}
                Err(err) => return Err(err.into()),
            }
        }
        self.console.drop_vm(id).await;
        for nic in &record.spec.nets {
            let _ = self.networks.release_nic(nic);
        }
        self.store.remove(id)?;
        for iso in cidata {
            if self.iso_users(&iso)?.is_empty() {
                let _ = self.delete_iso(&iso);
            }
        }
        for volume_id in disks {
            if !self.volume_users(volume_id)?.is_empty() || self.volume_is_backing(volume_id)? {
                continue;
            }
            let _ = self.delete_volume(volume_id).await;
        }
        self.cluster.bump()?;
        self.replicate().await;
        Ok(())
    }

    pub async fn apply_run(&self, mut record: VmRecord) -> Result<VmRecord, DaemonError> {
        record.node_id = Some(self.cluster.self_id());
        record.pid = None;
        record.api_socket = None;
        record.console_socket = None;
        record.graphics_socket = None;
        let serial = self
            .config
            .vmm
            .run_dir
            .join(format!("{}.serial", record.id));
        record.serial_log = Some(serial.clone());
        record.spec.serial_log = Some(serial);
        if record.state == VmState::Running {
            record.state = VmState::Created;
        }
        self.store.upsert(record.clone())?;
        match self.vmm.destroy(&record).await {
            Ok(()) | Err(pertisk_vmm::VmmError::NotFound(_)) => {}
            Err(_) => {}
        }
        self.start_local(record.id).await
    }

    pub async fn apply_stop(&self, record: VmRecord) -> Result<VmRecord, DaemonError> {
        let id = record.id;
        self.store.upsert(record)?;
        self.stop_local(id).await
    }

    pub async fn apply_shutdown(&self, record: VmRecord) -> Result<VmRecord, DaemonError> {
        let id = record.id;
        self.store.upsert(record)?;
        self.shutdown_local(id).await
    }

    pub async fn apply_restart(&self, record: VmRecord) -> Result<VmRecord, DaemonError> {
        let id = record.id;
        self.store.upsert(record)?;
        self.restart_local(id).await
    }

    pub async fn apply_drop(&self, record: &VmRecord) -> Result<(), DaemonError> {
        self.console.drop_vm(record.id).await;
        match self.vmm.destroy(record).await {
            Ok(()) | Err(pertisk_vmm::VmmError::NotFound(_)) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn migrate(
        &self,
        id: VmId,
        target: Option<pertisk_types::NodeId>,
    ) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let mut record = self.store.get(id)?;
        self.require_not_template(&record, "migrate")?;
        let dest = self.pick_node(&record.spec, target)?;
        let src = record.node_id.unwrap_or(self.cluster.self_id());
        if dest == src {
            return Ok(record);
        }
        if src == self.cluster.self_id() {
            self.sync_vm_volumes(&record).await;
        }
        let started = self.peer_run(dest, record.clone()).await?;
        if src == self.cluster.self_id() {
            let _ = self.apply_drop(&record).await;
        } else {
            let _ = self.peer_drop(src, &record).await;
        }
        record = started;
        record.node_id = Some(dest);
        self.store.upsert(record.clone())?;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub fn list_volumes(&self) -> Result<Vec<VolumeRecord>, DaemonError> {
        Ok(self.volumes.list_volumes()?)
    }

    pub fn get_volume(&self, id: VolumeId) -> Result<VolumeRecord, DaemonError> {
        Ok(self.volumes.get_volume(id)?)
    }

    pub async fn create_volume(
        &self,
        req: CreateVolumeRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_quorum()?;
        if self.config.storage.backend == StorageBackend::Rbd {
            if !Rbd::available() {
                return Err(DaemonError::Peer(
                    "storage.backend=rbd but the rbd CLI was not found".into(),
                ));
            }
            let rbd = Rbd::new(self.config.storage.rbd_pool.clone());
            rbd.create_image(&req.name, req.size_bytes)?;
            let record = VolumeRecord {
                id: VolumeId::new(),
                name: req.name.clone(),
                format: req.format,
                size_bytes: req.size_bytes,
                path: rbd.image_path(&req.name).into(),
                backing_id: None,
                snapshots: vec![],
                replicas: vec![],
                replica_count: 1,
                backend: StorageBackend::Rbd,
            };
            let record = self.volumes.put_record(record)?;
            self.cluster.bump()?;
            self.replicate().await;
            return Ok(record);
        }
        let mut record = self.volumes.create_volume(req.clone())?;
        let want = req
            .replicas
            .unwrap_or(self.config.storage.replica_count)
            .max(1);
        let online = self.cluster.online_ids();
        record.replica_count = want;
        record.replicas = cluster::place_replicas(&online, want, Some(self.cluster.self_id()));
        record.backend = StorageBackend::Replica;
        record = self.volumes.put_record(record)?;
        self.ensure_replicas(&record).await;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    /// Streamed qcow2/raw image → inventory volume (template for clone).
    pub async fn import_volume(
        &self,
        name: String,
        format: VolumeFormat,
        source: std::path::PathBuf,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_quorum()?;
        let mut record = self.volumes.import_volume(&source, name, format)?;
        let want = self.config.storage.replica_count.max(1);
        let online = self.cluster.online_ids();
        record.replica_count = want;
        record.replicas = cluster::place_replicas(&online, want, Some(self.cluster.self_id()));
        record.backend = StorageBackend::Replica;
        record = self.volumes.put_record(record)?;
        self.ensure_replicas(&record).await;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub async fn delete_volume(&self, id: VolumeId) -> Result<(), DaemonError> {
        if !self.volume_users(id)?.is_empty() {
            return Err(DaemonError::VolumeBusy(id));
        }
        let record = self.volumes.get_volume(id)?;
        for replica in &record.replicas {
            if *replica != self.cluster.self_id() {
                let _ = self.peer_delete_volume(*replica, id).await;
            }
        }
        if record.backend == StorageBackend::Rbd && Rbd::available() {
            let rbd = Rbd::new(self.config.storage.rbd_pool.clone());
            let _ = rbd.remove_image(&record.name);
        }
        self.volumes.delete_volume(id)?;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(())
    }

    pub async fn resize_volume(
        &self,
        id: VolumeId,
        req: ResizeVolumeRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "resize")?;
        let record = self.volumes.resize(id, req)?;
        self.sync_volume_replicas(&record).await;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub async fn clone_volume(
        &self,
        id: VolumeId,
        req: CloneVolumeRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "clone")?;
        let volumes = Arc::clone(&self.volumes);
        let req = req.clone();
        let mut record = tokio::task::spawn_blocking(move || volumes.clone_volume(id, req))
            .await
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))??;
        let online = self.cluster.online_ids();
        record.replicas = cluster::place_replicas(
            &online,
            record.replica_count.max(1),
            Some(self.cluster.self_id()),
        );
        record = self.volumes.put_record(record)?;
        self.ensure_replicas(&record).await;
        self.sync_volume_replicas(&record).await;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub async fn snapshot_volume(
        &self,
        id: VolumeId,
        req: SnapshotRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "snapshot")?;
        let record = self.volumes.snapshot(id, req)?;
        self.sync_volume_replicas(&record).await;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub async fn restore_volume(
        &self,
        id: VolumeId,
        name: &str,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "restore")?;
        let record = self.volumes.restore_snapshot(id, name)?;
        self.sync_volume_replicas(&record).await;
        self.cluster.bump()?;
        self.replicate().await;
        Ok(record)
    }

    pub fn list_isos(&self) -> Result<Vec<IsoRecord>, DaemonError> {
        Ok(self.volumes.list_isos()?)
    }

    pub fn import_iso(&self, req: ImportIsoRequest) -> Result<IsoRecord, DaemonError> {
        Ok(self.volumes.import_iso(&req.path, req.name)?)
    }

    pub fn create_cloudinit_iso(&self, req: CloudInitIsoRequest) -> Result<IsoRecord, DaemonError> {
        Ok(self.volumes.create_cloudinit_iso(req)?)
    }

    pub fn delete_iso(&self, name: &str) -> Result<(), DaemonError> {
        if !self.iso_users(name)?.is_empty() {
            return Err(DaemonError::IsoBusy(name.to_string()));
        }
        Ok(self.volumes.delete_iso(name)?)
    }

    pub fn attach_disk(
        &self,
        vm_id: VmId,
        req: AttachDiskRequest,
    ) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(vm_id)?;
        self.require_stopped(&vm, "attach disk")?;
        if vm
            .spec
            .disks
            .iter()
            .any(|disk| disk.volume_id == Some(req.volume_id))
        {
            return Ok(vm);
        }
        if !self.volume_users(req.volume_id)?.is_empty() {
            return Err(DaemonError::VolumeBusy(req.volume_id));
        }
        let volume = self.volumes.get_volume(req.volume_id)?;
        let path = if volume.backend == StorageBackend::Rbd {
            volume.path.clone()
        } else {
            self.volumes.local_path(volume.id, volume.format)
        };
        vm.spec.disks.push(DiskSpec {
            path,
            readonly: false,
            cdrom: false,
            volume_id: Some(volume.id),
            iso_name: None,
        });
        self.store.upsert(vm.clone())?;
        Ok(vm)
    }

    pub fn attach_iso(&self, vm_id: VmId, req: AttachIsoRequest) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(vm_id)?;
        self.require_stopped(&vm, "attach iso")?;
        if vm
            .spec
            .disks
            .iter()
            .any(|disk| disk.iso_name.as_deref() == Some(req.iso.as_str()))
        {
            return Ok(vm);
        }
        let iso = self.volumes.get_iso(&req.iso)?;
        vm.spec.disks.push(DiskSpec {
            path: iso.path,
            readonly: true,
            cdrom: true,
            volume_id: None,
            iso_name: Some(iso.name),
        });
        self.store.upsert(vm.clone())?;
        Ok(vm)
    }

    pub fn detach_disk(&self, vm_id: VmId, volume_id: VolumeId) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(vm_id)?;
        self.require_stopped(&vm, "detach disk")?;
        vm.spec
            .disks
            .retain(|disk| disk.volume_id != Some(volume_id));
        self.store.upsert(vm.clone())?;
        Ok(vm)
    }

    pub fn detach_iso(&self, vm_id: VmId, name: &str) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(vm_id)?;
        self.require_stopped(&vm, "detach iso")?;
        vm.spec
            .disks
            .retain(|disk| disk.iso_name.as_deref() != Some(name));
        self.store.upsert(vm.clone())?;
        Ok(vm)
    }

    fn require_stopped(&self, vm: &VmRecord, op: &'static str) -> Result<(), DaemonError> {
        if vm.state == VmState::Running {
            return Err(DaemonError::MustBeStopped(vm.id, op));
        }
        Ok(())
    }

    fn require_not_template(&self, vm: &VmRecord, op: &'static str) -> Result<(), DaemonError> {
        if vm.template {
            return Err(DaemonError::IsTemplate(vm.id, op));
        }
        Ok(())
    }

    fn next_numeric_vm_id(&self) -> Result<VmId, DaemonError> {
        let used: HashSet<u64> = self
            .store
            .list()?
            .into_iter()
            .filter_map(|vm| match vm.id {
                VmId::Numeric(n) => Some(n),
                VmId::Legacy(_) => None,
            })
            .collect();
        let mut n = 100u64;
        while used.contains(&n) {
            n += 1;
            if n > 9_999_999_999 {
                return Ok(VmId::new());
            }
        }
        Ok(VmId::Numeric(n))
    }

    pub(crate) fn unique_volume_name(&self, base: &str) -> Result<String, DaemonError> {
        let vols = self.volumes.list_volumes()?;
        if !vols.iter().any(|vol| vol.name == base) {
            return Ok(base.to_string());
        }
        for i in 2..10_000 {
            let name = format!("{base}-{i}");
            if !vols.iter().any(|vol| vol.name == name) {
                return Ok(name);
            }
        }
        Ok(format!("{base}-{}", VolumeId::new()))
    }

    pub(crate) fn unique_vm_name(&self, base: &str) -> Result<String, DaemonError> {
        if !self.store.name_taken(base, None)? {
            return Ok(base.to_string());
        }
        for i in 2..10_000 {
            let name = format!("{base}-{i}");
            if !self.store.name_taken(&name, None)? {
                return Ok(name);
            }
        }
        Ok(format!("{base}-{}", VmId::new()))
    }

    fn iso_users(&self, name: &str) -> Result<Vec<VmId>, DaemonError> {
        Ok(self
            .store
            .list()?
            .into_iter()
            .filter(|vm| {
                vm.spec
                    .disks
                    .iter()
                    .any(|disk| disk.iso_name.as_deref() == Some(name))
            })
            .map(|vm| vm.id)
            .collect())
    }

    fn require_volume_idle(&self, id: VolumeId, _op: &str) -> Result<(), DaemonError> {
        for vm_id in self.volume_users(id)? {
            let vm = self.store.get(vm_id)?;
            if vm.state == VmState::Running {
                return Err(DaemonError::MustBeStopped(vm.id, "change disk"));
            }
        }
        Ok(())
    }

    fn volume_users(&self, id: VolumeId) -> Result<Vec<VmId>, DaemonError> {
        Ok(self
            .store
            .list()?
            .into_iter()
            .filter(|vm| vm.spec.disks.iter().any(|disk| disk.volume_id == Some(id)))
            .map(|vm| vm.id)
            .collect())
    }

    fn volume_is_backing(&self, id: VolumeId) -> Result<bool, DaemonError> {
        Ok(self
            .volumes
            .list_volumes()?
            .iter()
            .any(|vol| vol.backing_id == Some(id)))
    }

    pub fn list_networks(&self) -> Result<Vec<NetworkRecord>, DaemonError> {
        Ok(self.networks.list()?)
    }

    fn default_clone_network_id(&self) -> Option<NetworkId> {
        let nets = self.networks.list().ok()?;
        nets.iter()
            .find(|n| n.mode == NetworkMode::Nat)
            .or_else(|| nets.first())
            .map(|n| n.id)
    }

    pub fn get_network(&self, id: NetworkId) -> Result<NetworkRecord, DaemonError> {
        Ok(self.networks.get(id)?)
    }

    pub fn create_network(&self, req: CreateNetworkRequest) -> Result<NetworkRecord, DaemonError> {
        Ok(self.networks.create(req)?)
    }

    pub fn delete_network(&self, id: NetworkId) -> Result<(), DaemonError> {
        if !self.network_users(id)?.is_empty() {
            return Err(DaemonError::NetworkBusy(id));
        }
        Ok(self.networks.delete(id)?)
    }

    pub fn attach_nic(&self, vm_id: VmId, req: AttachNicRequest) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(vm_id)?;
        self.require_stopped(&vm, "attach nic")?;
        let guests = self.store.list()?;
        let used_ips: Vec<String> = guests
            .iter()
            .flat_map(|guest| guest.spec.nets.iter())
            .filter_map(|nic| nic.ip.clone())
            .collect();
        let used_macs: Vec<String> = guests
            .iter()
            .filter(|guest| guest.id != vm_id)
            .flat_map(|guest| guest.spec.nets.iter())
            .filter_map(|nic| nic.mac.clone())
            .collect();
        let nic_index = u8::try_from(vm.spec.nets.len()).unwrap_or(0);
        let nic = self.networks.allocate_nic(
            req.network_id,
            vm.id,
            nic_index,
            req.ip.as_deref(),
            &used_ips,
            &used_macs,
            &self.cluster.self_id().as_bytes(),
        )?;
        vm.spec.nets.push(nic);
        self.store.upsert(vm.clone())?;
        Ok(vm)
    }

    pub fn detach_nic(&self, vm_id: VmId, tap: &str) -> Result<VmRecord, DaemonError> {
        let mut vm = self.store.get(vm_id)?;
        self.require_stopped(&vm, "detach nic")?;
        if let Some(nic) = vm
            .spec
            .nets
            .iter()
            .find(|nic| nic.tap.as_deref() == Some(tap))
            .cloned()
        {
            let _ = self.networks.release_nic(&nic);
        }
        vm.spec.nets.retain(|nic| nic.tap.as_deref() != Some(tap));
        self.store.upsert(vm.clone())?;
        Ok(vm)
    }

    pub fn console_info(&self, id: VmId) -> Result<ConsoleInfo, DaemonError> {
        let vm = self.store.get(id)?;
        let path = vm.serial_log.clone().or(vm.spec.serial_log.clone());
        let size = path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(ConsoleInfo {
            console_type: vm.spec.console_type,
            serial_log: path,
            graphics_socket: vm.graphics_socket.clone(),
            size,
            websocket: format!("/v1/vms/{id}/console/ws"),
            graphics_websocket: vm
                .graphics_socket
                .as_ref()
                .map(|_| format!("/v1/vms/{id}/graphics/ws")),
        })
    }

    pub async fn write_console(&self, id: VmId, text: &str) -> Result<(), DaemonError> {
        let vm = self.store.get(id)?;
        self.attach_console(&vm).await;
        let _ = self.console.write(id, text.as_bytes().to_vec()).await;
        Ok(())
    }

    pub async fn attach_console(&self, vm: &VmRecord) {
        let path = vm
            .serial_log
            .clone()
            .or_else(|| vm.spec.serial_log.clone())
            .unwrap_or_else(|| self.config.vmm.run_dir.join(format!("{}.serial", vm.id)));
        self.console
            .ensure(vm.id, path, vm.console_socket.clone())
            .await;
    }

    pub async fn subscribe_console(
        &self,
        id: VmId,
    ) -> Result<
        (
            tokio::sync::broadcast::Receiver<Vec<u8>>,
            tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        ),
        DaemonError,
    > {
        let vm = self.store.get(id)?;
        self.attach_console(&vm).await;
        self.console
            .subscribe(id)
            .await
            .ok_or_else(|| DaemonError::Peer(format!("console not ready for {id}")))
    }

    pub fn console_serial(
        &self,
        id: VmId,
        from: u64,
        max: u64,
    ) -> Result<SerialChunk, DaemonError> {
        let info = self.console_info(id)?;
        let Some(path) = info.serial_log else {
            return Ok(SerialChunk {
                from,
                next: from,
                text: String::new(),
            });
        };
        let bytes = std::fs::read(&path).unwrap_or_default();
        let start = usize::try_from(from).unwrap_or(0).min(bytes.len());
        let end = start.saturating_add(usize::try_from(max).unwrap_or(8192).min(64 * 1024));
        let end = end.min(bytes.len());
        let text = String::from_utf8_lossy(&bytes[start..end]).into_owned();
        Ok(SerialChunk {
            from,
            next: from + (end - start) as u64,
            text,
        })
    }

    fn iso_linux_boot_spec(&self, spec: &VmSpec) -> Result<VmSpec, DaemonError> {
        let mut spec = spec.clone();
        if spec.kernel.is_some() {
            return Ok(spec);
        }
        match self.driver() {
            DriverKind::CloudHypervisor | DriverKind::Qemu => {}
            DriverKind::Mock => return Ok(spec),
        }

        // Rocky/Alma/RHEL GenericCloud images ship Secure Boot shim as BOOTX64.EFI.
        // Cloud Hypervisor firmware cannot load it — kernel-boot from the BLS /boot FS.
        if matches!(self.driver(), DriverKind::CloudHypervisor) {
            if let Some(os_disk) = spec.disks.iter().find(|disk| !disk.cdrom) {
                let dest = self
                    .config
                    .storage
                    .root
                    .join("disk-boot")
                    .join(os_disk.volume_id.map(|id| id.to_string()).unwrap_or_else(|| {
                        os_disk
                            .path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "disk".into())
                    }));
                if let Some(boot) = pertisk_storage::prepare_shim_disk_boot(&os_disk.path, &dest)? {
                    tracing::info!(
                        disk = %os_disk.path.display(),
                        kernel = %boot.kernel.display(),
                        initramfs = %boot.initramfs.display(),
                        cmdline = %boot.cmdline,
                        "kernel-booting cloud disk (bypassing UEFI shim)"
                    );
                    spec.kernel = Some(boot.kernel);
                    spec.initramfs = Some(boot.initramfs);
                    if spec.cmdline.is_none() {
                        spec.cmdline = Some(boot.cmdline);
                    }
                    return Ok(spec);
                }
            }
        }

        let Some(disk) = spec
            .disks
            .iter()
            .find(|disk| disk.cdrom && !iso_is_cidata(disk))
        else {
            return Ok(spec);
        };
        let metal_iso = iso_is_metal(disk);
        // Installed disks boot via firmware — except Talos/metal ISOs, whose UKI is ~100MiB
        // and OVMF drops to the EFI shell instead of loading it.
        if !metal_iso
            && spec
                .disks
                .iter()
                .any(|disk| !disk.cdrom && pertisk_types::disk_likely_bootable(&disk.path))
        {
            return Ok(spec);
        }
        let name = disk
            .iso_name
            .as_deref()
            .map(std::path::Path::new)
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "iso".into());
        let dest = self.config.storage.root.join("iso-boot").join(&name);
        if let Some(boot) = pertisk_storage::prepare_linux_iso_boot(&disk.path, &dest)? {
            let initrd_mib = std::fs::metadata(&boot.initramfs)
                .map(|meta| meta.len() / (1024 * 1024))
                .unwrap_or(0) as u32;
            // The guest must hold the compressed initramfs plus its unpacked tmpfs copy; too
            // little RAM makes the kernel skip it and panic with "Unable to mount root fs".
            let talos = boot.cmdline.contains("talos.platform=");
            let needed = if talos {
                2048
            } else {
                (initrd_mib * 8).max(1024)
            };
            if spec.memory_mib < needed {
                return Err(pertisk_types::TypesError::InvalidSpec(format!(
                    "{name} boots a {initrd_mib} MiB initramfs and needs at least {needed} MiB of \
                     guest memory (VM has {}); Linux installers realistically want 2048+ MiB",
                    spec.memory_mib
                ))
                .into());
            }
            tracing::info!(
                iso = %name,
                kernel = %boot.kernel.display(),
                initramfs = %boot.initramfs.display(),
                initrd_mib,
                cmdline = %boot.cmdline,
                "kernel-booting installer ISO (bypassing UEFI shim)"
            );
            spec.kernel = Some(boot.kernel);
            spec.initramfs = Some(boot.initramfs);
            if spec.cmdline.is_none() {
                spec.cmdline = Some(boot.cmdline);
            }
        } else if metal_iso {
            return Err(pertisk_types::TypesError::InvalidSpec(format!(
                "{name} is a Talos/metal ISO but no /boot/vmlinuz was found to kernel-boot; \
                 OVMF cannot load the ~100MiB UKI and drops to the EFI shell"
            ))
            .into());
        }
        Ok(spec)
    }

    /// Drop installer ISOs from the boot config once the VM disk looks installed.
    fn prefer_disk_boot_spec(&self, spec: &VmSpec) -> VmSpec {
        let mut spec = spec.clone();
        let installed = spec
            .disks
            .iter()
            .any(|disk| !disk.cdrom && pertisk_types::disk_likely_bootable(&disk.path));
        if !installed {
            return spec;
        }
        let before = spec.disks.len();
        spec.disks.retain(|disk| {
            if !disk.cdrom || iso_is_cidata(disk) {
                return true;
            }
            // Talos metal ISO must stay attached so we can kernel-boot it; OVMF cannot load the UKI.
            iso_is_metal(disk)
        });
        if spec.disks.len() < before {
            tracing::info!(
                vm = %spec.name,
                "booting from disk; installer ISO omitted (detach ISO in Hardware to remove permanently)"
            );
        }
        spec
    }

    fn prefer_disk_recreate(&self, record: &VmRecord) -> bool {
        let has_installer = record
            .spec
            .disks
            .iter()
            .any(|disk| disk.cdrom && !iso_is_cidata(disk));
        let bootable = record
            .spec
            .disks
            .iter()
            .any(|disk| !disk.cdrom && pertisk_types::disk_likely_bootable(&disk.path));
        has_installer && bootable
    }

    /// Refuse to start when host disk or free RAM is too low (avoids ENOSPC / OOM mid-boot).
    fn ensure_start_capacity(&self, record: &VmRecord) -> Result<(), DaemonError> {
        const MIN_DISK_MIB: u64 = 1024;
        const DISK_HEADROOM_MIB: u64 = 2048;
        let storage_root = &self.config.storage.root;
        let avail_mib = free_space_mib(storage_root).or_else(|| free_space_mib(Path::new("/")));
        if let Some(avail) = avail_mib {
            if avail < MIN_DISK_MIB {
                return Err(DaemonError::Capacity(format!(
                    "only {avail} MiB free on storage (need ≥ {MIN_DISK_MIB} MiB); free disk or expand the appliance volume"
                )));
            }
            if avail < DISK_HEADROOM_MIB {
                tracing::warn!(
                    vm = %record.id,
                    avail_mib = avail,
                    "low disk space before guest start"
                );
            }
        }

        let need = u64::from(record.spec.memory_mib);
        let running_mem: u64 = self
            .store
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|vm| {
                vm.id != record.id
                    && vm.state == VmState::Running
                    && vm.node_id == Some(self.cluster.self_id())
            })
            .map(|vm| u64::from(vm.spec.memory_mib))
            .sum();
        if let Some(host_mib) = host_memory_mib() {
            let host_u32 = u32::try_from(host_mib.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
            let reserve = u64::from(cluster::host_memory_reserve_mib(host_u32));
            let free_for_guests = host_mib.saturating_sub(reserve);
            if !cluster::guest_start_fits(host_mib, running_mem, need) {
                return Err(DaemonError::Capacity(format!(
                    "guest needs {need} MiB; already running {running_mem} MiB of guests on a {host_mib} MiB host ({free_for_guests} MiB available after {reserve} MiB host reserve)"
                )));
            }
        }
        Ok(())
    }

    fn network_users(&self, id: NetworkId) -> Result<Vec<VmId>, DaemonError> {
        Ok(self
            .store
            .list()?
            .into_iter()
            .filter(|vm| vm.spec.nets.iter().any(|nic| nic.network_id == Some(id)))
            .map(|vm| vm.id)
            .collect())
    }

    fn require_quorum(&self) -> Result<(), DaemonError> {
        if !self.cluster.has_quorum() {
            return Err(DaemonError::NoQuorum);
        }
        if self.cluster.is_fenced() {
            return Err(DaemonError::Fenced);
        }
        Ok(())
    }

    fn loads(&self) -> Result<Vec<NodeLoad>, DaemonError> {
        let vms = self.store.list()?;
        let status = self.cluster.status(&[]);
        Ok(status
            .members
            .iter()
            .map(|m| {
                let placed: Vec<_> = vms
                    .iter()
                    .filter(|vm| vm.node_id == Some(m.id) && vm.state == VmState::Running)
                    .collect();
                NodeLoad {
                    id: m.id,
                    online: m.online,
                    cpus: m.cpus,
                    memory_mib: m.memory_mib,
                    used_vcpus: placed.iter().map(|vm| u32::from(vm.spec.vcpus)).sum(),
                    used_memory_mib: placed.iter().map(|vm| vm.spec.memory_mib).sum(),
                }
            })
            .collect())
    }

    fn start_memory_budget_mib(&self) -> Option<u32> {
        let host = host_memory_mib()?;
        let host_u32 = u32::try_from(host.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
        let self_id = self.cluster.self_id();
        let running: u32 = self
            .store
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|vm| vm.state == VmState::Running && vm.node_id == Some(self_id))
            .map(|vm| vm.spec.memory_mib)
            .sum();
        Some(cluster::guest_memory_budget_mib(host_u32).saturating_sub(running))
    }

    pub fn upload_tmp_path(&self, prefix: &str, ext: &str) -> Result<PathBuf, DaemonError> {
        let dir = self.config.storage.root.join("tmp");
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("{prefix}-{}.{ext}", uuid::Uuid::new_v4())))
    }

    fn pick_node(
        &self,
        spec: &VmSpec,
        prefer: Option<pertisk_types::NodeId>,
    ) -> Result<pertisk_types::NodeId, DaemonError> {
        self.cluster.touch_self();
        let loads = self.loads()?;
        let affinity = self.volume_affinity(spec);
        cluster::schedule_storage(&loads, spec, prefer, &affinity)
            .ok_or_else(|| DaemonError::Unschedulable(self.unschedulable_detail(&loads)))
    }

    /// Place a defined guest (create / template import). RAM is checked on start.
    fn pick_node_define(
        &self,
        spec: &VmSpec,
        prefer: Option<pertisk_types::NodeId>,
    ) -> Result<pertisk_types::NodeId, DaemonError> {
        self.cluster.touch_self();
        let loads = self.loads()?;
        let affinity = self.volume_affinity(spec);
        cluster::schedule_define(&loads, prefer, &affinity)
            .ok_or_else(|| DaemonError::Unschedulable(self.unschedulable_detail(&loads)))
    }

    fn unschedulable_detail(&self, loads: &[NodeLoad]) -> String {
        if loads.is_empty() {
            return "no members".into();
        }
        loads
            .iter()
            .map(|n| {
                format!(
                    "{} online={} vcpu {}/{} mem {}/{} MiB guest ({} MiB host)",
                    n.id,
                    n.online,
                    n.used_vcpus,
                    n.cpus,
                    n.used_memory_mib,
                    cluster::guest_memory_budget_mib(n.memory_mib),
                    n.memory_mib
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn volume_affinity(&self, spec: &VmSpec) -> Vec<pertisk_types::NodeId> {
        let mut sets = Vec::new();
        for disk in &spec.disks {
            let Some(id) = disk.volume_id else {
                continue;
            };
            let Ok(vol) = self.volumes.get_volume(id) else {
                continue;
            };
            if vol.backend == StorageBackend::Rbd || vol.replicas.is_empty() {
                continue;
            }
            sets.push(vol.replicas);
        }
        if sets.is_empty() {
            return Vec::new();
        }
        let mut acc = sets.remove(0);
        for set in sets {
            acc.retain(|id| set.contains(id));
        }
        acc
    }

    fn localize_disks(&self, record: &mut VmRecord) -> Result<(), DaemonError> {
        for disk in &mut record.spec.disks {
            let Some(id) = disk.volume_id else {
                continue;
            };
            let Ok(vol) = self.volumes.get_volume(id) else {
                continue;
            };
            if vol.backend == StorageBackend::Rbd {
                continue;
            }
            disk.path = self.volumes.local_path(vol.id, vol.format);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<pertisk_types::ClusterSnapshot, DaemonError> {
        let mut snap = self.cluster.membership_snapshot();
        snap.vms = self.store.list()?;
        snap.volumes = self.volumes.list_volumes()?;
        Ok(snap)
    }

    pub fn apply_snapshot(&self, snap: pertisk_types::ClusterSnapshot) -> Result<(), DaemonError> {
        if snap.generation < self.cluster.generation() {
            return Ok(());
        }
        self.cluster.apply_membership(&snap)?;
        self.store.replace_all(snap.vms)?;
        self.volumes.replace_records(snap.volumes)?;
        Ok(())
    }

    pub fn apply_accept(
        &self,
        node: pertisk_types::NodeRecord,
    ) -> Result<pertisk_types::ClusterSnapshot, DaemonError> {
        self.require_quorum()?;
        self.cluster.add_member(node)?;
        self.snapshot()
    }

    pub async fn accept_node(
        &self,
        node: pertisk_types::NodeRecord,
    ) -> Result<pertisk_types::ClusterSnapshot, DaemonError> {
        self.require_quorum()?;
        if !self.cluster.is_leader()
            && let Some(leader) = self.cluster.leader_id()
            && leader != self.cluster.self_id()
        {
            let snap: pertisk_types::ClusterSnapshot =
                self.peer_json(leader, "/v1/peer/accept", &node).await?;
            self.apply_snapshot(snap.clone())?;
            return Ok(snap);
        }
        let snap = self.apply_accept(node)?;
        self.replicate().await;
        Ok(snap)
    }

    pub async fn join_cluster(
        &self,
        peer: &str,
        username: &str,
        password: &str,
    ) -> Result<pertisk_types::ClusterStatus, DaemonError> {
        let peer = peer.trim_end_matches('/');
        let login: pertisk_api::TokenResponse = self
            .http
            .post(format!("{peer}/v1/login"))
            .json(&pertisk_api::LoginRequest {
                username: username.into(),
                password: password.into(),
            })
            .send()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .error_for_status()
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .json()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?;
        let remote: pertisk_types::ClusterStatus = self
            .http
            .get(format!("{peer}/v1/cluster"))
            .header("Authorization", format!("Bearer {}", login.token))
            .send()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .error_for_status()
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .json()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?;
        let snap: pertisk_types::ClusterSnapshot = self
            .http
            .post(format!("{peer}/v1/cluster/accept"))
            .header("Authorization", format!("Bearer {}", login.token))
            .json(&self.cluster.self_record())
            .send()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .error_for_status()
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .json()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?;
        self.apply_snapshot(snap)?;
        self.cluster
            .set_member_peer_url(remote.self_id, peer.to_string())?;
        self.cluster.touch(self.cluster.self_id(), None);
        let _ = self.cluster.heal_remote_peer_urls();
        Ok(self.cluster_status()?)
    }

    pub fn on_heartbeat(
        &self,
        msg: pertisk_types::HeartbeatMessage,
    ) -> Result<Option<pertisk_types::ClusterSnapshot>, DaemonError> {
        self.cluster.touch(msg.from, Some(msg.member));
        if let Some(snap) = msg.snapshot
            && snap.generation >= self.cluster.generation()
        {
            self.apply_snapshot(snap)?;
        }
        if self.cluster.is_leader() && msg.generation < self.cluster.generation() {
            return Ok(Some(self.snapshot()?));
        }
        Ok(None)
    }

    pub async fn cluster_tick(&self) -> Result<(), DaemonError> {
        self.cluster.touch_self();
        if crate::cluster::is_loopback_peer_url(&self.cluster.self_record().peer_url) {
            let url = crate::cluster::advertise_url(
                &self.config.daemon.listen,
                self.config.cluster.peer_url.as_deref(),
            );
            if !crate::cluster::is_loopback_peer_url(&url) {
                let _ = self.cluster.set_peer_url(url);
            }
        }
        let _ = self.cluster.heal_remote_peer_urls();
        let quorum = self.cluster.has_quorum();
        if self.cluster.set_fenced(!quorum) && !quorum {
            self.fence_local().await;
        }
        self.reconcile_local_vms().await;
        if quorum {
            self.recover_ha().await?;
            self.autostart_local().await;
        }
        self.send_heartbeats().await;
        if self.cluster.has_quorum()
            && self.cluster.is_leader()
            && let Ok(_guard) = self.rebuild.try_lock()
        {
            self.rebuild_volumes().await;
        }
        Ok(())
    }

    /// ACPI shutdown all running guests on this node (used before daemon exit).
    pub async fn shutdown_all_local_vms(&self) {
        let Ok(vms) = self.store.list() else {
            return;
        };
        let self_id = self.cluster.self_id();
        for vm in vms {
            if vm.state != VmState::Running || vm.node_id != Some(self_id) {
                continue;
            }
            tracing::info!(vm = %vm.id, "shutting down guest before daemon exit");
            if let Err(err) = self.shutdown_local(vm.id).await {
                tracing::warn!(vm = %vm.id, error = %err, "guest shutdown on daemon exit failed");
            }
        }
    }

    fn running_local_guest_count(&self) -> usize {
        let Ok(vms) = self.store.list() else {
            return 0;
        };
        let self_id = self.cluster.self_id();
        vms.iter()
            .filter(|vm| vm.state == VmState::Running && vm.node_id == Some(self_id))
            .count()
    }

    /// ACPI-stop local guests, then schedule hypervisor poweroff/reboot.
    pub fn begin_host_power(
        &self,
        user: &AuthUser,
        action: HostPowerAction,
    ) -> Result<HostPowerResult, DaemonError> {
        let override_cmd = self.config.daemon.host_power_cmd.as_deref();
        if override_cmd.is_none() && !cfg!(target_os = "linux") {
            return Err(DaemonError::HostPower(
                "hypervisor shutdown/reboot is only supported on Linux".into(),
            ));
        }
        let guests = self.running_local_guest_count();
        let kind = match action {
            HostPowerAction::Shutdown => "host.shutdown",
            HostPowerAction::Reboot => "host.reboot",
        };
        let task = self.begin_task(&user.username, kind, Some("host"))?;
        let svc = self.clone();
        let task_id = task.id.clone();
        tokio::spawn(async move {
            tracing::info!(
                action = action.as_str(),
                guests,
                "host power: shutting down local guests"
            );
            svc.shutdown_all_local_vms().await;
            let result = svc.execute_host_power(action).await;
            let mapped = result.map_err(|err| err.to_string());
            if let Err(err) = &mapped {
                tracing::error!(action = action.as_str(), error = %err, "host power failed");
            }
            let _ = svc.finish_task(&task_id, mapped);
        });
        Ok(HostPowerResult {
            ok: true,
            action: action.as_str().into(),
            guests,
        })
    }

    async fn execute_host_power(&self, action: HostPowerAction) -> Result<(), DaemonError> {
        tokio::time::sleep(host_power_delay()).await;
        let Some(argv) = host_power_argv(self.config.daemon.host_power_cmd.as_deref(), action)
        else {
            tracing::info!(action = action.as_str(), "host power skipped");
            return Ok(());
        };
        tracing::info!(command = ?argv, "host power");
        let mut cmd = tokio::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        let status = cmd
            .status()
            .await
            .map_err(|err| DaemonError::HostPower(format!("{}: {err}", argv[0])))?;
        if !status.success() {
            return Err(DaemonError::HostPower(format!(
                "{} exited {status}",
                argv.join(" ")
            )));
        }
        Ok(())
    }

    /// Poll the hypervisor until the guest exits, then mark the VM stopped.
    fn spawn_exit_watch(&self, record: &VmRecord) {
        if record.state != VmState::Running {
            return;
        }
        let self_id = self.cluster.self_id();
        if record.node_id != Some(self_id) {
            return;
        }
        let vmm = self.vmm.clone();
        let service = self.clone();
        let id = record.id;
        let snapshot = record.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let Ok(current) = service.store.get(id) else {
                    break;
                };
                if current.state != VmState::Running {
                    break;
                }
                if !vmm.is_running(&snapshot).await {
                    if let Err(err) = service.on_guest_exited(id).await {
                        tracing::warn!(vm = %id, error = %err, "guest exit cleanup failed");
                    }
                    break;
                }
            }
        });
    }

    /// Mark a VM stopped after the guest or hypervisor process exits without an API stop.
    async fn on_guest_exited(&self, id: VmId) -> Result<(), DaemonError> {
        let mut record = self.store.get(id)?;
        if record.state != VmState::Running {
            return Ok(());
        }
        if record.node_id != Some(self.cluster.self_id()) {
            return Ok(());
        }
        tracing::info!(vm = %id, "guest exited; marking stopped");
        self.console.drop_vm(id).await;
        let _ = self.vmm.stop(&record).await;
        record.state = VmState::Stopped;
        record.pid = None;
        record.last_error = None;
        self.store.upsert(record.clone())?;
        self.cluster.bump()?;
        self.replicate().await;
        self.sync_vm_volumes(&record).await;
        Ok(())
    }

    /// Correct stale Running state when the hypervisor process is already gone.
    pub(crate) async fn reconcile_local_vms(&self) {
        let self_id = self.cluster.self_id();
        let Ok(vms) = self.store.list() else {
            return;
        };
        for vm in vms {
            if vm.state != VmState::Running || vm.node_id != Some(self_id) {
                continue;
            }
            if !self.vmm.is_running(&vm).await
                && let Err(err) = self.on_guest_exited(vm.id).await
            {
                tracing::warn!(vm = %vm.id, error = %err, "reconcile stop failed");
            }
        }
    }

    async fn fence_local(&self) {
        let self_id = self.cluster.self_id();
        let Ok(vms) = self.store.list() else {
            return;
        };
        for vm in vms {
            if vm.node_id == Some(self_id) && vm.state == VmState::Running {
                let _ = self.vmm.stop(&vm).await;
                let mut stopped = vm;
                stopped.state = VmState::Stopped;
                let _ = self.store.upsert(stopped);
            }
        }
    }

    async fn recover_ha(&self) -> Result<(), DaemonError> {
        let loads = self.loads()?;
        let vms = self.store.list()?;
        for mut vm in vms {
            if vm.template || !vm.spec.ha || vm.state != VmState::Running {
                continue;
            }
            let Some(owner) = vm.node_id else {
                continue;
            };
            let owner_online = loads.iter().any(|n| n.id == owner && n.online);
            if owner_online {
                continue;
            }
            let affinity: Vec<_> = self
                .volume_affinity(&vm.spec)
                .into_iter()
                .filter(|id| loads.iter().any(|n| n.id == *id && n.online))
                .collect();
            eprintln!(
                "recovery candidate self={} vm={} owner={} affinity={:?} loads={:?}",
                self.cluster.self_id(),
                vm.id,
                owner,
                affinity,
                loads.iter().map(|n| (n.id, n.online)).collect::<Vec<_>>()
            );
            let Some(dest) =
                cluster::schedule_storage(&loads, &vm.spec, affinity.first().copied(), &affinity)
            else {
                continue;
            };
            if dest == owner {
                continue;
            }
            tracing::warn!(vm = %vm.id, from = %owner, to = %dest, "ha restart");
            vm.node_id = Some(dest);
            vm.state = VmState::Created;
            match self.peer_run(dest, vm.clone()).await {
                Ok(started) => {
                    let _ = self.store.upsert(started);
                }
                Err(err) => {
                    tracing::warn!(vm = %vm.id, %err, "ha restart failed");
                    vm.state = VmState::Failed;
                    vm.last_error = Some(err.to_string());
                    let _ = self.store.upsert(vm);
                }
            }
            self.cluster.bump()?;
        }
        self.replicate().await;
        Ok(())
    }

    /// Start local guests with `autostart` once per daemon lifetime, after `autostart_delay`.
    /// Guests created while this process is running are skipped (use Start after create).
    async fn autostart_local(&self) {
        if self.cluster.is_fenced() {
            return;
        }
        let self_id = self.cluster.self_id();
        let elapsed = self.started_at.elapsed().as_secs();
        let Ok(vms) = self.store.list() else {
            return;
        };
        let created: HashSet<VmId> = self
            .created_this_boot
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let mut candidates: Vec<_> = vms
            .into_iter()
            .filter(|vm| {
                !vm.template
                    && vm.spec.autostart
                    && vm.node_id == Some(self_id)
                    && !created.contains(&vm.id)
            })
            .collect();
        candidates.sort_by_key(|vm| (vm.spec.autostart_order, vm.spec.autostart_delay, vm.id));
        for vm in candidates {
            if elapsed < vm.spec.autostart_delay {
                continue;
            }
            {
                let mut done = self
                    .autostarted
                    .lock()
                    .unwrap_or_else(|err| err.into_inner());
                if !done.insert(vm.id) {
                    continue;
                }
            }
            if vm.state == VmState::Running {
                continue;
            }
            if !matches!(
                vm.state,
                VmState::Created | VmState::Stopped | VmState::Failed
            ) {
                continue;
            }
            tracing::info!(
                vm = %vm.id,
                delay = vm.spec.autostart_delay,
                order = vm.spec.autostart_order,
                "autostart"
            );
            if let Err(err) = self.ensure_start_capacity(&vm) {
                tracing::warn!(vm = %vm.id, error = %err, "autostart skipped (capacity)");
                let mut done = self.autostarted.lock().unwrap_or_else(|e| e.into_inner());
                done.remove(&vm.id);
                break;
            }
            if let Err(err) = self.start_local(vm.id).await {
                tracing::warn!(vm = %vm.id, error = %err, "autostart failed");
            }
            // Stagger heavy boots so qcow2 growth / memory pressure don't pile up.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    async fn send_heartbeats(&self) {
        let include = self.cluster.is_leader();
        let mut msg = self.cluster.heartbeat_out(include);
        if include {
            msg.snapshot = Some(
                self.snapshot()
                    .unwrap_or_else(|_| self.cluster.membership_snapshot()),
            );
        }
        for (_id, url) in self.cluster.peer_urls_except_self() {
            let url = format!("{}/v1/peer/heartbeat", url.trim_end_matches('/'));
            let _ = self
                .http
                .post(url)
                .timeout(std::time::Duration::from_secs(2))
                .header("x-pertisk-peer", self.cluster.secret())
                .json(&msg)
                .send()
                .await;
        }
    }

    async fn replicate(&self) {
        if !self.cluster.is_leader() {
            return;
        }
        let Ok(snap) = self.snapshot() else {
            return;
        };
        for (_id, url) in self.cluster.peer_urls_online_except_self() {
            let url = format!("{}/v1/peer/snapshot", url.trim_end_matches('/'));
            let _ = self
                .http
                .post(&url)
                .header("x-pertisk-peer", self.cluster.secret())
                .json(&snap)
                .send()
                .await;
        }
    }

    async fn peer_run(
        &self,
        dest: pertisk_types::NodeId,
        record: VmRecord,
    ) -> Result<VmRecord, DaemonError> {
        if dest == self.cluster.self_id() {
            return self.apply_run(record).await;
        }
        self.peer_json(dest, "/v1/peer/run", &record).await
    }

    async fn peer_stop(
        &self,
        dest: pertisk_types::NodeId,
        record: VmRecord,
    ) -> Result<VmRecord, DaemonError> {
        self.peer_json(dest, "/v1/peer/stop", &record).await
    }

    async fn peer_shutdown(
        &self,
        dest: pertisk_types::NodeId,
        record: VmRecord,
    ) -> Result<VmRecord, DaemonError> {
        self.peer_json(dest, "/v1/peer/shutdown", &record).await
    }

    async fn peer_restart(
        &self,
        dest: pertisk_types::NodeId,
        record: VmRecord,
    ) -> Result<VmRecord, DaemonError> {
        self.peer_json(dest, "/v1/peer/restart", &record).await
    }

    async fn peer_drop(
        &self,
        dest: pertisk_types::NodeId,
        record: &VmRecord,
    ) -> Result<(), DaemonError> {
        let _: serde_json::Value = self.peer_json(dest, "/v1/peer/drop", record).await?;
        Ok(())
    }

    async fn peer_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        dest: pertisk_types::NodeId,
        path: &str,
        body: &B,
    ) -> Result<T, DaemonError> {
        let url = self
            .cluster
            .member_url(dest)
            .ok_or_else(|| DaemonError::Peer(format!("unknown node {dest}")))?;
        let response = self
            .http
            .post(format!("{}{path}", url.trim_end_matches('/')))
            .header("x-pertisk-peer", self.cluster.secret())
            .json(body)
            .send()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(DaemonError::Peer(format!("{status}: {text}")));
        }
        serde_json::from_str(&text).map_err(|err| DaemonError::Peer(err.to_string()))
    }

    pub fn apply_ensure_volume(&self, record: VolumeRecord) -> Result<VolumeRecord, DaemonError> {
        Ok(self.volumes.ensure_local(&record)?)
    }

    pub fn apply_volume_blob(
        &self,
        id: VolumeId,
        bytes: &[u8],
    ) -> Result<VolumeRecord, DaemonError> {
        if self.volumes.get_volume(id).is_err() {
            return Err(DaemonError::Storage(StorageError::NotFound(id)));
        }
        Ok(self.volumes.write_blob(id, bytes)?)
    }

    pub fn volume_stat(&self, id: VolumeId) -> Result<serde_json::Value, DaemonError> {
        let (exists, size) = self.volumes.local_stat(id).unwrap_or((false, 0));
        Ok(serde_json::json!({ "exists": exists, "size": size }))
    }

    pub fn apply_delete_replica(&self, id: VolumeId) -> Result<(), DaemonError> {
        match self.volumes.delete_volume(id) {
            Ok(()) | Err(StorageError::NotFound(_)) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    async fn ensure_replicas(&self, record: &VolumeRecord) {
        let online = self.cluster.online_ids();
        for replica in &record.replicas {
            if *replica == self.cluster.self_id() {
                let _ = self.volumes.ensure_local(record);
                continue;
            }
            if !online.contains(replica) {
                continue;
            }
            let _ = self
                .peer_json::<VolumeRecord, _>(*replica, "/v1/peer/volumes/ensure", record)
                .await;
        }
    }

    async fn sync_volume_replicas(&self, record: &VolumeRecord) {
        if record.backend == StorageBackend::Rbd {
            return;
        }
        if !self.volumes.has_local(record.id, record.format) {
            return;
        }
        let online = self.cluster.online_ids();
        let has_remote = record
            .replicas
            .iter()
            .any(|replica| *replica != self.cluster.self_id() && online.contains(replica));
        if !has_remote {
            return;
        }
        let Ok(bytes) = self.volumes.read_blob(record.id) else {
            return;
        };
        for replica in &record.replicas {
            if *replica == self.cluster.self_id() {
                continue;
            }
            if !online.contains(replica) {
                continue;
            }
            let _ = self.peer_put_blob(*replica, record.id, &bytes).await;
        }
    }

    async fn sync_vm_volumes(&self, vm: &VmRecord) {
        for disk in &vm.spec.disks {
            let Some(id) = disk.volume_id else {
                continue;
            };
            if let Ok(vol) = self.volumes.get_volume(id) {
                self.sync_volume_replicas(&vol).await;
            }
        }
    }

    async fn rebuild_volumes(&self) {
        let Ok(vols) = self.volumes.list_volumes() else {
            return;
        };
        let online = self.cluster.online_ids();
        for mut vol in vols {
            if vol.backend == StorageBackend::Rbd {
                continue;
            }
            vol.replicas.retain(|id| online.contains(id));
            if vol.replicas.is_empty() {
                vol.replicas = cluster::place_replicas(
                    &online,
                    vol.replica_count.max(1),
                    Some(self.cluster.self_id()),
                );
            }
            while vol.replicas.len() < usize::from(vol.replica_count.max(1))
                && vol.replicas.len() < online.len()
            {
                if let Some(extra) = online.iter().find(|id| !vol.replicas.contains(id)) {
                    vol.replicas.push(*extra);
                } else {
                    break;
                }
            }
            let _ = self.volumes.put_record(vol.clone());
            self.ensure_replicas(&vol).await;
            if self.volumes.has_local(vol.id, vol.format) {
                self.sync_volume_replicas(&vol).await;
            } else if let Some(src) = vol
                .replicas
                .iter()
                .copied()
                .find(|id| *id != self.cluster.self_id())
                && let Ok(bytes) = self.peer_get_blob(src, vol.id).await
            {
                let _ = self.volumes.ensure_local(&vol);
                let _ = self.volumes.write_blob(vol.id, &bytes);
            }
        }
    }

    async fn peer_put_blob(
        &self,
        dest: pertisk_types::NodeId,
        id: VolumeId,
        bytes: &[u8],
    ) -> Result<(), DaemonError> {
        let url = self
            .cluster
            .member_url(dest)
            .ok_or_else(|| DaemonError::Peer(format!("unknown node {dest}")))?;
        let response = self
            .http
            .put(format!(
                "{}/v1/peer/volumes/{id}/blob",
                url.trim_end_matches('/')
            ))
            .header("x-pertisk-peer", self.cluster.secret())
            .header("content-type", "application/octet-stream")
            .body(bytes.to_vec())
            .timeout(std::time::Duration::from_millis(600))
            .send()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?;
        if !response.status().is_success() {
            return Err(DaemonError::Peer(format!(
                "{}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    async fn peer_get_blob(
        &self,
        dest: pertisk_types::NodeId,
        id: VolumeId,
    ) -> Result<Vec<u8>, DaemonError> {
        let url = self
            .cluster
            .member_url(dest)
            .ok_or_else(|| DaemonError::Peer(format!("unknown node {dest}")))?;
        let response = self
            .http
            .get(format!(
                "{}/v1/peer/volumes/{id}/blob",
                url.trim_end_matches('/')
            ))
            .header("x-pertisk-peer", self.cluster.secret())
            .timeout(std::time::Duration::from_millis(600))
            .send()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?;
        if !response.status().is_success() {
            return Err(DaemonError::Peer(format!("{}", response.status())));
        }
        Ok(response
            .bytes()
            .await
            .map_err(|err| DaemonError::Peer(err.to_string()))?
            .to_vec())
    }

    async fn peer_delete_volume(
        &self,
        dest: pertisk_types::NodeId,
        id: VolumeId,
    ) -> Result<(), DaemonError> {
        let url = self
            .cluster
            .member_url(dest)
            .ok_or_else(|| DaemonError::Peer(format!("unknown node {dest}")))?;
        let _ = self
            .http
            .delete(format!(
                "{}/v1/peer/volumes/{id}",
                url.trim_end_matches('/')
            ))
            .header("x-pertisk-peer", self.cluster.secret())
            .send()
            .await;
        Ok(())
    }

    pub fn leave_cluster(&self) -> Result<pertisk_types::ClusterStatus, DaemonError> {
        self.cluster.reset_solo()?;
        self.cluster_status()
    }
}

/// Fill / refresh NIC IPs from QEMU guest agent and/or the host neighbour table.
/// Returns true when the in-memory record changed (caller may persist).
fn enrich_observed_ips(vm: &mut VmRecord, run_dir: &Path) -> bool {
    let qga = run_dir.join(format!("{}.qga.sock", vm.id));
    let qga_ips = if vm.state == VmState::Running && qga.exists() {
        pertisk_vmm::qga_addrs_by_mac(&qga)
    } else {
        Default::default()
    };
    let serial_v4 = vm
        .serial_log
        .as_deref()
        .and_then(ipv4_from_serial_log);
    let blocked = host_blocked_ipv4s();
    let mut changed = false;
    for nic in &mut vm.spec.nets {
        let Some(mac) = nic.mac.as_deref() else {
            continue;
        };
        let want = pertisk_net::normalize_mac(mac);
        let observed_v4 = want
            .as_ref()
            .and_then(|want| {
                qga_ips
                    .ipv4
                    .iter()
                    .find(|(m, _)| m == want)
                    .map(|(_, ip)| ip.clone())
            })
            .or_else(|| pertisk_net::ipv4_for_mac(mac))
            .or_else(|| serial_v4.clone())
            .filter(|ip| is_guest_ipv4(ip) && !blocked.contains(ip));
        if nic
            .ip
            .as_deref()
            .is_some_and(|ip| blocked.contains(ip) || !is_guest_ipv4(ip))
        {
            nic.ip = None;
            changed = true;
        }
        if let Some(ip) = observed_v4 {
            if nic.ip.as_deref() != Some(ip.as_str()) {
                nic.ip = Some(ip);
                changed = true;
            }
        }
        let observed_v6 = {
            let mut candidates: Vec<String> = want
                .as_ref()
                .map(|want| {
                    qga_ips
                        .ipv6
                        .iter()
                        .filter(|(m, _)| m == want)
                        .map(|(_, ip)| ip.clone())
                        .collect()
                })
                .unwrap_or_default();
            let has_public = candidates.iter().any(|ip| {
                ip.parse::<std::net::Ipv6Addr>()
                    .ok()
                    .is_some_and(|addr| !addr.is_unique_local())
            });
            if !has_public {
                pertisk_net::probe_guest_ipv6_ll(mac);
                if let Some(ip) = pertisk_net::ipv6_for_mac(mac) {
                    candidates.push(ip);
                }
            }
            pertisk_types::prefer_ipv6(candidates.clone()).or_else(|| candidates.into_iter().next())
        };
        if let Some(ip) = observed_v6 {
            if nic.ipv6.as_deref() != Some(ip.as_str()) {
                nic.ipv6 = Some(ip);
                changed = true;
            }
        }
    }
    changed
}

/// Cloud images often print `https://A.B.C.D:9090/` (Cockpit) on the serial console.
/// Useful when ARP/neigh is cold (no ping on the host).
fn ipv4_from_serial_log(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let start = data.len().saturating_sub(64 * 1024);
    let text = String::from_utf8_lossy(&data[start..]);
    let mut from_device = None;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("ci-info") {
            continue;
        }
        // Route dumps list the DHCP gateway; device rows use scope "global".
        if !lower.contains("global") {
            continue;
        }
        for token in ipv4_tokens(line) {
            if is_guest_ipv4(token) {
                from_device = Some(token.to_string());
            }
        }
    }
    if from_device.is_some() {
        return from_device;
    }
    let mut last = None;
    let mut rest = text.as_ref();
    while let Some(idx) = rest.find("://") {
        let after = &rest[idx + 3..];
        let host = after
            .split(|c| c == '/' || c == ':' || c == ' ' || c == '\n' || c == '\r' || c == '\'')
            .next()
            .unwrap_or("");
        if is_guest_ipv4(host) {
            last = Some(host.to_string());
        }
        rest = &after[host.len().min(after.len())..];
        if rest.is_empty() {
            break;
        }
    }
    last
}

fn ipv4_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|t| !t.is_empty())
}

fn host_blocked_ipv4s() -> HashSet<String> {
    let mut out: HashSet<String> = probe_host_addrs().ipv4.into_iter().collect();
    if let Some(gw) = default_ipv4_gateway() {
        out.insert(gw);
    }
    out
}

fn default_ipv4_gateway() -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-4", "route", "show", "default"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let Some(via) = fields.iter().position(|f| *f == "via") else {
            continue;
        };
        let Some(ip) = fields.get(via + 1) else {
            continue;
        };
        if is_guest_ipv4(ip) {
            return Some((*ip).to_string());
        }
    }
    None
}

fn vm_needs_ip_probe(vm: &VmRecord) -> bool {
    // Always probe running guests so DHCP renewals after restart update inventory.
    vm.state == VmState::Running && vm.spec.nets.iter().any(|nic| nic.mac.is_some())
}

fn free_space_mib(path: &Path) -> Option<u64> {
    let probe = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new("/")).to_path_buf()
    };
    let out = std::process::Command::new("df")
        .args(["-Pk", probe.to_str()?])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Filesystem 1024-blocks Used Available Capacity Mounted on
    let line = text.lines().nth(1)?;
    let avail_kb: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail_kb / 1024)
}

fn host_memory_mib() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb / 1024);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostPowerAction {
    Shutdown,
    Reboot,
}

impl HostPowerAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shutdown => "shutdown",
            Self::Reboot => "reboot",
        }
    }

    fn systemd_unit(self) -> &'static str {
        match self {
            Self::Shutdown => "poweroff",
            Self::Reboot => "reboot",
        }
    }
}

fn host_power_delay() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(50)
    } else {
        std::time::Duration::from_secs(1)
    }
}

/// `None` means skip (tests, or an explicit `skip` override).
///
/// During `cargo test`, a missing override never runs `systemctl` — tests often
/// run as root on a hypervisor.
fn host_power_argv(override_cmd: Option<&str>, action: HostPowerAction) -> Option<Vec<String>> {
    match override_cmd.map(str::trim) {
        Some("") | Some("skip") | Some("none") | Some("off") => None,
        Some(template) => {
            let expanded = template.replace("{action}", action.systemd_unit());
            let args: Vec<String> = expanded.split_whitespace().map(str::to_string).collect();
            if args.is_empty() { None } else { Some(args) }
        }
        None => {
            if cfg!(test) {
                return None;
            }
            Some(vec![
                "systemctl".into(),
                "--no-block".into(),
                "--no-wall".into(),
                action.systemd_unit().into(),
            ])
        }
    }
}

fn iso_is_cidata(disk: &DiskSpec) -> bool {
    disk.iso_name
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .contains("cidata")
        || disk
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("cidata")
}

fn iso_is_metal(disk: &DiskSpec) -> bool {
    let name = disk
        .iso_name
        .as_deref()
        .or_else(|| disk.path.file_name().and_then(|n| n.to_str()))
        .unwrap_or("")
        .to_ascii_lowercase();
    name.contains("talos") || name.contains("metal-amd") || name.contains("metal-arm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::ControlStore;
    use pertisk_types::{
        AttachNicRequest, CreateNetworkRequest, CreateVolumeRequest, UpdateVmRequest, VmId,
        VolumeFormat, parse_size,
    };
    use pertisk_vmm::VmmBackend;

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vms.json")).unwrap();
        let volumes = VolumePool::open(dir.path().join("storage"), None).unwrap();
        let networks = NetworkPool::open(dir.path().join("net"), false).unwrap();
        let control = ControlStore::open(dir.path().join("control.db"), Some("admin")).unwrap();
        let config = HostConfig::default_for(dir.path());
        let vmm =
            VmmBackend::from_config(DriverKind::Mock, None, dir.path().join("run"), None).unwrap();
        (
            Service::new(
                vmm,
                store,
                volumes,
                networks,
                control,
                config,
                dir.path().to_path_buf(),
            ),
            dir,
        )
    }

    fn spec(name: &str) -> VmSpec {
        VmSpec {
            name: name.into(),
            vcpus: 1,
            memory_mib: 512,
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

    fn vm_id(id: u64) -> VmId {
        id.to_string().parse().unwrap()
    }

    #[test]
    fn host_power_argv_skips_and_expands() {
        assert_eq!(host_power_argv(Some("skip"), HostPowerAction::Reboot), None);
        assert_eq!(
            host_power_argv(Some("touch /tmp/host-{action}"), HostPowerAction::Shutdown),
            Some(vec!["touch".into(), "/tmp/host-poweroff".into()])
        );
        assert_eq!(
            host_power_argv(Some("touch /tmp/host-{action}"), HostPowerAction::Reboot),
            Some(vec!["touch".into(), "/tmp/host-reboot".into()])
        );
        assert_eq!(host_power_argv(None, HostPowerAction::Reboot), None);
    }

    #[tokio::test]
    async fn create_start_stop_destroy() {
        let (svc, _dir) = service();
        let vm = svc.create(vm_id(100), spec("demo")).await.unwrap();
        assert_eq!(vm.state, VmState::Created);
        let vm = svc.start(vm.id).await.unwrap();
        assert_eq!(vm.state, VmState::Running);
        let chunk = svc.console_serial(vm.id, 0, 4096).unwrap();
        assert!(
            chunk.text.contains("started"),
            "expected boot serial, got {:?}",
            chunk.text
        );
        svc.write_console(vm.id, "help\n").await.unwrap();
        let mut echoed = false;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let chunk = svc.console_serial(vm.id, 0, 4096).unwrap();
            if chunk.text.contains("help") {
                echoed = true;
                break;
            }
        }
        assert!(echoed, "console input was not captured");
        let vm = svc.stop(vm.id).await.unwrap();
        assert_eq!(vm.state, VmState::Stopped);
        svc.destroy(vm.id).await.unwrap();
        assert!(svc.get(vm.id).is_err());
    }

    #[tokio::test]
    async fn define_template_ignores_host_ram() {
        let (svc, _dir) = service();
        let mut spec = spec("cloud-tpl");
        spec.memory_mib = 4096;
        let vm = svc.create(vm_id(100), spec).await.unwrap();
        assert_eq!(vm.spec.memory_mib, 4096);
        assert_eq!(vm.state, VmState::Created);
        let tpl = svc.convert_to_template(vm.id).await.unwrap();
        assert!(tpl.template);
    }

    #[tokio::test]
    async fn clone_defines_when_memory_exceeds_host() {
        let (svc, _dir) = service();
        let mut spec = spec("cloud-tpl");
        spec.memory_mib = 4096;
        let tpl = svc.create(vm_id(100), spec).await.unwrap();
        let tpl = svc.convert_to_template(tpl.id).await.unwrap();
        let guest = svc
            .clone_vm(
                tpl.id,
                CloneVmRequest {
                    id: Some(vm_id(101)),
                    name: "web-1".into(),
                    linked: false,
                    vcpus: None,
                    memory_mib: Some(4096),
                    ha: Some(false),
                    autostart: Some(false),
                    network_id: None,
                    ip: None,
                    cloud_init: None,
                    disk_size_bytes: None,
                    start: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(guest.spec.name, "web-1");
        assert_eq!(guest.spec.memory_mib, 4096);
        assert_ne!(guest.state, VmState::Running);
        assert!(!guest.template);
    }

    #[tokio::test]
    async fn clone_attaches_default_nat_when_template_has_no_nic() {
        let (svc, _dir) = service();
        let net = svc
            .create_network(CreateNetworkRequest {
                name: "lan".into(),
                cidr: "10.88.0.0/24".into(),
                gateway: None,
                bridge: Some("vmbr0".into()),
                dhcp: true,
                isolate: true,
                mode: Default::default(),
            })
            .unwrap();
        let tpl = svc.create(vm_id(100), spec("cloud-tpl")).await.unwrap();
        let tpl = svc.convert_to_template(tpl.id).await.unwrap();
        let guest = svc
            .clone_vm(
                tpl.id,
                CloneVmRequest {
                    id: Some(vm_id(101)),
                    name: "web-1".into(),
                    linked: false,
                    vcpus: None,
                    memory_mib: None,
                    ha: Some(false),
                    autostart: Some(false),
                    network_id: None,
                    ip: None,
                    cloud_init: None,
                    disk_size_bytes: None,
                    start: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(guest.spec.nets.len(), 1);
        assert_eq!(guest.spec.nets[0].network_id, Some(net.id));
    }

    #[test]
    fn serial_log_uses_ci_info_address_not_gateway_or_netmask() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serial.log");
        std::fs::write(
            &path,
            "\
ci-info: +++++++++++++++++++++++++++++Net device info+++++++++++++++++++++++++++++
ci-info: | Device |  Up  |   Address    |      Mask     | Scope  |     Hw-Address    |
ci-info: |  ens3  | True |  10.1.1.42   | 255.255.255.0 | global | 52:54:00:2e:3b:6a |
ci-info: ++++++++++++++++++++++++++++++++Route info+++++++++++++++++++++++++++++++
ci-info: | Route | Destination |  Gateway  | Interface |
ci-info: |   0   |   0.0.0.0   | 10.1.1.10 |   ens3    |
",
        )
        .unwrap();
        assert_eq!(
            ipv4_from_serial_log(&path).as_deref(),
            Some("10.1.1.42")
        );
    }

    #[tokio::test]
    async fn update_spec_when_stopped() {
        let (svc, _dir) = service();
        let vm = svc.create(vm_id(101), spec("demo")).await.unwrap();
        let vm = svc
            .update(
                vm.id,
                UpdateVmRequest {
                    name: Some("web".into()),
                    vcpus: Some(2),
                    memory_mib: Some(1024),
                    ha: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(vm.spec.name, "web");
        assert_eq!(vm.spec.vcpus, 2);
        assert_eq!(vm.spec.memory_mib, 1024);
        assert!(!vm.spec.ha);
        let running = svc.start(vm.id).await.unwrap();
        let err = svc
            .update(
                running.id,
                UpdateVmRequest {
                    vcpus: Some(4),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::MustBeStopped(_, _)));
        let running = svc
            .update(
                running.id,
                UpdateVmRequest {
                    ha: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(running.spec.ha);
    }

    #[tokio::test]
    async fn autostart_starts_existing_vm_on_tick() {
        let (svc, _dir) = service();
        let mut spec = spec("boot");
        spec.autostart = true;
        let vm = svc.create(vm_id(200), spec).await.unwrap();
        svc.created_this_boot.lock().unwrap().remove(&vm.id);
        svc.cluster_tick().await.unwrap();
        assert_eq!(svc.get(vm.id).unwrap().state, VmState::Running);
    }

    #[tokio::test]
    async fn autostart_skips_vm_created_this_boot() {
        let (svc, _dir) = service();
        let mut spec = spec("new");
        spec.autostart = true;
        let vm = svc.create(vm_id(201), spec).await.unwrap();
        svc.cluster_tick().await.unwrap();
        assert_eq!(svc.get(vm.id).unwrap().state, VmState::Created);
    }

    #[tokio::test]
    async fn autostart_respects_delay() {
        let (svc, _dir) = service();
        let mut spec = spec("later");
        spec.autostart = true;
        spec.autostart_delay = 3600;
        let vm = svc.create(vm_id(202), spec).await.unwrap();
        svc.created_this_boot.lock().unwrap().remove(&vm.id);
        svc.cluster_tick().await.unwrap();
        assert_eq!(svc.get(vm.id).unwrap().state, VmState::Created);
    }

    #[tokio::test]
    async fn rejects_duplicate_name() {
        let (svc, _dir) = service();
        svc.create(vm_id(102), spec("demo")).await.unwrap();
        let err = svc.create(vm_id(103), spec("demo")).await.unwrap_err();
        assert!(matches!(err, DaemonError::NameTaken(_)));
    }

    #[tokio::test]
    async fn attach_volume_and_iso() {
        let (svc, dir) = service();
        let vm = svc.create(vm_id(104), spec("demo")).await.unwrap();
        let vol = svc
            .create_volume(CreateVolumeRequest {
                name: "root".into(),
                size_bytes: parse_size("8M").unwrap(),
                format: VolumeFormat::Raw,
                replicas: None,
            })
            .await
            .unwrap();
        let vm = svc
            .attach_disk(vm.id, AttachDiskRequest { volume_id: vol.id })
            .unwrap();
        assert_eq!(vm.spec.disks.len(), 1);
        assert_eq!(vm.spec.disks[0].volume_id, Some(vol.id));

        let iso_src = dir.path().join("os.iso");
        std::fs::write(&iso_src, b"iso").unwrap();
        svc.import_iso(ImportIsoRequest {
            path: iso_src,
            name: None,
        })
        .unwrap();
        let vm = svc
            .attach_iso(
                vm.id,
                AttachIsoRequest {
                    iso: "os.iso".into(),
                },
            )
            .unwrap();
        assert_eq!(vm.spec.disks.len(), 2);
        assert!(svc.delete_volume(vol.id).await.is_err());
        svc.detach_disk(vm.id, vol.id).unwrap();
        svc.delete_volume(vol.id).await.unwrap();
    }

    #[tokio::test]
    async fn destroy_deletes_exclusive_volume() {
        let (svc, _dir) = service();
        let vm = svc.create(vm_id(105), spec("demo")).await.unwrap();
        let vol = svc
            .create_volume(CreateVolumeRequest {
                name: "root".into(),
                size_bytes: parse_size("8M").unwrap(),
                format: VolumeFormat::Raw,
                replicas: None,
            })
            .await
            .unwrap();
        svc.attach_disk(vm.id, AttachDiskRequest { volume_id: vol.id })
            .unwrap();
        svc.destroy(vm.id).await.unwrap();
        assert!(svc.get_volume(vol.id).is_err());
    }

    #[tokio::test]
    async fn destroy_leaves_unattached_volume() {
        let (svc, _dir) = service();
        let vm = svc.create(vm_id(106), spec("demo")).await.unwrap();
        let vol = svc
            .create_volume(CreateVolumeRequest {
                name: "spare".into(),
                size_bytes: parse_size("8M").unwrap(),
                format: VolumeFormat::Raw,
                replicas: None,
            })
            .await
            .unwrap();
        svc.destroy(vm.id).await.unwrap();
        assert!(svc.get_volume(vol.id).is_ok());
    }

    #[tokio::test]
    async fn attach_nic_and_console() {
        let (svc, _dir) = service();
        let vm = svc.create(vm_id(107), spec("demo")).await.unwrap();
        let net = svc
            .create_network(CreateNetworkRequest {
                name: "lan".into(),
                cidr: "10.88.0.0/24".into(),
                gateway: None,
                bridge: Some("vmbr0".into()),
                dhcp: true,
                isolate: true,
                mode: Default::default(),
            })
            .unwrap();
        let vm = svc
            .attach_nic(
                vm.id,
                AttachNicRequest {
                    network_id: net.id,
                    ip: None,
                },
            )
            .unwrap();
        assert_eq!(vm.spec.nets.len(), 1);
        assert_eq!(vm.spec.nets[0].ip.as_deref(), Some("10.88.0.2"));
        let vm = svc.start(vm.id).await.unwrap();
        let chunk = svc.console_serial(vm.id, 0, 4096).unwrap();
        assert!(chunk.text.contains("started"));
        svc.stop(vm.id).await.unwrap();
        svc.detach_nic(vm.id, vm.spec.nets[0].tap.as_deref().unwrap())
            .unwrap();
        svc.delete_network(net.id).unwrap();
    }
}
