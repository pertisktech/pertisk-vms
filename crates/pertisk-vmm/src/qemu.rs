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

    fn qga_socket_path(&self, id: VmId) -> PathBuf {
        self.run_dir.join(format!("{id}.qga.sock"))
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
        let qga_socket = self.qga_socket_path(id);
        if let Some(parent) = serial_log.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Cap serial log growth so busy guests do not fill the appliance root.
        if serial_log.exists() {
            if let Ok(meta) = tokio::fs::metadata(&serial_log).await
                && meta.len() > 2 * 1024 * 1024
            {
                let _ = tokio::fs::write(&serial_log, []).await;
            }
        }
        for path in [&qmp, &serial_socket, &graphics_socket, &qga_socket] {
            if path.exists() {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        let mut cmd = qemu_command(&self.binary);
        cmd.arg("-name")
            .arg(format!("pertisk-{id}"))
            .arg("-machine")
            .arg(qemu_machine())
            .arg("-m")
            .arg(spec.memory_mib.to_string())
            .arg("-smp")
            .arg(spec.vcpus.to_string())
            .arg("-nodefaults")
            .arg("-display")
            .arg("none")
            .arg("-S")
            .arg("-qmp")
            .arg(format!("unix:{},server,nowait", qmp.display()))
            .arg("-serial")
            .arg(format!("unix:{},server,nowait", serial_socket.display()))
            .arg("-vnc")
            .arg(format!("unix:{},share=ignore", graphics_socket.display()))
            .arg("-chardev")
            .arg(format!(
                "socket,path={},server=on,wait=off,id=qga0",
                qga_socket.display()
            ));
        if host_is_aarch64() {
            cmd.arg("-device").arg("virtio-gpu-pci");
        } else {
            cmd.arg("-vga").arg("std");
        }
        cmd.arg("-device")
            .arg("virtio-serial-pci,id=virtio-serial0")
            .arg("-device")
            .arg("virtserialport,chardev=qga0,name=org.qemu.guest_agent.0");

        if pertisk_types::kvm_available() {
            cmd.arg("-enable-kvm").arg("-cpu").arg("host");
        } else if host_is_aarch64() {
            cmd.arg("-cpu").arg("max");
        }

        if spec.kernel.is_none() {
            if let Some((code, vars_template)) = find_uefi() {
                let vars = self.run_dir.join(format!("{id}.OVMF_VARS.fd"));
                // Always start from the template. A previous failed Talos/UKI boot stores
                // Boot0001=empty virtio + Boot0002=EFI shell in NVRAM, which then wins forever.
                tokio::fs::copy(&vars_template, &vars).await?;
                info!(vm = %id, firmware = %code.display(), "uefi");
                cmd.arg("-drive")
                    .arg(format!(
                        "if=pflash,format=raw,readonly=on,file={}",
                        code.display()
                    ))
                    .arg("-drive")
                    .arg(format!("if=pflash,format=raw,file={}", vars.display()));
            }
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
        let has_installer_cd = ordered.iter().any(|disk| disk.cdrom && !is_cidata(disk));
        let disk_bootable = ordered
            .iter()
            .any(|disk| !disk.cdrom && pertisk_types::disk_likely_bootable(&disk.path));
        if ordered.iter().any(|disk| disk.cdrom) && !host_is_aarch64() {
            cmd.arg("-device").arg("ich9-ahci,id=ahci");
        }
        let mut bootindex = 1u8;
        let mut cd_index = 0u8;
        for (index, disk) in ordered.iter().enumerate() {
            let format = drive_format(disk);
            let drive_id = format!("disk{index}");
            let boot = drive_bootindex(disk, disk_bootable, &mut bootindex);
            if disk.cdrom {
                cmd.arg("-drive").arg(format!(
                    "file={},if=none,id={drive_id},media=cdrom,readonly=on,format={format}",
                    disk.path.display()
                ));
                if host_is_aarch64() {
                    cmd.arg("-device")
                        .arg(format!("virtio-blk-pci,drive={drive_id}{boot}"));
                } else {
                    cmd.arg("-device")
                        .arg(format!("ide-cd,drive={drive_id},bus=ahci.{cd_index}{boot}"));
                    cd_index = cd_index.saturating_add(1);
                }
            } else {
                cmd.arg("-drive").arg(format!(
                    "file={},if=none,id={drive_id},format={format},cache=none,discard=unmap",
                    disk.path.display()
                ));
                cmd.arg("-device")
                    .arg(format!("virtio-blk-pci,drive={drive_id}{boot}"));
            }
        }
        if has_installer_cd {
            if disk_bootable {
                cmd.arg("-boot").arg("order=c");
            } else {
                cmd.arg("-boot").arg("order=d");
            }
        }

        for (i, net) in spec.nets.iter().enumerate() {
            if let Some(tap) = &net.tap {
                let id_net = format!("net{i}");
                cmd.arg("-netdev").arg(format!(
                    "tap,id={id_net},ifname={tap},script=no,downscript=no"
                ));
                let mut nic = format!("virtio-net-pci,netdev={id_net},romfile=");
                if let Some(mac) = &net.mac {
                    nic.push_str(&format!(",mac={mac}"));
                }
                cmd.arg("-device").arg(nic);
            }
        }

        info!(vm = %id, qmp = %qmp.display(), "starting qemu");
        let stderr_log = self.run_dir.join(format!("{id}.qemu.stderr"));
        let stderr = std::fs::File::create(&stderr_log).ok();
        let mut cmd = cmd;
        if let Some(file) = stderr {
            cmd.stderr(Stdio::from(file));
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .kill_on_drop(false)
            .spawn()
            .map_err(|err| {
                VmmError::Message(format!("qemu spawn: {err} (see {})", stderr_log.display()))
            })?;
        let pid = child.id();
        self.children.lock().await.insert(id, child);

        if let Err(err) = wait_qmp(&qmp, Duration::from_secs(8)).await {
            let tail = std::fs::read_to_string(&stderr_log)
                .ok()
                .map(|s| {
                    s.lines()
                        .rev()
                        .take(5)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|s| !s.is_empty());
            return Err(VmmError::Message(format!(
                "{err}{}",
                tail.map(|t| format!(": {t}")).unwrap_or_default()
            )));
        }

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

    /// True while the guest OS is running (QEMU alive and QMP status not shutdown).
    pub async fn is_running(&self, record: &VmRecord) -> bool {
        let Some(pid) = record.pid.filter(|pid| *pid > 0) else {
            return false;
        };
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return false;
        }
        let Some(qmp) = record.api_socket.as_ref() else {
            return true;
        };
        if !qmp.exists() {
            return false;
        }
        match qmp_query_status(qmp).await {
            Ok(status) => guest_qmp_running(&status),
            Err(_) => false,
        }
    }
}

fn guest_qmp_running(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "running" | "paused" | "prelaunch"
    )
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

/// Assign firmware bootindex. An empty disk must not outrank the installer CD
/// (Talos/metal EFI ISOs otherwise drop to the OVMF shell).
fn drive_bootindex(
    disk: &pertisk_types::DiskSpec,
    disk_bootable: bool,
    bootindex: &mut u8,
) -> String {
    if is_cidata(disk) {
        return String::new();
    }
    if disk.cdrom && disk_bootable {
        return String::new();
    }
    let idx = *bootindex;
    *bootindex = bootindex.saturating_add(1);
    format!(",bootindex={idx}")
}

fn host_is_aarch64() -> bool {
    cfg!(target_arch = "aarch64")
}

/// On big.LITTLE hosts (RK3588 A55+A76), pin QEMU to one cluster.
/// Mixed cores + `-cpu host` + smp>1 crash AAVMF with "Synchronous Exception".
fn qemu_command(binary: &Path) -> Command {
    if let Some(cpus) = qemu_taskset_cpus() {
        let taskset = ["/usr/bin/taskset", "/bin/taskset"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.exists());
        if let Some(taskset) = taskset {
            info!(%cpus, "pinning qemu to one CPU cluster");
            let mut cmd = Command::new(taskset);
            cmd.arg("-c").arg(cpus).arg(binary);
            return cmd;
        }
        warn!("heterogeneous CPUs but taskset is missing; qemu may fail UEFI");
    }
    Command::new(binary)
}

fn qemu_taskset_cpus() -> Option<String> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    preferred_homogeneous_cpuset(&cpuinfo)
}

fn preferred_homogeneous_cpuset(cpuinfo: &str) -> Option<String> {
    let groups = cpu_part_groups(cpuinfo);
    if groups.len() <= 1 {
        return None;
    }
    let cpus = groups
        .into_iter()
        .max_by(|left, right| left.1.len().cmp(&right.1.len()).then(left.0.cmp(&right.0)))?
        .1;
    if cpus.is_empty() {
        return None;
    }
    Some(compact_cpu_list(&cpus))
}

fn cpu_part_groups(cpuinfo: &str) -> Vec<(u32, Vec<u32>)> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut cpu: Option<u32> = None;
    for line in cpuinfo.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("processor") {
            cpu = rest.split(':').nth(1).and_then(|s| s.trim().parse().ok());
            continue;
        }
        let key = line.split(':').next().unwrap_or("").trim();
        if !key.eq_ignore_ascii_case("cpu part") {
            continue;
        }
        let part = line.split(':').nth(1).map(str::trim).and_then(|raw| {
            raw.strip_prefix("0x")
                .or_else(|| raw.strip_prefix("0X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| raw.parse().ok())
        });
        if let (Some(id), Some(part)) = (cpu, part) {
            groups.entry(part).or_default().push(id);
        }
    }
    groups.into_iter().collect()
}

fn compact_cpu_list(cpus: &[u32]) -> String {
    let mut cpus = cpus.to_vec();
    cpus.sort_unstable();
    cpus.dedup();
    let Some(&first) = cpus.first() else {
        return String::new();
    };
    let mut parts = Vec::new();
    let mut start = first;
    let mut prev = first;
    for &cpu in &cpus[1..] {
        if cpu == prev + 1 {
            prev = cpu;
            continue;
        }
        parts.push(format_cpu_range(start, prev));
        start = cpu;
        prev = cpu;
    }
    parts.push(format_cpu_range(start, prev));
    parts.join(",")
}

fn format_cpu_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

fn qemu_machine() -> &'static str {
    if host_is_aarch64() {
        "virt,gic-version=3,accel=kvm:tcg"
    } else {
        "q35,accel=kvm:tcg"
    }
}

fn find_uefi() -> Option<(PathBuf, PathBuf)> {
    let pairs: &[(&str, &str)] = if host_is_aarch64() {
        &[
            (
                "/usr/share/AAVMF/AAVMF_CODE.fd",
                "/usr/share/AAVMF/AAVMF_VARS.fd",
            ),
            (
                "/usr/share/qemu-efi-aarch64/QEMU_EFI.fd",
                "/usr/share/AAVMF/AAVMF_VARS.fd",
            ),
            (
                "/usr/share/edk2/aarch64/QEMU_EFI.fd",
                "/usr/share/edk2/aarch64/QEMU_VARS.fd",
            ),
        ]
    } else {
        &[
            (
                "/usr/share/OVMF/OVMF_CODE_4M.fd",
                "/usr/share/OVMF/OVMF_VARS_4M.fd",
            ),
            (
                "/usr/share/edk2/ovmf/OVMF_CODE_4M.fd",
                "/usr/share/edk2/ovmf/OVMF_VARS_4M.fd",
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
        ]
    };
    for (code, vars) in pairs {
        let code = PathBuf::from(code);
        let vars = PathBuf::from(vars);
        if code.is_file() && vars.is_file() {
            return Some((code, vars));
        }
    }
    None
}

async fn wait_qmp(path: &Path, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let mut last_err = String::new();
    while start.elapsed() < timeout {
        match qmp_execute(path, "qmp_capabilities").await {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = err.to_string();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    Err(VmmError::Message(format!(
        "timed out waiting for qmp {} ({last_err})",
        path.display()
    )))
}

async fn qmp_execute(path: &Path, execute: &str) -> Result<()> {
    qmp_query(path, execute).await.map(|_| ())
}

async fn qmp_query_status(path: &Path) -> Result<String> {
    let resp = qmp_query(path, "query-status").await?;
    resp.get("return")
        .and_then(|ret| ret.get("status"))
        .and_then(|status| status.as_str())
        .map(str::to_owned)
        .ok_or_else(|| VmmError::Message("query-status missing status".into()))
}

async fn qmp_query(path: &Path, execute: &str) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(path)
        .await
        .map_err(|err| VmmError::Message(format!("qmp connect {}: {err}", path.display())))?;
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
        let n = stream.read(&mut buf).await?;
        let text = std::str::from_utf8(&buf[..n]).unwrap_or("");
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                return Ok(value);
            }
        }
        return Err(VmmError::Message(format!("qmp {execute} returned no json")));
    }
    Ok(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::DiskSpec;

    #[test]
    fn guest_qmp_status_running() {
        assert!(guest_qmp_running("running"));
        assert!(guest_qmp_running("Running"));
        assert!(guest_qmp_running("paused"));
        assert!(!guest_qmp_running("shutdown"));
        assert!(!guest_qmp_running("guest-panicked"));
    }

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

    #[test]
    fn installer_cd_boots_before_empty_disk() {
        let iso = DiskSpec {
            path: PathBuf::from("/var/metal-amd64.iso"),
            readonly: true,
            cdrom: true,
            volume_id: None,
            iso_name: Some("metal-amd64.iso".into()),
        };
        let disk = DiskSpec {
            path: PathBuf::from("/var/empty.qcow2"),
            readonly: false,
            cdrom: false,
            volume_id: None,
            iso_name: None,
        };
        let mut idx = 1u8;
        assert_eq!(drive_bootindex(&iso, false, &mut idx), ",bootindex=1");
        assert_eq!(drive_bootindex(&disk, false, &mut idx), ",bootindex=2");
        let mut idx = 1u8;
        assert_eq!(drive_bootindex(&iso, true, &mut idx), "");
        assert_eq!(drive_bootindex(&disk, true, &mut idx), ",bootindex=1");
    }

    #[test]
    fn rk3588_pins_to_a76_cluster() {
        let cpuinfo = "\
processor\t: 0
CPU part\t: 0xd05
processor\t: 1
CPU part\t: 0xd05
processor\t: 2
CPU part\t: 0xd05
processor\t: 3
CPU part\t: 0xd05
processor\t: 4
CPU part\t: 0xd0b
processor\t: 5
CPU part\t: 0xd0b
processor\t: 6
CPU part\t: 0xd0b
processor\t: 7
CPU part\t: 0xd0b
";
        assert_eq!(
            preferred_homogeneous_cpuset(cpuinfo).as_deref(),
            Some("4-7")
        );
    }

    #[test]
    fn homogeneous_host_does_not_pin() {
        let cpuinfo = "\
processor\t: 0
CPU part\t: 0xd0b
processor\t: 1
CPU part\t: 0xd0b
";
        assert_eq!(preferred_homogeneous_cpuset(cpuinfo), None);
    }
}
