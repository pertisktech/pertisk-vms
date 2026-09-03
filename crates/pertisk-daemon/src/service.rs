use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pertisk_net::{NetError, NetworkPool};
use pertisk_storage::{Rbd, StorageError, VolumePool};
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, AttachNicRequest, CloneVolumeRequest, ConsoleInfo,
    CreateNetworkRequest, CreateVolumeRequest, DiskSpec, DriverKind, HostConfig, HostInfo,
    ImportIsoRequest, IsoRecord, NetworkId, NetworkRecord, ResizeVolumeRequest, SerialChunk,
    SnapshotRequest, StorageBackend, UpdateVmRequest, VmId, VmRecord, VmSpec, VmState,
    VolumeFormat, VolumeId, VolumeRecord, probe_host, ClusterMetrics, NodeMetrics, VmMetrics,
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
                .connect_timeout(std::time::Duration::from_millis(250))
                .timeout(std::time::Duration::from_secs(5))
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
        info
    }

    pub fn node_metrics(&self) -> Result<NodeMetrics, DaemonError> {
        let live = metrics::sample_host(&self.metrics, &self.config.storage.root);
        let self_id = self.cluster.self_id();
        let status = self.cluster_status()?;
        let me = status
            .members
            .iter()
            .find(|m| m.id == self_id)
            .cloned();
        let vms = self.store.list()?;
        let running: Vec<_> = vms
            .iter()
            .filter(|vm| vm.node_id == Some(self_id) && vm.state == VmState::Running)
            .collect();
        Ok(NodeMetrics {
            node_id: self_id,
            name: me
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| self.config.cluster.node_name.clone().unwrap_or_else(|| "local".into())),
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
            total_vms: vms.len() as u32,
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
        let dest = self.pick_node(&spec, None)?;
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

    pub async fn start(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.require_quorum()?;
        let mut record = self.store.get(id)?;
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
        let mut record = self.volumes.clone_volume(id, req)?;
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

    pub fn create_cloudinit_iso(
        &self,
        req: pertisk_types::CloudInitIsoRequest,
    ) -> Result<IsoRecord, DaemonError> {
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

    pub fn list_networks(&self) -> Result<Vec<NetworkRecord>, DaemonError> {
        Ok(self.networks.list()?)
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
        let used_ips: Vec<String> = self
            .store
            .list()?
            .into_iter()
            .flat_map(|guest| guest.spec.nets)
            .filter_map(|nic| nic.ip)
            .collect();
        let nic_index = u8::try_from(vm.spec.nets.len()).unwrap_or(0);
        let nic = self.networks.allocate_nic(
            req.network_id,
            vm.id,
            nic_index,
            req.ip.as_deref(),
            &used_ips,
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
            // Keep ~1.5 GiB for the host daemon / nested KVM.
            let reserve = 1536u64;
            let free_for_guests = host_mib.saturating_sub(reserve);
            if running_mem.saturating_add(need) > free_for_guests {
                return Err(DaemonError::Capacity(format!(
                    "guest needs {need} MiB; already running {running_mem} MiB of guests on a {host_mib} MiB host (reserve {reserve} MiB)"
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

    fn pick_node(
        &self,
        spec: &VmSpec,
        prefer: Option<pertisk_types::NodeId>,
    ) -> Result<pertisk_types::NodeId, DaemonError> {
        self.cluster.touch_self();
        let loads = self.loads()?;
        let affinity = self.volume_affinity(spec);
        cluster::schedule_storage(&loads, spec, prefer, &affinity).ok_or_else(|| {
            let detail = if loads.is_empty() {
                "no members".into()
            } else {
                loads
                    .iter()
                    .map(|n| {
                        format!(
                            "{} online={} vcpu {}/{} mem {}/{} MiB",
                            n.id, n.online, n.used_vcpus, n.cpus, n.used_memory_mib, n.memory_mib
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            DaemonError::Unschedulable(detail)
        })
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
        self.cluster.touch(self.cluster.self_id(), None);
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
            if !vm.spec.ha || vm.state != VmState::Running {
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
                vm.spec.autostart && vm.node_id == Some(self_id) && !created.contains(&vm.id)
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
                .timeout(std::time::Duration::from_millis(400))
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
        let Ok(bytes) = self.volumes.read_blob(record.id) else {
            return;
        };
        let online = self.cluster.online_ids();
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

fn vm_needs_ip_probe(vm: &VmRecord) -> bool {
    vm.state == VmState::Running
        && vm
            .spec
            .nets
            .iter()
            .any(|nic| nic.ip.is_none() && nic.mac.is_some())
}

/// Fill missing NIC IPs from QEMU guest agent and/or the host neighbour table.
/// Returns true when the in-memory record gained an IP (caller may persist).
fn enrich_observed_ips(vm: &mut VmRecord, run_dir: &Path) -> bool {
    let qga = run_dir.join(format!("{}.qga.sock", vm.id));
    let qga_ips = if vm.state == VmState::Running && qga.exists() {
        pertisk_vmm::qga_ipv4_by_mac(&qga)
    } else {
        Vec::new()
    };
    let mut changed = false;
    for nic in &mut vm.spec.nets {
        if nic.ip.is_some() {
            continue;
        }
        let Some(mac) = nic.mac.as_deref() else {
            continue;
        };
        let want = pertisk_net::normalize_mac(mac);
        let observed = want
            .as_ref()
            .and_then(|want| {
                qga_ips
                    .iter()
                    .find(|(m, _)| m == want)
                    .map(|(_, ip)| ip.clone())
            })
            .or_else(|| pertisk_net::ipv4_for_mac(mac));
        if let Some(ip) = observed {
            nic.ip = Some(ip);
            changed = true;
        }
    }
    changed
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
