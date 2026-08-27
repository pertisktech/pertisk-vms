use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use pertisk_types::{VmRecord, VmSpec};
use serde::Serialize;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::unix_http::{expect_ok, put_json, wait_ready};
use crate::{CreateResult, Result, StartResult, VmmError};

#[derive(Debug)]
pub struct CloudHypervisorDriver {
    binary: PathBuf,
    run_dir: PathBuf,
    children: Mutex<HashMap<pertisk_types::VmId, Child>>,
}

impl CloudHypervisorDriver {
    pub fn new(binary: PathBuf, run_dir: PathBuf) -> Self {
        Self {
            binary,
            run_dir,
            children: Mutex::new(HashMap::new()),
        }
    }

    fn socket_path(&self, id: pertisk_types::VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.sock"))
    }

    fn serial_path(&self, id: pertisk_types::VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.serial"))
    }

    fn console_socket_path(&self, id: pertisk_types::VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.console.sock"))
    }

    pub async fn create(&self, id: pertisk_types::VmId, spec: &VmSpec) -> Result<CreateResult> {
        if spec.kernel.is_none() && spec.disks.is_empty() {
            return Err(VmmError::Message(
                "cloud-hypervisor needs a kernel or at least one disk".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.run_dir).await?;
        let socket = self.socket_path(id);
        if socket.exists() {
            let _ = tokio::fs::remove_file(&socket).await;
        }
        let serial_log = spec
            .serial_log
            .clone()
            .unwrap_or_else(|| self.serial_path(id));
        let console_socket = self.console_socket_path(id);
        if let Some(parent) = serial_log.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if console_socket.exists() {
            let _ = tokio::fs::remove_file(&console_socket).await;
        }

        info!(vm = %id, socket = %socket.display(), "starting cloud-hypervisor");
        let child = Command::new(&self.binary)
            .arg("--api-socket")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false)
            .spawn()?;
        let pid = child.id();
        self.children.lock().await.insert(id, child);

        wait_ready(&socket, Duration::from_secs(5)).await?;

        let config = ChVmConfig::from_spec(spec, &console_socket);
        let body = serde_json::to_vec(&config)?;
        let (status, resp) = put_json(&socket, "/api/v1/vm.create", Some(&body)).await?;
        expect_ok(status, &resp)?;

        Ok(CreateResult {
            api_socket: Some(socket),
            pid,
            serial_log: Some(serial_log),
            console_socket: Some(console_socket),
        })
    }

    pub async fn start(&self, record: &VmRecord) -> Result<StartResult> {
        let socket = record
            .api_socket
            .as_ref()
            .ok_or_else(|| VmmError::Message("missing api socket".into()))?;
        let (status, body) = put_json(socket, "/api/v1/vm.boot", None).await?;
        expect_ok(status, &body)?;
        Ok(StartResult { pid: record.pid })
    }

    pub async fn stop(&self, record: &VmRecord) -> Result<()> {
        if let Some(socket) = &record.api_socket
            && socket.exists()
        {
            match put_json(socket, "/api/v1/vm.shutdown", None).await {
                Ok((status, body)) => {
                    if let Err(err) = expect_ok(status, &body) {
                        warn!(vm = %record.id, %err, "vm.shutdown returned error");
                    }
                }
                Err(err) => warn!(vm = %record.id, %err, "vm.shutdown request failed"),
            }
        }
        self.reap_or_kill(record).await
    }

    pub async fn destroy(&self, record: &VmRecord) -> Result<()> {
        if let Some(socket) = &record.api_socket
            && socket.exists()
        {
            let _ = put_json(socket, "/api/v1/vm.delete", None).await;
            let _ = put_json(socket, "/api/v1/vmm.shutdown", None).await;
        }
        self.reap_or_kill(record).await?;
        if let Some(socket) = &record.api_socket {
            let _ = tokio::fs::remove_file(socket).await;
        }
        if let Some(socket) = &record.console_socket {
            let _ = tokio::fs::remove_file(socket).await;
        }
        Ok(())
    }

    async fn reap_or_kill(&self, record: &VmRecord) -> Result<()> {
        let mut children = self.children.lock().await;
        if let Some(mut child) = children.remove(&record.id) {
            match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => Err(err.into()),
                Err(_) => {
                    child.start_kill()?;
                    let _ = child.wait().await;
                    Ok(())
                }
            }
        } else if let Some(pid) = record.pid.filter(|pid| *pid > 0) {
            kill_pid(pid);
            Ok(())
        } else {
            Ok(())
        }
    }
}

fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}

#[derive(Serialize)]
struct ChVmConfig {
    cpus: ChCpus,
    memory: ChMemory,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<ChPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disks: Option<Vec<ChDisk>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    net: Option<Vec<ChNet>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    serial: Option<ChConsole>,
}

#[derive(Serialize)]
struct ChCpus {
    boot_vcpus: u8,
    max_vcpus: u8,
}

#[derive(Serialize)]
struct ChMemory {
    size: u64,
}

#[derive(Serialize)]
struct ChPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    kernel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cmdline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initramfs: Option<String>,
}

#[derive(Serialize)]
struct ChDisk {
    path: String,
    readonly: bool,
}

#[derive(Serialize)]
struct ChNet {
    #[serde(skip_serializing_if = "Option::is_none")]
    tap: Option<String>,
}

#[derive(Serialize)]
struct ChConsole {
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    socket: Option<String>,
}

impl ChVmConfig {
    fn from_spec(spec: &VmSpec, console_socket: &std::path::Path) -> Self {
        let payload = spec.kernel.as_ref().map(|kernel| ChPayload {
            kernel: Some(kernel.display().to_string()),
            cmdline: spec
                .cmdline
                .clone()
                .or_else(|| Some("console=ttyS0 reboot=k panic=1".into())),
            initramfs: spec
                .initramfs
                .as_ref()
                .map(|path| path.display().to_string()),
        });
        let disks = if spec.disks.is_empty() {
            None
        } else {
            Some(
                spec.disks
                    .iter()
                    .map(|disk| ChDisk {
                        path: disk.path.display().to_string(),
                        readonly: disk.readonly,
                    })
                    .collect(),
            )
        };
        let net = if spec.nets.is_empty() {
            None
        } else {
            Some(
                spec.nets
                    .iter()
                    .map(|net| ChNet {
                        tap: net.tap.clone(),
                    })
                    .collect(),
            )
        };
        Self {
            cpus: ChCpus {
                boot_vcpus: spec.vcpus,
                max_vcpus: spec.vcpus,
            },
            memory: ChMemory {
                size: u64::from(spec.memory_mib) * 1024 * 1024,
            },
            payload,
            disks,
            net,
            serial: Some(ChConsole {
                mode: "Socket",
                file: None,
                socket: Some(console_socket.display().to_string()),
            }),
        }
    }
}
