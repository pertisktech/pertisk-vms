use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

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

    /// Insert a new guest, allocating a numeric ID when `requested` is `None`.
    /// ID and name uniqueness are checked under the same lock so parallel
    /// clones cannot both pick the next free ID and overwrite each other.
    pub fn insert_unique(
        &self,
        requested: Option<VmId>,
        name: &str,
        build: impl FnOnce(VmId) -> VmRecord,
    ) -> Result<VmRecord, DaemonError> {
        let record = {
            let mut vms = self.vms.lock().expect("store lock");
            if vms.values().any(|vm| vm.spec.name == name) {
                return Err(DaemonError::NameTaken(name.to_string()));
            }
            let id = match requested {
                Some(id) if vms.contains_key(&id) => return Err(DaemonError::IdTaken(id)),
                Some(id) => id,
                None => next_numeric_locked(&vms),
            };
            let record = build(id);
            vms.insert(id, record.clone());
            record
        };
        self.flush()?;
        Ok(record)
    }

    /// Patch a live record under the lock so a stale `list()` cannot wipe disks/ISO.
    pub fn update<F>(&self, id: VmId, f: F) -> Result<VmRecord, DaemonError>
    where
        F: FnOnce(&mut VmRecord),
    {
        let record = {
            let mut vms = self.vms.lock().expect("store lock");
            let rec = vms.get_mut(&id).ok_or(DaemonError::NotFound(id))?;
            f(rec);
            rec.clone()
        };
        self.flush()?;
        Ok(record)
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

fn next_numeric_locked(vms: &BTreeMap<VmId, VmRecord>) -> VmId {
    let used: HashSet<u64> = vms
        .keys()
        .filter_map(|id| match id {
            VmId::Numeric(n) => Some(*n),
            VmId::Legacy(_) => None,
        })
        .collect();
    let mut n = 100u64;
    while used.contains(&n) {
        n += 1;
        if n > 9_999_999_999 {
            return VmId::new();
        }
    }
    VmId::Numeric(n)
}

fn atomic_write(path: &Path, json: &[u8]) -> Result<(), DaemonError> {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let tmp = path.with_extension(format!("json.tmp.{}", SEQ.fetch_add(1, Ordering::Relaxed)));
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
                    ssh_user: None,
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

    #[test]
    fn concurrent_removes_do_not_fail_on_tmp_rename() {
        let (store, _dir) = tmp_store();
        let ids: Vec<_> = (0..20)
            .map(|i| {
                let id = VmId::Numeric(200 + i);
                store
                    .upsert(VmRecord {
                        id,
                        spec: VmSpec {
                            name: format!("vm-{i}"),
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
                            ssh_user: None,
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
                id
            })
            .collect();
        std::thread::scope(|s| {
            for id in ids {
                let store = &store;
                s.spawn(move || store.remove(id).unwrap());
            }
        });
        assert!(store.list().unwrap().is_empty());
    }

    fn rec(id: VmId, name: &str) -> VmRecord {
        VmRecord {
            id,
            spec: VmSpec {
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
                ssh_user: None,
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
        }
    }

    #[test]
    fn insert_unique_allocates_distinct_numeric_ids() {
        let (store, _dir) = tmp_store();
        let a = store
            .insert_unique(None, "a", |id| rec(id, "a"))
            .unwrap();
        let b = store
            .insert_unique(None, "b", |id| rec(id, "b"))
            .unwrap();
        assert_eq!(a.id, VmId::Numeric(100));
        assert_eq!(b.id, VmId::Numeric(101));
        assert!(matches!(
            store.insert_unique(Some(a.id), "c", |id| rec(id, "c")),
            Err(DaemonError::IdTaken(_))
        ));
        assert!(matches!(
            store.insert_unique(None, "a", |id| rec(id, "a")),
            Err(DaemonError::NameTaken(_))
        ));
    }

    #[test]
    fn parallel_insert_unique_does_not_reuse_ids() {
        let (store, _dir) = tmp_store();
        let store = std::sync::Arc::new(store);
        std::thread::scope(|s| {
            let mut joins = Vec::new();
            for i in 0..24 {
                let store = store.clone();
                joins.push(s.spawn(move || {
                    let name = format!("vm-{i}");
                    store.insert_unique(None, &name, |id| rec(id, &name))
                }));
            }
            let mut ids = HashSet::new();
            for j in joins {
                let rec = j.join().unwrap().unwrap();
                assert!(ids.insert(rec.id), "duplicate id {}", rec.id);
            }
            assert_eq!(ids.len(), 24);
        });
    }
}
