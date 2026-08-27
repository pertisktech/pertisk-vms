use std::sync::Arc;

use pertisk_storage::{StorageError, VolumePool};
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, CloneVolumeRequest, CreateVolumeRequest, DiskSpec,
    DriverKind, HostConfig, HostInfo, ImportIsoRequest, IsoRecord, ResizeVolumeRequest,
    SnapshotRequest, VmId, VmRecord, VmSpec, VmState, VolumeId, VolumeRecord, probe_host,
};
use pertisk_vmm::VmmBackend;
use thiserror::Error;

use crate::Store;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("vm not found: {0}")]
    NotFound(VmId),
    #[error("vm name already exists: {0}")]
    NameTaken(String),
    #[error("vm {0} must be stopped to {1}")]
    MustBeStopped(VmId, &'static str),
    #[error("volume {0} is attached to a vm")]
    VolumeBusy(VolumeId),
    #[error("iso {0} is attached to a vm")]
    IsoBusy(String),
    #[error(transparent)]
    Types(#[from] pertisk_types::TypesError),
    #[error(transparent)]
    Vmm(#[from] pertisk_vmm::VmmError),
    #[error(transparent)]
    Storage(#[from] StorageError),
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
    config: HostConfig,
    data_dir: std::path::PathBuf,
}

impl Service {
    pub fn new(
        vmm: VmmBackend,
        store: Store,
        volumes: VolumePool,
        config: HostConfig,
        data_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            vmm: Arc::new(vmm),
            store: Arc::new(store),
            volumes: Arc::new(volumes),
            config,
            data_dir,
        }
    }

    pub fn driver(&self) -> DriverKind {
        self.vmm.kind()
    }

    pub fn host_info(&self) -> HostInfo {
        probe_host(&self.config, self.data_dir.clone())
    }

    pub fn list(&self) -> Result<Vec<VmRecord>, DaemonError> {
        self.store.list()
    }

    pub fn get(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.store.get(id)
    }

    pub async fn create(&self, spec: VmSpec) -> Result<VmRecord, DaemonError> {
        spec.validate()?;
        if self.store.name_taken(&spec.name, None)? {
            return Err(DaemonError::NameTaken(spec.name));
        }
        let id = VmId::new();
        let created = self.vmm.create(id, &spec).await?;
        let record = VmRecord {
            id,
            spec,
            state: VmState::Created,
            pid: created.pid,
            api_socket: created.api_socket,
            serial_log: created.serial_log,
            last_error: None,
        };
        self.store.upsert(record.clone())?;
        Ok(record)
    }

    pub async fn start(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let mut record = self.store.get(id)?;
        match record.state {
            VmState::Created | VmState::Stopped => {}
            state => {
                return Err(pertisk_vmm::VmmError::InvalidState {
                    state,
                    op: "start",
                }
                .into());
            }
        }
        match self.vmm.start(&record).await {
            Ok(started) => {
                record.state = VmState::Running;
                if started.pid.is_some() {
                    record.pid = started.pid;
                }
                record.last_error = None;
                self.store.upsert(record.clone())?;
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
        let mut record = self.store.get(id)?;
        self.vmm.stop(&record).await?;
        record.state = VmState::Stopped;
        record.last_error = None;
        self.store.upsert(record.clone())?;
        Ok(record)
    }

    pub async fn destroy(&self, id: VmId) -> Result<(), DaemonError> {
        let record = self.store.get(id)?;
        self.vmm.destroy(&record).await?;
        self.store.remove(id)?;
        Ok(())
    }

    pub fn list_volumes(&self) -> Result<Vec<VolumeRecord>, DaemonError> {
        Ok(self.volumes.list_volumes()?)
    }

    pub fn get_volume(&self, id: VolumeId) -> Result<VolumeRecord, DaemonError> {
        Ok(self.volumes.get_volume(id)?)
    }

    pub fn create_volume(&self, req: CreateVolumeRequest) -> Result<VolumeRecord, DaemonError> {
        Ok(self.volumes.create_volume(req)?)
    }

    pub fn delete_volume(&self, id: VolumeId) -> Result<(), DaemonError> {
        if !self.volume_users(id)?.is_empty() {
            return Err(DaemonError::VolumeBusy(id));
        }
        Ok(self.volumes.delete_volume(id)?)
    }

    pub fn resize_volume(
        &self,
        id: VolumeId,
        req: ResizeVolumeRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "resize")?;
        Ok(self.volumes.resize(id, req)?)
    }

    pub fn clone_volume(
        &self,
        id: VolumeId,
        req: CloneVolumeRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "clone")?;
        Ok(self.volumes.clone_volume(id, req)?)
    }

    pub fn snapshot_volume(
        &self,
        id: VolumeId,
        req: SnapshotRequest,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "snapshot")?;
        Ok(self.volumes.snapshot(id, req)?)
    }

    pub fn restore_volume(
        &self,
        id: VolumeId,
        name: &str,
    ) -> Result<VolumeRecord, DaemonError> {
        self.require_volume_idle(id, "restore")?;
        Ok(self.volumes.restore_snapshot(id, name)?)
    }

    pub fn list_isos(&self) -> Result<Vec<IsoRecord>, DaemonError> {
        Ok(self.volumes.list_isos()?)
    }

    pub fn import_iso(&self, req: ImportIsoRequest) -> Result<IsoRecord, DaemonError> {
        Ok(self.volumes.import_iso(&req.path, req.name)?)
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
        vm.spec.disks.push(DiskSpec {
            path: volume.path,
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
            .filter(|vm| {
                vm.spec
                    .disks
                    .iter()
                    .any(|disk| disk.volume_id == Some(id))
            })
            .map(|vm| vm.id)
            .collect())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::{CreateVolumeRequest, VolumeFormat, parse_size};
    use pertisk_vmm::VmmBackend;

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vms.json")).unwrap();
        let volumes = VolumePool::open(dir.path().join("storage"), None).unwrap();
        let config = HostConfig::default_for(dir.path());
        let vmm = VmmBackend::from_config(DriverKind::Mock, None, dir.path().join("run")).unwrap();
        (
            Service::new(vmm, store, volumes, config, dir.path().to_path_buf()),
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
            disks: vec![],
            nets: vec![],
            serial_log: None,
        }
    }

    #[tokio::test]
    async fn create_start_stop_destroy() {
        let (svc, _dir) = service();
        let vm = svc.create(spec("demo")).await.unwrap();
        assert_eq!(vm.state, VmState::Created);
        let vm = svc.start(vm.id).await.unwrap();
        assert_eq!(vm.state, VmState::Running);
        let vm = svc.stop(vm.id).await.unwrap();
        assert_eq!(vm.state, VmState::Stopped);
        svc.destroy(vm.id).await.unwrap();
        assert!(svc.get(vm.id).is_err());
    }

    #[tokio::test]
    async fn rejects_duplicate_name() {
        let (svc, _dir) = service();
        svc.create(spec("demo")).await.unwrap();
        let err = svc.create(spec("demo")).await.unwrap_err();
        assert!(matches!(err, DaemonError::NameTaken(_)));
    }

    #[tokio::test]
    async fn attach_volume_and_iso() {
        let (svc, dir) = service();
        let vm = svc.create(spec("demo")).await.unwrap();
        let vol = svc
            .create_volume(CreateVolumeRequest {
                name: "root".into(),
                size_bytes: parse_size("8M").unwrap(),
                format: VolumeFormat::Raw,
            })
            .unwrap();
        let vm = svc
            .attach_disk(
                vm.id,
                AttachDiskRequest {
                    volume_id: vol.id,
                },
            )
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
            .attach_iso(vm.id, AttachIsoRequest { iso: "os.iso".into() })
            .unwrap();
        assert_eq!(vm.spec.disks.len(), 2);
        assert!(svc.delete_volume(vol.id).is_err());
        svc.detach_disk(vm.id, vol.id).unwrap();
        svc.delete_volume(vol.id).unwrap();
    }
}
