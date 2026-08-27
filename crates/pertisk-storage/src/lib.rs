//! Local directory volumes, file snapshots, ISO library, and optional qemu-img.

mod qemu;

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pertisk_types::{
    CloneVolumeRequest, CreateVolumeRequest, IsoRecord, ResizeVolumeRequest, SnapshotRequest,
    VolumeFormat, VolumeId, VolumeRecord, VolumeSnapshot,
};
use serde::{Deserialize, Serialize};

use crate::qemu::QemuImg;

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
        };
        self.upsert_volume(record.clone())?;
        Ok(record)
    }

    pub fn delete_volume(&self, id: VolumeId) -> Result<()> {
        let record = {
            let mut inner = self.inner.lock().expect("storage lock");
            inner.volumes.remove(&id).ok_or(StorageError::NotFound(id))?
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
            self.qemu
                .linked_clone(&source.path, source.format, &path)?;
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
}
