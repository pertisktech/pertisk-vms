use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use pertisk_types::{VmRecord, VmSpec};
use serde::Serialize;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::unix_http::{expect_ok, get, put_json, wait_ready};
use crate::{CreateResult, Result, StartResult, VmmError};

#[derive(Debug)]
pub struct CloudHypervisorDriver {
    binary: PathBuf,
    run_dir: PathBuf,
    firmware: Option<PathBuf>,
    children: Mutex<HashMap<pertisk_types::VmId, Child>>,
}

impl CloudHypervisorDriver {
    pub fn new(binary: PathBuf, run_dir: PathBuf, firmware: Option<PathBuf>) -> Self {
        Self {
            binary,
            run_dir,
            firmware,
            children: Mutex::new(HashMap::new()),
        }
    }

    fn socket_path(&self, id: pertisk_types::VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.sock"))
    }

    fn serial_path(&self, id: pertisk_types::VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.serial"))
    }

    fn serial_socket_path(&self, id: pertisk_types::VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.serial.sock"))
    }

    pub async fn create(&self, id: pertisk_types::VmId, spec: &VmSpec) -> Result<CreateResult> {
        if spec.kernel.is_none() && spec.disks.is_empty() {
            return Err(VmmError::Message(
                "cloud-hypervisor needs a kernel or at least one disk".into(),
            ));
        }
        if spec.kernel.is_none() && spec.firmware.is_none() && self.firmware.is_none() {
            return Err(VmmError::Message(
                "disk/ISO boot needs firmware (set vmm.firmware or install hypervisor-fw)".into(),
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
        let serial_socket = self.serial_socket_path(id);
        if let Some(parent) = serial_log.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if serial_socket.exists() {
            let _ = tokio::fs::remove_file(&serial_socket).await;
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

        let config = ChVmConfig::from_spec(spec, &serial_socket, self.firmware.as_deref());
        let body = serde_json::to_vec(&config)?;
        let (status, resp) = put_json(&socket, "/api/v1/vm.create", Some(&body)).await?;
        expect_ok(status, &resp)?;

        Ok(CreateResult {
            api_socket: Some(socket),
            pid,
            serial_log: Some(serial_log),
            console_socket: Some(serial_socket),
            graphics_socket: None,
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
        self.shutdown_wait(record, Duration::from_secs(3)).await
    }

    pub async fn shutdown(&self, record: &VmRecord) -> Result<()> {
        self.shutdown_wait(record, Duration::from_secs(120)).await
    }

    pub async fn restart(&self, record: &VmRecord) -> Result<()> {
        let _ = record;
        Err(VmmError::Message(
            "cloud-hypervisor restart is handled by stop+start".into(),
        ))
    }

    async fn shutdown_wait(&self, record: &VmRecord, timeout: Duration) -> Result<()> {
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
        self.reap_or_kill(record, timeout).await
    }

    pub async fn destroy(&self, record: &VmRecord) -> Result<()> {
        if let Some(socket) = &record.api_socket
            && socket.exists()
        {
            let _ = put_json(socket, "/api/v1/vm.delete", None).await;
            let _ = put_json(socket, "/api/v1/vmm.shutdown", None).await;
        }
        self.reap_or_kill(record, Duration::from_secs(3)).await?;
        if let Some(socket) = &record.api_socket {
            let _ = tokio::fs::remove_file(socket).await;
        }
        if let Some(socket) = &record.console_socket {
            let _ = tokio::fs::remove_file(socket).await;
        }
        if let Some(socket) = &record.graphics_socket {
            let _ = tokio::fs::remove_file(socket).await;
        }
        Ok(())
    }

    async fn reap_or_kill(&self, record: &VmRecord, timeout: Duration) -> Result<()> {
        let mut children = self.children.lock().await;
        if let Some(mut child) = children.remove(&record.id) {
            match tokio::time::timeout(timeout, child.wait()).await {
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

    /// True while cloud-hypervisor reports the guest as running.
    pub async fn is_running(&self, record: &VmRecord) -> bool {
        let Some(socket) = record.api_socket.as_ref() else {
            return false;
        };
        if !socket.exists() {
            return false;
        }
        let Ok((status, body)) = get(socket, "/api/v1/vm.info").await else {
            return false;
        };
        if !(200..300).contains(&status) {
            return false;
        }
        let Ok(info) = serde_json::from_slice::<serde_json::Value>(&body) else {
            return false;
        };
        info.get("state")
            .and_then(|state| state.as_str())
            .is_some_and(|state| state.eq_ignore_ascii_case("running"))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    console: Option<ChConsole>,
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
    firmware: Option<String>,
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
    fn from_spec(
        spec: &VmSpec,
        serial_socket: &std::path::Path,
        host_firmware: Option<&std::path::Path>,
    ) -> Self {
        let firmware = spec
            .firmware
            .as_ref()
            .map(|p| p.as_path())
            .or(host_firmware)
            .map(|p| p.display().to_string());
        let payload = if spec.kernel.is_some() {
            Some(ChPayload {
                firmware: None,
                kernel: spec.kernel.as_ref().map(|p| p.display().to_string()),
                cmdline: spec
                    .cmdline
                    .clone()
                    .or_else(|| Some("console=ttyS0 reboot=k panic=1".into())),
                initramfs: spec
                    .initramfs
                    .as_ref()
                    .map(|path| path.display().to_string()),
            })
        } else if firmware.is_some() {
            Some(ChPayload {
                firmware,
                kernel: None,
                cmdline: spec.cmdline.clone(),
                initramfs: None,
            })
        } else {
            None
        };
        let mut ordered: Vec<&pertisk_types::DiskSpec> = spec.disks.iter().collect();
        ordered.sort_by_key(|disk| boot_rank(disk));
        let disks: Vec<ChDisk> = ordered
            .into_iter()
            .map(|disk| ChDisk {
                path: disk.path.display().to_string(),
                readonly: disk.readonly || disk.cdrom,
            })
            .collect();
        let disks = if disks.is_empty() { None } else { Some(disks) };
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

        // Configure serial and graphics consoles based on console_type
        let serial = Some(ChConsole {
            mode: "Socket",
            file: None,
            socket: Some(serial_socket.display().to_string()),
        });
        // Cloud Hypervisor has no VGA; use the QEMU driver for graphical consoles.
        let console = None;

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
            serial,
            console,
        }
    }
}

fn is_cidata(disk: &pertisk_types::DiskSpec) -> bool {
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

fn boot_rank(disk: &pertisk_types::DiskSpec) -> u8 {
    if disk.cdrom && !is_cidata(disk) {
        0
    } else if !disk.cdrom {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::{DiskSpec, VmSpec};
    use std::path::PathBuf;

    fn spec() -> VmSpec {
        VmSpec {
            name: "iso".into(),
            vcpus: 1,
            memory_mib: 512,
            kernel: None,
            cmdline: None,
            initramfs: None,
            firmware: None,
            disks: vec![
                DiskSpec {
                    path: PathBuf::from("/var/disk.raw"),
                    readonly: false,
                    cdrom: false,
                    volume_id: None,
                    iso_name: None,
                },
                DiskSpec {
                    path: PathBuf::from("/var/os.iso"),
                    readonly: true,
                    cdrom: true,
                    volume_id: None,
                    iso_name: Some("os.iso".into()),
                },
                DiskSpec {
                    path: PathBuf::from("/var/seed-cidata.iso"),
                    readonly: true,
                    cdrom: true,
                    volume_id: None,
                    iso_name: Some("web-cidata.iso".into()),
                },
            ],
            nets: vec![],
            serial_log: None,
            console_type: Default::default(),
            ha: true,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
        }
    }

    #[test]
    fn disk_boot_uses_host_firmware_and_iso_first() {
        let cfg = ChVmConfig::from_spec(
            &spec(),
            PathBuf::from("/tmp/c.sock").as_path(),
            Some(PathBuf::from("/usr/lib/hypervisor-fw").as_path()),
        );
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["payload"]["firmware"], "/usr/lib/hypervisor-fw");
        assert!(json["payload"].get("kernel").is_none());
        let disks = json["disks"].as_array().unwrap();
        assert_eq!(disks[0]["path"], "/var/os.iso");
        assert_eq!(disks[0]["readonly"], true);
        assert_eq!(disks[1]["path"], "/var/disk.raw");
        assert_eq!(disks[2]["path"], "/var/seed-cidata.iso");
    }
}
