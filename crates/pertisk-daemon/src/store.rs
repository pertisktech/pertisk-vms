use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pertisk_types::{VmId, VmRecord};

use crate::DaemonError;

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    vms: Mutex<BTreeMap<VmId, VmRecord>>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DaemonError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let vms = match load_json_map(&path)? {
            Some(map) => map,
            None => BTreeMap::new(),
        };
        Ok(Self {
            path,
            vms: Mutex::new(vms),
        })
    }

    pub fn list(&self) -> Result<Vec<VmRecord>, DaemonError> {
        let vms = self.vms.lock().expect("store lock");
        Ok(vms.values().cloned().collect())
    }

    pub fn get(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        self.vms
            .lock()
            .expect("store lock")
            .get(&id)
            .cloned()
            .ok_or(DaemonError::NotFound(id))
    }

    pub fn contains(&self, id: VmId) -> bool {
        self.vms.lock().expect("store lock").contains_key(&id)
    }

    pub fn name_taken(&self, name: &str, except: Option<VmId>) -> Result<bool, DaemonError> {
        let vms = self.vms.lock().expect("store lock");
        Ok(vms
            .values()
            .any(|vm| vm.spec.name == name && except.is_none_or(|id| vm.id != id)))
    }

    pub fn upsert(&self, record: VmRecord) -> Result<(), DaemonError> {
        {
            let mut vms = self.vms.lock().expect("store lock");
            vms.insert(record.id, record);
        }
        self.flush()
    }

    pub fn replace_all(&self, records: Vec<VmRecord>) -> Result<(), DaemonError> {
        {
            let mut vms = self.vms.lock().expect("store lock");
            vms.clear();
            for record in records {
                vms.insert(record.id, record);
            }
        }
        self.flush()
    }

    pub fn remove(&self, id: VmId) -> Result<VmRecord, DaemonError> {
        let record = {
            let mut vms = self.vms.lock().expect("store lock");
            vms.remove(&id).ok_or(DaemonError::NotFound(id))?
        };
        self.flush()?;
        Ok(record)
    }

    fn flush(&self) -> Result<(), DaemonError> {
        let vms = self.vms.lock().expect("store lock");
        let json = serde_json::to_vec_pretty(&*vms)?;
        atomic_write(&self.path, &json)
    }
}

/// Read a JSON object map, or `None` when the file is missing/empty/corrupt
/// (e.g. all-zero after a hard power cut mid-write).
fn load_json_map(path: &Path) -> Result<Option<BTreeMap<VmId, VmRecord>>, DaemonError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    if !json_bytes_look_valid(&bytes) {
        quarantine_corrupt(path, &bytes)?;
        return Ok(None);
    }
    match serde_json::from_slice(&bytes) {
        Ok(map) => Ok(Some(map)),
        Err(err) => {
            quarantine_corrupt(path, &bytes)?;
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "vms.json corrupt; starting with empty inventory"
            );
            Ok(None)
        }
    }
}

fn json_bytes_look_valid(bytes: &[u8]) -> bool {
    let trimmed = trim_ascii(bytes);
    !trimmed.is_empty() && (trimmed[0] == b'{' || trimmed[0] == b'[')
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn quarantine_corrupt(path: &Path, bytes: &[u8]) -> Result<(), DaemonError> {
    let backup = path.with_extension("json.corrupt");
    let _ = std::fs::write(&backup, bytes);
    tracing::warn!(
        path = %path.display(),
        backup = %backup.display(),
        "quarantined corrupt state file"
    );
    Ok(())
}

fn atomic_write(path: &Path, json: &[u8]) -> Result<(), DaemonError> {
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::File::open(parent).and_then(|f| f.sync_all());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::{VmSpec, VmState};

    fn tmp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vms.json")).unwrap();
        (store, dir)
    }

    #[test]
    fn persists_roundtrip() {
        let (store, dir) = tmp_store();
        let id = VmId::new();
        store
            .upsert(VmRecord {
                id,
                spec: VmSpec {
                    name: "a".into(),
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
                },
                state: VmState::Created,
                pid: None,
                api_socket: None,
                serial_log: None,
                console_socket: None,
                graphics_socket: None,
                last_error: None,
                node_id: None,
                template: false,
            })
            .unwrap();
        drop(store);
        let reopened = Store::open(dir.path().join("vms.json")).unwrap();
        assert_eq!(reopened.get(id).unwrap().spec.name, "a");
    }

    #[test]
    fn opens_empty_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vms.json");
        std::fs::write(&path, "\n").unwrap();
        assert!(Store::open(path).unwrap().list().unwrap().is_empty());
    }

    #[test]
    fn opens_zeroed_corrupt_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vms.json");
        std::fs::write(&path, vec![0u8; 128]).unwrap();
        assert!(Store::open(&path).unwrap().list().unwrap().is_empty());
        assert!(path.with_extension("json.corrupt").exists());
    }
}
