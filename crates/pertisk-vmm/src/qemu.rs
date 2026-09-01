//! QEMU/KVM guests with serial (unix) and VNC (unix) at the same time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use pertisk_types::{VmId, VmRecord, VmSpec};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::{CreateResult, Result, StartResult, VmmError};

#[derive(Debug)]
pub struct QemuDriver {
    binary: PathBuf,
    run_dir: PathBuf,
    children: Mutex<HashMap<VmId, Child>>,
}

impl QemuDriver {
    pub fn new(binary: PathBuf, run_dir: PathBuf) -> Self {
        Self {
            binary,
            run_dir,
            children: Mutex::new(HashMap::new()),
        }
    }

    fn qmp_path(&self, id: VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.qmp.sock"))
    }

    fn serial_path(&self, id: VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.serial"))
    }

    fn serial_socket_path(&self, id: VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.serial.sock"))
    }

    fn graphics_socket_path(&self, id: VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.graphics.sock"))
    }

    pub async fn create(&self, id: VmId, spec: &VmSpec) -> Result<CreateResult> {
        if spec.disks.is_empty() && spec.kernel.is_none() {
            return Err(VmmError::Message(
                "qemu needs a kernel or at least one disk".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.run_dir).await?;
        let qmp = self.qmp_path(id);
        let serial_log = spec
            .serial_log
            .clone()
            .unwrap_or_else(|| self.serial_path(id));
        let serial_socket = self.serial_socket_path(id);
        let graphics_socket = self.graphics_socket_path(id);
        if let Some(parent) = serial_log.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        for path in [&qmp, &serial_socket, &graphics_socket] {
            if path.exists() {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        let mut cmd = Command::new(&self.binary);
        cmd.arg("-name")
            .arg(format!("pertisk-{id}"))
            .arg("-machine")
            .arg("q35,accel=kvm:tcg")
            .arg("-m")
            .arg(spec.memory_mib.to_string())
            .arg("-smp")
            .arg(spec.vcpus.to_string())
            .arg("-nodefaults")
            .arg("-display")
            .arg("none")
            .arg("-vga")
            .arg("std")
            .arg("-S")
            .arg("-qmp")
            .arg(format!("unix:{},server,nowait", qmp.display()))
            .arg("-serial")
            .arg(format!("unix:{},server,nowait", serial_socket.display()))
            .arg("-vnc")
            .arg(format!("unix:{},share=ignore", graphics_socket.display()));

        if pertisk_types::kvm_available() {
            cmd.arg("-enable-kvm").arg("-cpu").arg("host");
        }

        if let Some((code, vars_template)) = find_ovmf() {
            let vars = self.run_dir.join(format!("{id}.OVMF_VARS.fd"));
            tokio::fs::copy(&vars_template, &vars).await?;
            cmd.arg("-drive")
                .arg(format!(
                    "if=pflash,format=raw,readonly=on,file={}",
                    code.display()
                ))
                .arg("-drive")
                .arg(format!("if=pflash,format=raw,file={}", vars.display()));
        }

        if let Some(kernel) = &spec.kernel {
            cmd.arg("-kernel").arg(kernel);
            if let Some(initramfs) = &spec.initramfs {
                cmd.arg("-initrd").arg(initramfs);
            }
            if let Some(cmdline) = &spec.cmdline {
                cmd.arg("-append").arg(cmdline);
            }
        }

        let mut ordered: Vec<_> = spec.disks.iter().collect();
        ordered.sort_by_key(|disk| boot_rank(disk));
        let has_installer_cd = ordered
            .iter()
            .any(|disk| disk.cdrom && !is_cidata(disk));
        for (index, disk) in ordered.iter().enumerate() {
            let format = drive_format(disk);
            if disk.cdrom {
                cmd.arg("-drive").arg(format!(
                    "file={},if=ide,index={index},media=cdrom,readonly=on,format={format}",
                    disk.path.display()
                ));
            } else {
                cmd.arg("-drive").arg(format!(
                    "file={},if=virtio,index={index},format={format},discard=unmap",
                    disk.path.display()
                ));
            }
        }
        if has_installer_cd {
            cmd.arg("-boot").arg("order=dc");
        }

        for (i, net) in spec.nets.iter().enumerate() {
            if let Some(tap) = &net.tap {
                let id_net = format!("net{i}");
                cmd.arg("-netdev").arg(format!(
                    "tap,id={id_net},ifname={tap},script=no,downscript=no"
                ));
                let mut nic = format!("virtio-net-pci,netdev={id_net}");
                if let Some(mac) = &net.mac {
                    nic.push_str(&format!(",mac={mac}"));
                }
                cmd.arg("-device").arg(nic);
            }
        }

        info!(vm = %id, qmp = %qmp.display(), "starting qemu");
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false)
            .spawn()?;
        let pid = child.id();
        self.children.lock().await.insert(id, child);

        wait_socket(&qmp, Duration::from_secs(8)).await?;
        qmp_execute(&qmp, "qmp_capabilities").await?;

        Ok(CreateResult {
            api_socket: Some(qmp),
            pid,
            serial_log: Some(serial_log),
            console_socket: Some(serial_socket),
            graphics_socket: Some(graphics_socket),
        })
    }

    pub async fn start(&self, record: &VmRecord) -> Result<StartResult> {
        let qmp = record
            .api_socket
            .as_ref()
            .ok_or_else(|| VmmError::Message("missing qemu qmp socket".into()))?;
        qmp_execute(qmp, "cont").await?;
        Ok(StartResult { pid: record.pid })
    }

    pub async fn stop(&self, record: &VmRecord) -> Result<()> {
        if let Some(qmp) = &record.api_socket
            && qmp.exists()
        {
            if let Err(err) = qmp_execute(qmp, "quit").await {
                warn!(vm = %record.id, %err, "qemu quit failed");
            }
        }
        self.reap_or_kill(record).await
    }

    /// ACPI shutdown; waits for the guest to power off, then force-kills on timeout.
    pub async fn shutdown(&self, record: &VmRecord) -> Result<()> {
        if let Some(qmp) = &record.api_socket
            && qmp.exists()
        {
            if let Err(err) = qmp_execute(qmp, "system_powerdown").await {
                warn!(vm = %record.id, %err, "qemu system_powerdown failed");
            }
        }
        self.wait_or_kill(record, Duration::from_secs(120)).await
    }

    /// Hard reset while the guest keeps running.
    pub async fn restart(&self, record: &VmRecord) -> Result<()> {
        let qmp = record
            .api_socket
            .as_ref()
            .ok_or_else(|| VmmError::Message("missing qemu qmp socket".into()))?;
        qmp_execute(qmp, "system_reset").await
    }

    pub async fn destroy(&self, record: &VmRecord) -> Result<()> {
        let _ = self.stop(record).await;
        for path in [
            record.api_socket.as_ref(),
            record.console_socket.as_ref(),
            record.graphics_socket.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = tokio::fs::remove_file(path).await;
        }
        let vars = self.run_dir.join(format!("{}.OVMF_VARS.fd", record.id));
        let _ = tokio::fs::remove_file(vars).await;
        Ok(())
    }

    async fn wait_or_kill(&self, record: &VmRecord, timeout: Duration) -> Result<()> {
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
            let _ = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status();
            Ok(())
        } else {
            Ok(())
        }
    }

    async fn reap_or_kill(&self, record: &VmRecord) -> Result<()> {
        self.wait_or_kill(record, Duration::from_secs(3)).await
    }
}

fn drive_format(disk: &pertisk_types::DiskSpec) -> &'static str {
    let name = disk
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".qcow2") {
        "qcow2"
    } else {
        "raw"
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

fn find_ovmf() -> Option<(PathBuf, PathBuf)> {
    const PAIRS: &[(&str, &str)] = &[
        (
            "/usr/share/OVMF/OVMF_CODE_4M.fd",
            "/usr/share/OVMF/OVMF_VARS_4M.fd",
        ),
        (
            "/usr/share/OVMF/OVMF_CODE.fd",
            "/usr/share/OVMF/OVMF_VARS.fd",
        ),
        (
            "/usr/share/edk2/ovmf/OVMF_CODE.fd",
            "/usr/share/edk2/ovmf/OVMF_VARS.fd",
        ),
        (
            "/usr/share/pve-edk2-firmware/OVMF_CODE.fd",
            "/usr/share/pve-edk2-firmware/OVMF_VARS.fd",
        ),
    ];
    for (code, vars) in PAIRS {
        let code = PathBuf::from(code);
        let vars = PathBuf::from(vars);
        if code.is_file() && vars.is_file() {
            return Some((code, vars));
        }
    }
    None
}

async fn wait_socket(path: &Path, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(VmmError::Message(format!(
        "timed out waiting for qemu socket {}",
        path.display()
    )))
}

async fn qmp_execute(path: &Path, execute: &str) -> Result<()> {
    let mut stream = UnixStream::connect(path).await.map_err(|err| {
        VmmError::Message(format!("qmp connect {}: {err}", path.display()))
    })?;
    let mut buf = vec![0u8; 65536];
    let _ = stream.read(&mut buf).await?;
    let cap = serde_json::json!({"execute": "qmp_capabilities"});
    stream.write_all(cap.to_string().as_bytes()).await?;
    stream.write_all(b"\n").await?;
    let _ = stream.read(&mut buf).await?;
    if execute != "qmp_capabilities" {
        let cmd = serde_json::json!({"execute": execute});
        stream.write_all(cmd.to_string().as_bytes()).await?;
        stream.write_all(b"\n").await?;
        let _ = stream.read(&mut buf).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::DiskSpec;

    #[test]
    fn qcow2_and_iso_formats() {
        let disk = DiskSpec {
            path: PathBuf::from("/var/disk.qcow2"),
            readonly: false,
            cdrom: false,
            volume_id: None,
            iso_name: None,
        };
        assert_eq!(drive_format(&disk), "qcow2");
        let iso = DiskSpec {
            path: PathBuf::from("/var/os.iso"),
            readonly: true,
            cdrom: true,
            volume_id: None,
            iso_name: Some("os.iso".into()),
        };
        assert_eq!(drive_format(&iso), "raw");
        assert_eq!(boot_rank(&iso), 0);
        assert_eq!(boot_rank(&disk), 1);
    }
}
