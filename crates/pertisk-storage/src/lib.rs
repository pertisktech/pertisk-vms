//! Local directory volumes, file snapshots, ISO library, optional qemu-img, optional Ceph RBD.

mod iso9660;
mod qemu;
mod rbd;

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pertisk_types::{
    CloneVolumeRequest, CloudInitIsoRequest, CreateVolumeRequest, IsoRecord, ResizeVolumeRequest,
    SnapshotRequest, StorageBackend, VolumeFormat, VolumeId, VolumeRecord, VolumeSnapshot,
};
use serde::{Deserialize, Serialize};

use crate::iso9660::cidata_iso;
use crate::qemu::QemuImg;
pub use rbd::Rbd;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("volume not found: {0}")]
    NotFound(VolumeId),
    #[error("volume name already exists: {0}")]
    NameTaken(String),
    #[error("iso not found: {0}")]
    IsoNotFound(String),
    #[error("iso already exists: {0}")]
    IsoExists(String),
    #[error("invalid iso name: {0}")]
    InvalidIsoName(String),
    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),
    #[error("snapshot already exists: {0}")]
    SnapshotExists(String),
    #[error("qcow2 requires qemu-img")]
    QemuImgRequired,
    #[error("linked clone requires qemu-img")]
    LinkedRequiresQemu,
    #[error("cannot shrink volume from {from} to {to} bytes")]
    CannotShrink { from: u64, to: u64 },
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Serialize, Deserialize, Default)]
struct Inventory {
    volumes: BTreeMap<VolumeId, VolumeRecord>,
    isos: BTreeMap<String, IsoRecord>,
}

#[derive(Debug)]
pub struct VolumePool {
    root: PathBuf,
    qemu: QemuImg,
    inner: Mutex<Inventory>,
    inventory_path: PathBuf,
}

