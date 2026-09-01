use std::collections::HashMap;
use std::sync::Mutex;

use pertisk_types::{VmId, VmRecord, VmSpec, VmState};

use crate::{CreateResult, Result, StartResult, VmmError};

#[derive(Debug)]
struct MockVm {
    state: VmState,
}

#[derive(Debug, Default)]
pub struct MockDriver {
    vms: Mutex<HashMap<VmId, MockVm>>,
}

impl MockDriver {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(&self, id: VmId, _spec: &VmSpec) -> Result<CreateResult> {
        let mut vms = self.vms.lock().expect("mock vmm lock");
        vms.insert(
            id,
            MockVm {
                state: VmState::Created,
            },
        );
        Ok(CreateResult {
            api_socket: None,
            pid: None,
            serial_log: None,
            console_socket: None,
            graphics_socket: None,
        })
    }

    pub async fn start(&self, record: &VmRecord) -> Result<StartResult> {
        let mut vms = self.vms.lock().expect("mock vmm lock");
        let vm = vms
            .get_mut(&record.id)
            .ok_or(VmmError::NotFound(record.id))?;
        match vm.state {
            VmState::Created | VmState::Stopped => {
                vm.state = VmState::Running;
                if let Some(path) = record
                    .serial_log
                    .as_ref()
                    .or(record.spec.serial_log.as_ref())
                {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .and_then(|mut file| {
                            use std::io::Write;
                            writeln!(file, "pertisk mock serial: vm {} started", record.id)
                        });
                }
                Ok(StartResult { pid: Some(0) })
            }
            state => Err(VmmError::InvalidState { state, op: "start" }),
        }
    }

    pub async fn stop(&self, record: &VmRecord) -> Result<()> {
        self.set_stopped(record, "stop").await
    }

    pub async fn shutdown(&self, record: &VmRecord) -> Result<()> {
        self.set_stopped(record, "shutdown").await
    }

    pub async fn restart(&self, record: &VmRecord) -> Result<()> {
        let mut vms = self.vms.lock().expect("mock vmm lock");
        let vm = vms
            .get_mut(&record.id)
            .ok_or(VmmError::NotFound(record.id))?;
        if vm.state != VmState::Running {
            return Err(VmmError::InvalidState {
                state: vm.state,
                op: "restart",
            });
        }
        Ok(())
    }

    async fn set_stopped(&self, record: &VmRecord, op: &'static str) -> Result<()> {
        let mut vms = self.vms.lock().expect("mock vmm lock");
        let vm = vms
            .get_mut(&record.id)
            .ok_or(VmmError::NotFound(record.id))?;
        if vm.state != VmState::Running {
            return Err(VmmError::InvalidState {
                state: vm.state,
                op,
            });
        }
        vm.state = VmState::Stopped;
        Ok(())
    }

    pub async fn destroy(&self, record: &VmRecord) -> Result<()> {
        let mut vms = self.vms.lock().expect("mock vmm lock");
        vms.remove(&record.id)
            .ok_or(VmmError::NotFound(record.id))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::VmSpec;

    fn spec() -> VmSpec {
        VmSpec {
            name: "demo".into(),
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
        }
    }

    fn record(id: VmId, spec: VmSpec) -> VmRecord {
        VmRecord {
            id,
            spec,
            state: VmState::Created,
            pid: None,
            api_socket: None,
            serial_log: None,
            console_socket: None,
            graphics_socket: None,
            last_error: None,
            node_id: None,
        }
    }

    #[tokio::test]
    async fn lifecycle() {
        let driver = MockDriver::new();
        let id = VmId::new();
        let spec = spec();
        driver.create(id, &spec).await.unwrap();
        let rec = record(id, spec);
        driver.start(&rec).await.unwrap();
        driver.stop(&rec).await.unwrap();
        driver.destroy(&rec).await.unwrap();
    }
}
