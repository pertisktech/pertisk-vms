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
        let vms = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            serde_json::from_str(&text)?
        } else {
            BTreeMap::new()
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

    pub fn name_taken(&self, name: &str, except: Option<VmId>) -> Result<bool, DaemonError> {
        let vms = self.vms.lock().expect("store lock");
        Ok(vms.values().any(|vm| {
            vm.spec.name == name && except.is_none_or(|id| vm.id != id)
        }))
    }

    pub fn upsert(&self, record: VmRecord) -> Result<(), DaemonError> {
        {
            let mut vms = self.vms.lock().expect("store lock");
            vms.insert(record.id, record);
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
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
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
                    disks: vec![],
                    nets: vec![],
                    serial_log: None,
                },
                state: VmState::Created,
                pid: None,
                api_socket: None,
                serial_log: None,
                last_error: None,
            })
            .unwrap();
        drop(store);
        let reopened = Store::open(dir.path().join("vms.json")).unwrap();
        assert_eq!(reopened.get(id).unwrap().spec.name, "a");
    }
}