impl VolumePool {
    pub fn open(root: impl Into<PathBuf>, qemu_img: Option<PathBuf>) -> Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(root.join("disks"))?;
        std::fs::create_dir_all(root.join("iso"))?;
        std::fs::create_dir_all(root.join("snapshots"))?;
        let inventory_path = root.join("inventory.json");
        let inner = if inventory_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&inventory_path)?)?
        } else {
            Inventory::default()
        };
        Ok(Self {
            qemu: QemuImg::new(qemu_img),
            root,
            inner: Mutex::new(inner),
            inventory_path,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn qemu_img(&self) -> Option<&Path> {
        self.qemu.binary()
    }

    pub fn local_path(&self, id: VolumeId, format: VolumeFormat) -> PathBuf {
        self.root
            .join("disks")
            .join(format!("{id}.{}", format.extension()))
    }

    pub fn has_local(&self, id: VolumeId, format: VolumeFormat) -> bool {
        self.local_path(id, format).is_file()
    }

    pub fn put_record(&self, mut record: VolumeRecord) -> Result<VolumeRecord> {
        if record.backend != StorageBackend::Rbd {
            record.path = self.local_path(record.id, record.format);
        }
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn replace_records(&self, records: Vec<VolumeRecord>) -> Result<()> {
        let rewritten: Vec<VolumeRecord> = records
            .into_iter()
            .map(|mut record| {
                if record.backend != StorageBackend::Rbd {
                    record.path = self.local_path(record.id, record.format);
                }
                record
            })
            .collect();
        {
            let mut inner = self.inner.lock().expect("storage lock");
            inner.volumes.clear();
            for record in rewritten {
                inner.volumes.insert(record.id, record);
            }
        }
        self.flush()
    }

    pub fn ensure_local(&self, record: &VolumeRecord) -> Result<VolumeRecord> {
        if record.backend == StorageBackend::Rbd {
            return self.put_record(record.clone());
        }
        let path = self.local_path(record.id, record.format);
        if !path.exists() {
            match record.format {
                VolumeFormat::Raw => create_raw(&path, record.size_bytes)?,
                VolumeFormat::Qcow2 => {
                    if self.qemu.available() {
                        self.qemu.create_qcow2(&path, record.size_bytes)?;
                    } else {
                        create_raw(&path, record.size_bytes)?;
                    }
                }
            }
        }
        let mut stored = record.clone();
        stored.path = path;
        self.upsert_volume(stored.clone())?;
        Ok(stored)
    }

    pub fn read_blob(&self, id: VolumeId) -> Result<Vec<u8>> {
        let record = self.get_volume(id)?;
        Ok(std::fs::read(&record.path)?)
    }

    pub fn write_blob(&self, id: VolumeId, bytes: &[u8]) -> Result<VolumeRecord> {
        let mut record = self.get_volume(id)?;
        if record.backend == StorageBackend::Rbd {
            return Err(StorageError::Message(
                "cannot write a blob to an rbd volume".into(),
            ));
        }
        record.path = self.local_path(record.id, record.format);
        if let Some(parent) = record.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&record.path, bytes)?;
        record.size_bytes = record.size_bytes.max(bytes.len() as u64);
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn local_stat(&self, id: VolumeId) -> Result<(bool, u64)> {
        let record = self.get_volume(id)?;
        if !record.path.exists() {
            return Ok((false, 0));
        }
        Ok((true, std::fs::metadata(&record.path)?.len()))
    }

    pub fn list_volumes(&self) -> Result<Vec<VolumeRecord>> {
        let inner = self.inner.lock().expect("storage lock");
        Ok(inner.volumes.values().cloned().collect())
    }

    pub fn get_volume(&self, id: VolumeId) -> Result<VolumeRecord> {
        self.inner
            .lock()
            .expect("storage lock")
            .volumes
            .get(&id)
            .cloned()
            .ok_or(StorageError::NotFound(id))
    }

    pub fn create_volume(&self, req: CreateVolumeRequest) -> Result<VolumeRecord> {
        if req.name.trim().is_empty() {
            return Err(StorageError::Message("volume name is required".into()));
        }
        if req.size_bytes == 0 {
            return Err(StorageError::Message("volume size must be > 0".into()));
        }
        if req.format == VolumeFormat::Qcow2 && !self.qemu.available() {
            return Err(StorageError::QemuImgRequired);
        }
        {
            let inner = self.inner.lock().expect("storage lock");
            if inner.volumes.values().any(|vol| vol.name == req.name) {
                return Err(StorageError::NameTaken(req.name));
            }
        }
        let id = VolumeId::new();
        let path = self
            .root
            .join("disks")
            .join(format!("{id}.{}", req.format.extension()));
        match req.format {
            VolumeFormat::Raw => create_raw(&path, req.size_bytes)?,
            VolumeFormat::Qcow2 => self.qemu.create_qcow2(&path, req.size_bytes)?,
        }
        let record = VolumeRecord {
            id,
            name: req.name,
            format: req.format,
            size_bytes: req.size_bytes,
            path,
            backing_id: None,
            snapshots: vec![],
            replicas: vec![],
            replica_count: 1,
            backend: pertisk_types::StorageBackend::Replica,
        };
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn delete_volume(&self, id: VolumeId) -> Result<()> {
        let record = {
            let mut inner = self.inner.lock().expect("storage lock");
            inner
                .volumes
                .remove(&id)
                .ok_or(StorageError::NotFound(id))?
        };
        let _ = std::fs::remove_file(&record.path);
        let snap_dir = self.root.join("snapshots").join(id.to_string());
        let _ = std::fs::remove_dir_all(snap_dir);
        self.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: VolumeId, req: ResizeVolumeRequest) -> Result<VolumeRecord> {
        let mut record = self.get_volume(id)?;
        if req.size_bytes < record.size_bytes {
            return Err(StorageError::CannotShrink {
                from: record.size_bytes,
                to: req.size_bytes,
            });
        }
        if req.size_bytes == record.size_bytes {
            return Ok(record);
        }
        match record.format {
            VolumeFormat::Raw => {
                let file = OpenOptions::new().write(true).open(&record.path)?;
                file.set_len(req.size_bytes)?;
            }
            VolumeFormat::Qcow2 => self.qemu.resize(&record.path, req.size_bytes)?,
        }
        record.size_bytes = req.size_bytes;
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn clone_volume(&self, id: VolumeId, req: CloneVolumeRequest) -> Result<VolumeRecord> {
        if req.name.trim().is_empty() {
            return Err(StorageError::Message("clone name is required".into()));
        }
        let source = self.get_volume(id)?;
        {
            let inner = self.inner.lock().expect("storage lock");
            if inner.volumes.values().any(|vol| vol.name == req.name) {
                return Err(StorageError::NameTaken(req.name));
            }
        }
        let new_id = VolumeId::new();
        let format = if req.linked {
            VolumeFormat::Qcow2
        } else {
            source.format
        };
        let path = self
            .root
            .join("disks")
            .join(format!("{new_id}.{}", format.extension()));
        let backing_id = if req.linked {
            if !self.qemu.available() {
                return Err(StorageError::LinkedRequiresQemu);
            }
            self.qemu.linked_clone(&source.path, source.format, &path)?;
            Some(source.id)
        } else {
            std::fs::copy(&source.path, &path)?;
            None
        };
        let record = VolumeRecord {
            id: new_id,
            name: req.name,
            format,
            size_bytes: source.size_bytes,
            path,
            backing_id,
            snapshots: vec![],
            replicas: vec![],
            replica_count: source.replica_count.max(1),
            backend: source.backend,
        };
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn snapshot(&self, id: VolumeId, req: SnapshotRequest) -> Result<VolumeRecord> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(StorageError::Message("snapshot name is required".into()));
        }
        let mut record = self.get_volume(id)?;
        if record.snapshots.iter().any(|snap| snap.name == name) {
            return Err(StorageError::SnapshotExists(name.to_string()));
        }
        let created_unix = unix_now();
        let snap = if record.format == VolumeFormat::Qcow2 && self.qemu.available() {
            self.qemu.snapshot_create(&record.path, name)?;
            VolumeSnapshot {
                name: name.to_string(),
                created_unix,
                path: None,
            }
        } else {
            let dir = self.root.join("snapshots").join(id.to_string());
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(name);
            std::fs::copy(&record.path, &path)?;
            VolumeSnapshot {
                name: name.to_string(),
                created_unix,
                path: Some(path),
            }
        };
        record.snapshots.push(snap);
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn restore_snapshot(&self, id: VolumeId, name: &str) -> Result<VolumeRecord> {
        let record = self.get_volume(id)?;
        let snap = record
            .snapshots
            .iter()
            .find(|snap| snap.name == name)
            .cloned()
            .ok_or_else(|| StorageError::SnapshotNotFound(name.to_string()))?;
        if let Some(path) = snap.path {
            std::fs::copy(path, &record.path)?;
        } else {
            self.qemu.snapshot_apply(&record.path, name)?;
        }
        Ok(record)
    }

    pub fn list_isos(&self) -> Result<Vec<IsoRecord>> {
        let inner = self.inner.lock().expect("storage lock");
        Ok(inner.isos.values().cloned().collect())
    }

    pub fn get_iso(&self, name: &str) -> Result<IsoRecord> {
        self.inner
            .lock()
            .expect("storage lock")
            .isos
            .get(name)
            .cloned()
            .ok_or_else(|| StorageError::IsoNotFound(name.to_string()))
    }

    pub fn import_iso(&self, source: &Path, name: Option<String>) -> Result<IsoRecord> {
        let name = iso_name(source, name)?;
        {
            let inner = self.inner.lock().expect("storage lock");
            if inner.isos.contains_key(&name) {
                return Err(StorageError::IsoExists(name));
            }
        }
        if !source.is_file() {
            return Err(StorageError::Message(format!(
                "iso source is not a file: {}",
                source.display()
            )));
        }
        let dest = self.root.join("iso").join(&name);
        std::fs::copy(source, &dest)?;
        let size_bytes = dest.metadata()?.len();
        let record = IsoRecord {
            name: name.clone(),
            path: dest,
            size_bytes,
        };
        {
            let mut inner = self.inner.lock().expect("storage lock");
            inner.isos.insert(name, record.clone());
        }
        self.flush()?;
        Ok(record)
    }

    pub fn create_cloudinit_iso(&self, req: CloudInitIsoRequest) -> Result<IsoRecord> {
        let mut name = req.name.trim().to_string();
        if name.is_empty() {
            name = "cidata.iso".into();
        }
        if !name.to_ascii_lowercase().ends_with(".iso") {
            name.push_str(".iso");
        }
        if !name.to_ascii_lowercase().contains("cidata") {
            let stem = name.trim_end_matches(".iso").trim_end_matches(".ISO");
            name = format!("{stem}-cidata.iso");
        }
        let name = iso_name(Path::new(&name), Some(name.clone()))?;
        {
            let inner = self.inner.lock().expect("storage lock");
            if inner.isos.contains_key(&name) {
                return Err(StorageError::IsoExists(name));
            }
        }
        let user_data = cloudinit_user_data(&req);
        let hostname = req
            .hostname
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("pertisk");
        let meta_data = format!("instance-id: iid-{hostname}\nlocal-hostname: {hostname}\n");
        let bytes = cidata_iso(user_data.as_bytes(), meta_data.as_bytes());
        let dest = self.root.join("iso").join(&name);
        std::fs::write(&dest, &bytes)?;
        let record = IsoRecord {
            name: name.clone(),
            path: dest,
            size_bytes: bytes.len() as u64,
        };
        {
            let mut inner = self.inner.lock().expect("storage lock");
            inner.isos.insert(name, record.clone());
        }
        self.flush()?;
        Ok(record)
    }

    pub fn delete_iso(&self, name: &str) -> Result<()> {
        let record = {
            let mut inner = self.inner.lock().expect("storage lock");
            inner
                .isos
                .remove(name)
                .ok_or_else(|| StorageError::IsoNotFound(name.to_string()))?
        };
        let _ = std::fs::remove_file(&record.path);
        self.flush()?;
        Ok(())
    }

    fn upsert_volume(&self, record: VolumeRecord) -> Result<()> {
        {
            let mut inner = self.inner.lock().expect("storage lock");
            inner.volumes.insert(record.id, record);
        }
        self.flush()
    }

    fn flush(&self) -> Result<()> {
        let inner = self.inner.lock().expect("storage lock");
        let json = serde_json::to_vec_pretty(&*inner)?;
        let tmp = self.inventory_path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, &self.inventory_path)?;
        Ok(())
    }
}

fn create_raw(path: &Path, size: u64) -> Result<()> {
    let file = File::create(path)?;
    file.set_len(size)?;
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cloudinit_user_data(req: &CloudInitIsoRequest) -> String {
    if let Some(raw) = req
        .userdata
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return raw.to_string();
    }
    let user = req
        .user
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("ubuntu");
    let hostname = req
        .hostname
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("pertisk");
    let mut yaml = format!(
        "#cloud-config\nhostname: {hostname}\nmanage_etc_hosts: true\nusers:\n  - name: {user}\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    groups: sudo\n    shell: /bin/bash\n    lock_passwd: false\n"
    );
    let keys: Vec<&str> = req
        .ssh_authorized_keys
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .collect();
    if !keys.is_empty() {
        yaml.push_str("    ssh_authorized_keys:\n");
        for key in keys {
            yaml.push_str("      - ");
            yaml.push_str(key);
            yaml.push('\n');
        }
    }
    if let Some(password) = req
        .password
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        yaml.push_str("chpasswd:\n  expire: false\n  list: |\n    ");
        yaml.push_str(user);
        yaml.push(':');
        yaml.push_str(password);
        yaml.push_str("\nssh_pwauth: true\n");
    }
    yaml
}

fn iso_name(source: &Path, explicit: Option<String>) -> Result<String> {
    let name = match explicit {
        Some(name) => name,
        None => source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("disk.iso")
            .to_string(),
    };
    let name = Path::new(&name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() || name.starts_with('.') || name.contains('/') || name.contains('\\') {
        return Err(StorageError::InvalidIsoName(name));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::parse_size;

    fn pool() -> (VolumePool, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let pool = VolumePool::open(dir.path(), pertisk_types::find_in_path("qemu-img")).unwrap();
        (pool, dir)
    }

    #[test]
    fn raw_create_resize_clone_snapshot() {
        let (pool, _dir) = pool();
        let vol = pool
            .create_volume(CreateVolumeRequest {
                name: "disk".into(),
                size_bytes: parse_size("8M").unwrap(),
                format: VolumeFormat::Raw,
                replicas: None,
            })
            .unwrap();
        assert_eq!(vol.path.metadata().unwrap().len(), 8 * 1024 * 1024);

        let vol = pool
            .resize(
                vol.id,
                ResizeVolumeRequest {
                    size_bytes: parse_size("16M").unwrap(),
                },
            )
            .unwrap();
        assert_eq!(vol.size_bytes, 16 * 1024 * 1024);

        let clone = pool
            .clone_volume(
                vol.id,
                CloneVolumeRequest {
                    name: "disk-copy".into(),
                    linked: false,
                },
            )
            .unwrap();
        assert!(clone.backing_id.is_none());
        assert_eq!(clone.size_bytes, vol.size_bytes);

        let vol = pool
            .snapshot(
                vol.id,
                SnapshotRequest {
                    name: "before".into(),
                },
            )
            .unwrap();
        assert_eq!(vol.snapshots.len(), 1);
        std::fs::write(&vol.path, b"changed").unwrap();
        pool.restore_snapshot(vol.id, "before").unwrap();
        pool.delete_volume(clone.id).unwrap();
        pool.delete_volume(vol.id).unwrap();
        assert!(pool.list_volumes().unwrap().is_empty());
    }

    #[test]
    fn replica_path_is_local_and_sparse() {
        let (pool, _dir) = pool();
        let vol = pool
            .create_volume(CreateVolumeRequest {
                name: "disk".into(),
                size_bytes: parse_size("8M").unwrap(),
                format: VolumeFormat::Raw,
                replicas: None,
            })
            .unwrap();
        let ensured = pool.ensure_local(&vol).unwrap();
        assert_eq!(ensured.path, pool.local_path(vol.id, vol.format));
        assert!(pool.has_local(vol.id, vol.format));
        pool.replace_records(vec![vol.clone()]).unwrap();
        let got = pool.get_volume(vol.id).unwrap();
        assert_eq!(got.path, pool.local_path(vol.id, vol.format));
    }

    #[test]
    fn iso_import_and_delete() {
        let (pool, dir) = pool();
        let src = dir.path().join("installer.iso");
        std::fs::write(&src, b"iso-bytes").unwrap();
        let iso = pool.import_iso(&src, None).unwrap();
        assert_eq!(iso.name, "installer.iso");
        assert_eq!(iso.size_bytes, 9);
        pool.delete_iso("installer.iso").unwrap();
        assert!(pool.list_isos().unwrap().is_empty());
    }

    #[test]
    fn cloudinit_iso_is_cidata() {
        let (pool, _dir) = pool();
        let iso = pool
            .create_cloudinit_iso(CloudInitIsoRequest {
                name: "web-1".into(),
                hostname: Some("web-1".into()),
                user: Some("ubuntu".into()),
                password: Some("ubuntu".into()),
                ssh_authorized_keys: vec![],
                userdata: None,
            })
            .unwrap();
        assert_eq!(iso.name, "web-1-cidata.iso");
        let bytes = std::fs::read(&iso.path).unwrap();
        assert!(bytes.len() >= 25 * 2048);
        let vol = std::str::from_utf8(&bytes[32768 + 40..32768 + 46]).unwrap();
        assert_eq!(vol, "CIDATA");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("hostname: web-1"));
        assert!(text.contains("ubuntu:ubuntu"));
    }
}
