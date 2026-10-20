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
        self.spawn(id, spec, None).await
    }

    /// Paused QEMU waiting for `migrate` from the source. Does not continue the guest.
    pub async fn create_incoming(
        &self,
        id: VmId,
        spec: &VmSpec,
        port: u16,
    ) -> Result<CreateResult> {
        self.spawn(id, spec, Some(port)).await
    }

    /// Internal qcow2 snapshot of one drive while the guest keeps running.
    /// The guest agent is frozen first when it answers.
    pub async fn snapshot_internal(
        &self,
        record: &VmRecord,
        device: &str,
        name: &str,
    ) -> Result<()> {
        let qmp = qmp_socket(record)?;
        let qga = self.qga_socket_path(record.id);
        let frozen = tokio::task::spawn_blocking({
            let qga = qga.clone();
            move || crate::qga::fsfreeze(&qga, false).is_ok()
        })
        .await
        .unwrap_or(false);
        let result = qmp_ok(
            qmp,
            "blockdev-snapshot-internal-sync",
            Some(serde_json::json!({ "device": device, "name": name })),
        )
        .await
        .map(|_| ());
        if frozen {
            let _ = tokio::task::spawn_blocking(move || crate::qga::fsfreeze(&qga, true)).await;
        }
        result
    }

    /// Delete one internal qcow2 snapshot while the guest keeps running.
    pub async fn snapshot_delete_internal(
        &self,
        record: &VmRecord,
        device: &str,
        name: &str,
    ) -> Result<()> {
        let qmp = qmp_socket(record)?;
        qmp_ok(
            qmp,
            "blockdev-snapshot-delete-internal-sync",
            Some(serde_json::json!({ "device": device, "name": name })),
        )
        .await
        .map(|_| ())
    }

    pub async fn migrate_to(&self, record: &VmRecord, uri: &str) -> Result<()> {
        let qmp = record
            .api_socket
            .as_ref()
            .ok_or_else(|| VmmError::Message("missing qemu qmp socket".into()))?;
        qmp_execute_with(qmp, "migrate", Some(serde_json::json!({ "uri": uri })))
            .await
            .map(|_| ())
    }

    /// Export local (non-RBD, non-CD) drives so the source can `drive-mirror` into them.
    pub async fn nbd_serve(
        &self,
        record: &VmRecord,
        port: u16,
        exports: &[MirrorExport],
    ) -> Result<()> {
        if exports.is_empty() {
            return Ok(());
        }
        let qmp = qmp_socket(record)?;
        qmp_ok(
            qmp,
            "nbd-server-start",
            Some(serde_json::json!({
                "addr": {
                    "type": "inet",
                    "data": { "host": "0.0.0.0", "port": port.to_string() }
                }
            })),
        )
        .await?;
        for export in exports {
            qmp_ok(
                qmp,
                "nbd-server-add",
                Some(serde_json::json!({
                    "device": export.device,
                    "name": export.device,
                    "writable": true
                })),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn nbd_stop(&self, record: &VmRecord) -> Result<()> {
        let Some(qmp) = record.api_socket.as_deref() else {
            return Ok(());
        };
        if !qmp.exists() {
            return Ok(());
        }
        let _ = qmp_ok(qmp, "nbd-server-stop", None).await;
        Ok(())
    }

    pub async fn mirror_start(
        &self,
        record: &VmRecord,
        host: &str,
        port: u16,
        exports: &[MirrorExport],
    ) -> Result<()> {
        let qmp = qmp_socket(record)?;
        for export in exports {
            qmp_ok(
                qmp,
                "drive-mirror",
                Some(serde_json::json!({
                    "device": export.device,
                    "target": nbd_target(host, port, &export.device),
                    "sync": "full",
                    "mode": "existing",
                    "format": export.format
                })),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn wait_mirrors(
        &self,
        record: &VmRecord,
        exports: &[MirrorExport],
        timeout: Duration,
    ) -> Result<()> {
        if exports.is_empty() {
            return Ok(());
        }
        let qmp = qmp_socket(record)?;
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(VmmError::Message(format!(
                    "disk mirror timed out after {}s",
                    timeout.as_secs()
                )));
            }
            let resp = qmp_ok(qmp, "query-block-jobs", None).await?;
            match mirrors_ready(&resp, exports) {
                Ok(true) => return Ok(()),
                Ok(false) => tokio::time::sleep(Duration::from_millis(500)).await,
                Err(err) => return Err(err),
            }
        }
    }

    pub async fn complete_mirrors(
        &self,
        record: &VmRecord,
        exports: &[MirrorExport],
    ) -> Result<()> {
        let qmp = qmp_socket(record)?;
        for export in exports {
            qmp_ok(
                qmp,
                "block-job-complete",
                Some(serde_json::json!({ "device": export.device })),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn cancel_mirrors(&self, record: &VmRecord, exports: &[MirrorExport]) -> Result<()> {
        let Some(qmp) = record.api_socket.as_deref() else {
            return Ok(());
        };
        if !qmp.exists() {
            return Ok(());
        }
        for export in exports {
            let _ = qmp_ok(
                qmp,
                "block-job-cancel",
                Some(serde_json::json!({ "device": export.device })),
            )
            .await;
        }
        Ok(())
    }

    pub async fn wait_migrated(&self, record: &VmRecord, timeout: Duration) -> Result<()> {
        let qmp = record
            .api_socket
            .as_ref()
            .ok_or_else(|| VmmError::Message("missing qemu qmp socket".into()))?;
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(VmmError::Message(format!(
                    "live migration timed out after {}s",
                    timeout.as_secs()
                )));
            }
            let resp = qmp_execute_with(qmp, "query-migrate", None).await?;
            let status = migration_status_name(&resp).unwrap_or("unknown");
            if migration_succeeded(status) {
                return Ok(());
            }
            if migration_failed(status) {
                return Err(VmmError::Message(format!("live migration {status}")));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn spawn(&self, id: VmId, spec: &VmSpec, incoming: Option<u16>) -> Result<CreateResult> {
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
        // A previous start can leave qemu running with -S after wait_qmp
        // times out. Unlinking the socket and spawning again hits the qcow2
        // write lock. Reuse a live QMP, otherwise reap leftovers first.
        let leftover = qemu_pids_for(id);
        if incoming.is_none()
            && !leftover.is_empty()
            && wait_qmp(&qmp, Duration::from_secs(3)).await.is_ok()
        {
            let pid = leftover.into_iter().next();
            info!(vm = %id, pid = ?pid, "reusing qemu");
            return Ok(CreateResult {
                api_socket: Some(qmp),
                pid,
                serial_log: Some(serial_log),
                console_socket: Some(serial_socket),
                graphics_socket: Some(graphics_socket),
            });
        }
        if !leftover.is_empty() {
            warn!(vm = %id, pids = ?leftover, "stopping leftover qemu before start");
            let _ = self.children.lock().await.remove(&id);
            for pid in leftover {
                kill_pid(pid);
                wait_pid_gone(pid, Duration::from_secs(5)).await;
            }
        }
        for path in [&qmp, &serial_socket, &graphics_socket, &qga_socket] {
            if path.exists() {
                let _ = tokio::fs::remove_file(path).await;
            }
        }

        let mut cmd = qemu_command(&self.binary);
        cmd.arg("-name")
            .arg(qemu_guest_name(id, spec))
            .arg("-machine")
            .arg(qemu_machine())
            .arg("-m")
            .arg(spec.memory_mib.to_string())
            .arg("-smp")
            .arg(spec.vcpus.to_string())
            .arg("-nodefaults")
            .arg("-display")
            .arg("none")
            .arg("-S");
        if let Some(port) = incoming {
            cmd.arg("-incoming").arg(format!("tcp:0.0.0.0:{port}"));
        }
        cmd.arg("-qmp")
            .arg(format!("unix:{},server,nowait", qmp.display()))
            .arg("-serial")
            .arg(format!("unix:{},server,nowait", serial_socket.display()))
            .arg("-vnc")
            .arg(format!(
                "unix:{},share=force-shared",
                graphics_socket.display()
            ))
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
        // Absolute pointer so noVNC tracks. aarch64 virt has no PS/2 keyboard.
        for (flag, spec) in graphics_input_devices() {
            cmd.arg(flag).arg(spec);
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
        if has_installer_cd && !host_is_aarch64() {
            cmd.arg("-device").arg("ich9-ahci,id=ahci");
        }
        let mut bootindex = 1u8;
        let mut cd_index = 0u8;
        for (index, disk) in ordered.iter().enumerate() {
            let format = drive_format(disk);
            let drive_id = format!("disk{index}");
            let boot = drive_bootindex(disk, disk_bootable, &mut bootindex);
            if is_cidata(disk) {
                cmd.arg("-drive").arg(format!(
                    "file={},if=none,id={drive_id},readonly=on,format=raw",
                    disk.path.display()
                ));
                cmd.arg("-device")
                    .arg(format!("virtio-blk-pci,drive={drive_id},serial=cidata"));
            } else if disk.cdrom {
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
                let locking = if disk.path.to_string_lossy().starts_with("rbd:") {
                    ",file.locking=off"
                } else {
                    ""
                };
                let file = pertisk_types::guest_disk_path(&disk.path);
                cmd.arg("-drive").arg(format!(
                    "file={},if=none,id={drive_id},format={format},cache=writeback,aio=threads,detect-zeroes=off{locking}",
                    file.display()
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

        if let Err(err) = wait_qmp(&qmp, Duration::from_secs(30)).await {
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
            if let Some(mut child) = self.children.lock().await.remove(&id) {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            } else if let Some(pid) = pid {
                kill_pid(pid);
                wait_pid_gone(pid, Duration::from_secs(3)).await;
            }
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
                Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Ok(Err(err)) => Err(err.into()),
                Err(_) => {
                    if let Err(err) = child.start_kill()
                        && err.kind() != std::io::ErrorKind::NotFound
                    {
                        return Err(err.into());
                    }
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
        let pid = record
            .pid
            .filter(|pid| *pid > 0 && std::path::Path::new(&format!("/proc/{pid}")).exists())
            .or_else(|| qemu_pids_for(record.id).into_iter().next());
        let Some(pid) = pid else {
            return false;
        };
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return false;
        }
        let qmp = record
            .api_socket
            .clone()
            .unwrap_or_else(|| self.qmp_path(record.id));
        if !qmp.exists() {
            return false;
        }
        match qmp_query_status(&qmp).await {
            Ok(status) => guest_qmp_running(&status),
            Err(_) => false,
        }
    }
}

fn qemu_pids_for(id: VmId) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for ent in entries.flatten() {
        let Ok(pid) = ent.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmdline) = std::fs::read(ent.path().join("cmdline")) else {
            continue;
        };
        if cmdline_has_qemu_id(&cmdline, id) {
            pids.push(pid);
        }
    }
    pids
}

fn cmdline_has_qemu_id(cmdline: &[u8], id: VmId) -> bool {
    let needle = format!("process=pertisk-{id}");
    String::from_utf8_lossy(cmdline)
        .split('\0')
        .any(|part| part == needle || part.ends_with(&format!(",{needle}")))
}

fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}

async fn wait_pid_gone(pid: u32, timeout: Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn guest_qmp_running(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "running" | "paused" | "prelaunch"
    )
}

fn qemu_guest_name(id: VmId, spec: &VmSpec) -> String {
    let mut name: String = spec
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    while name.contains("--") {
        name = name.replace("--", "-");
    }
    let name = name.trim_matches('-').to_string();
    let name = if name.is_empty() {
        format!("vm-{id}")
    } else {
        name
    };
    format!("{name},process=pertisk-{id}")
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

/// USB tablet gives noVNC an absolute pointer. x86 keeps the machine PS/2 keyboard
/// so keystrokes are not delivered twice. aarch64 virt has no PS/2, so it gets a USB keyboard.
fn graphics_input_devices() -> Vec<(&'static str, &'static str)> {
    let mut devices = vec![
        ("-device", "qemu-xhci,id=xhci"),
        ("-device", "usb-tablet,bus=xhci.0"),
    ];
    if host_is_aarch64() {
        devices.push(("-device", "usb-kbd,bus=xhci.0"));
    }
    devices
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
    qmp_execute_with(path, execute, None).await.map(|_| ())
}

async fn qmp_execute_with(
    path: &Path,
    execute: &str,
    arguments: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    qmp_roundtrip(path, execute, arguments).await
}

/// Local disks the source must mirror. CD-ROMs and Ceph RBD images are skipped.
/// Device ids match the `disk{n}` drives QEMU is spawned with.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MirrorExport {
    pub device: String,
    pub format: String,
}

/// Drive id QEMU was given for this volume, after the same boot-order sort as spawn.
pub fn disk_device_id(spec: &VmSpec, volume_id: pertisk_types::VolumeId) -> Option<String> {
    let mut ordered: Vec<_> = spec.disks.iter().collect();
    ordered.sort_by_key(|disk| boot_rank(disk));
    ordered.iter().enumerate().find_map(|(index, disk)| {
        if disk.cdrom || disk.volume_id != Some(volume_id) {
            None
        } else {
            Some(format!("disk{index}"))
        }
    })
}

pub fn mirror_exports(spec: &VmSpec) -> Vec<MirrorExport> {
    let mut ordered: Vec<_> = spec.disks.iter().collect();
    ordered.sort_by_key(|disk| boot_rank(disk));
    ordered
        .iter()
        .enumerate()
        .filter_map(|(index, disk)| {
            if disk.cdrom || disk.path.to_string_lossy().starts_with("rbd:") {
                return None;
            }
            Some(MirrorExport {
                device: format!("disk{index}"),
                format: drive_format(disk).to_string(),
            })
        })
        .collect()
}

fn qmp_socket(record: &VmRecord) -> Result<&Path> {
    record
        .api_socket
        .as_deref()
        .ok_or_else(|| VmmError::Message("missing qemu qmp socket".into()))
}

fn nbd_target(host: &str, port: u16, device: &str) -> String {
    if host.contains(':') {
        format!("nbd:[{host}]:{port}:exportname={device}")
    } else {
        format!("nbd:{host}:{port}:exportname={device}")
    }
}

async fn qmp_ok(
    path: &Path,
    execute: &str,
    arguments: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let value = qmp_execute_with(path, execute, arguments).await?;
    if let Some(desc) = value
        .get("error")
        .and_then(|err| err.get("desc"))
        .and_then(|desc| desc.as_str())
    {
        return Err(VmmError::Message(format!("qmp {execute}: {desc}")));
    }
    if value.get("error").is_some() {
        return Err(VmmError::Message(format!("qmp {execute} failed")));
    }
    Ok(value)
}

fn mirrors_ready(value: &serde_json::Value, exports: &[MirrorExport]) -> Result<bool> {
    let jobs = value
        .get("return")
        .and_then(|ret| ret.as_array())
        .ok_or_else(|| VmmError::Message("query-block-jobs returned no list".into()))?;
    for export in exports {
        let Some(job) = jobs.iter().find(|job| {
            let device = job.get("device").and_then(|v| v.as_str());
            let id = job.get("id").and_then(|v| v.as_str());
            device == Some(export.device.as_str()) || id == Some(export.device.as_str())
        }) else {
            return Ok(false);
        };
        if let Some(err) = job.get("error").and_then(|v| v.as_str()) {
            return Err(VmmError::Message(format!(
                "disk mirror {} failed: {err}",
                export.device
            )));
        }
        if job.get("ready").and_then(|v| v.as_bool()) != Some(true) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn migration_status_name(value: &serde_json::Value) -> Option<&str> {
    value
        .get("return")
        .and_then(|ret| ret.get("status"))
        .and_then(|status| status.as_str())
}

pub(crate) fn migration_succeeded(status: &str) -> bool {
    status == "completed"
}

pub(crate) fn migration_failed(status: &str) -> bool {
    matches!(status, "failed" | "cancelled")
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
    qmp_roundtrip(path, execute, None).await
}

async fn qmp_roundtrip(
    path: &Path,
    execute: &str,
    arguments: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let mut stream = tokio::time::timeout(Duration::from_secs(3), UnixStream::connect(path))
        .await
        .map_err(|_| VmmError::Message(format!("qmp connect {}: timed out", path.display())))?
        .map_err(|err| VmmError::Message(format!("qmp connect {}: {err}", path.display())))?;
    let mut buf = vec![0u8; 65536];
    let n = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf))
        .await
        .map_err(|_| VmmError::Message(format!("qmp greeting {}: timed out", path.display())))?
        .map_err(|err| VmmError::Message(format!("qmp greeting {}: {err}", path.display())))?;
    let _ = n;
    let cap = serde_json::json!({"execute": "qmp_capabilities"});
    stream.write_all(cap.to_string().as_bytes()).await?;
    stream.write_all(b"\n").await?;
    let _ = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf))
        .await
        .map_err(|_| VmmError::Message(format!("qmp capabilities {}: timed out", path.display())))?
        .map_err(|err| VmmError::Message(format!("qmp capabilities {}: {err}", path.display())))?;
    if execute != "qmp_capabilities" {
        let cmd = match arguments {
            Some(arguments) => serde_json::json!({"execute": execute, "arguments": arguments}),
            None => serde_json::json!({"execute": execute}),
        };
        stream.write_all(cmd.to_string().as_bytes()).await?;
        stream.write_all(b"\n").await?;
        let n = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf))
            .await
            .map_err(|_| VmmError::Message(format!("qmp {execute} {}: timed out", path.display())))?
            .map_err(|err| VmmError::Message(format!("qmp {execute} {}: {err}", path.display())))?;
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
    use pertisk_types::{DiskSpec, VmSpec};

    #[test]
    fn graphics_input_has_an_absolute_tablet() {
        let devices = graphics_input_devices();
        assert!(devices.iter().any(|(_, spec)| spec.contains("usb-tablet")));
        assert!(devices.iter().any(|(_, spec)| spec.contains("qemu-xhci")));
        if cfg!(target_arch = "aarch64") {
            assert!(devices.iter().any(|(_, spec)| spec.contains("usb-kbd")));
        } else {
            assert!(!devices.iter().any(|(_, spec)| spec.contains("usb-kbd")));
        }
    }

    #[test]
    fn mirror_exports_skip_cdrom_and_rbd() {
        let spec: VmSpec = serde_json::from_value(serde_json::json!({
            "name": "guest",
            "disks": [
                {"path": "/iso/alpine.iso", "readonly": true, "cdrom": true, "iso_name": "alpine.iso"},
                {"path": "/disks/root.qcow2", "readonly": false, "cdrom": false},
                {"path": "rbd:vms/data", "readonly": false, "cdrom": false}
            ]
        }))
        .unwrap();
        let exports = mirror_exports(&spec);
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].device, "disk1");
        assert_eq!(exports[0].format, "qcow2");
        assert_eq!(
            nbd_target("10.1.1.2", 9000, "disk1"),
            "nbd:10.1.1.2:9000:exportname=disk1"
        );
        assert_eq!(
            nbd_target("fe80::1", 9000, "disk1"),
            "nbd:[fe80::1]:9000:exportname=disk1"
        );
    }

    #[test]
    fn disk_device_id_follows_boot_order() {
        let volume = pertisk_types::VolumeId::new();
        let spec = VmSpec {
            name: "guest".into(),
            vcpus: 1,
            memory_mib: 512,
            kernel: None,
            cmdline: None,
            initramfs: None,
            firmware: None,
            disks: vec![
                DiskSpec {
                    path: "/iso/alpine.iso".into(),
                    readonly: true,
                    cdrom: true,
                    volume_id: None,
                    iso_name: Some("alpine.iso".into()),
                },
                DiskSpec {
                    path: "/disks/root.qcow2".into(),
                    readonly: false,
                    cdrom: false,
                    volume_id: Some(volume),
                    iso_name: None,
                },
            ],
            nets: vec![],
            serial_log: None,
            console_type: Default::default(),
            ha: false,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
            ssh_user: None,
            group: None,
            backup: None,
            snapshot: None,
        };
        assert_eq!(disk_device_id(&spec, volume).as_deref(), Some("disk1"));
        assert_eq!(disk_device_id(&spec, pertisk_types::VolumeId::new()), None);
    }

    #[test]
    fn migration_status_parses() {
        let completed = serde_json::json!({"return": {"status": "completed"}});
        assert_eq!(migration_status_name(&completed), Some("completed"));
        assert!(migration_succeeded("completed"));
        assert!(migration_failed("failed"));
        assert!(!migration_failed("active"));
    }

    #[test]
    fn guest_qmp_status_running() {
        assert!(guest_qmp_running("running"));
        assert!(guest_qmp_running("Running"));
        assert!(guest_qmp_running("paused"));
        assert!(!guest_qmp_running("shutdown"));
        assert!(!guest_qmp_running("guest-panicked"));
    }

    #[test]
    fn qemu_cmdline_matches_exact_vm_id() {
        let cmd = b"qemu-system-x86_64\0-name\0rocky-1,process=pertisk-104\0-S\0";
        assert!(cmdline_has_qemu_id(cmd, VmId::Numeric(104)));
        assert!(!cmdline_has_qemu_id(cmd, VmId::Numeric(10)));
        assert!(!cmdline_has_qemu_id(cmd, VmId::Numeric(1040)));
        assert!(!cmdline_has_qemu_id(cmd, VmId::Numeric(105)));
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
    fn qemu_name_uses_guest_name() {
        let spec = VmSpec {
            name: "AlmaLinux 10-1".into(),
            vcpus: 1,
            memory_mib: 512,
            kernel: None,
            cmdline: None,
            initramfs: None,
            firmware: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
            console_type: pertisk_types::ConsoleType::Serial,
            ha: true,
            autostart: false,
            autostart_delay: 0,
            autostart_order: 0,
            ssh_user: None,
            group: None,
            backup: None,
            snapshot: None,
        };
        assert_eq!(
            qemu_guest_name(pertisk_types::VmId::Numeric(101), &spec),
            "AlmaLinux-10-1,process=pertisk-101"
        );
    }

    #[test]
    fn cidata_seed_is_not_bootable() {
        let seed = DiskSpec {
            path: PathBuf::from("/var/web-cidata.iso"),
            readonly: true,
            cdrom: true,
            volume_id: None,
            iso_name: Some("web-cidata.iso".into()),
        };
        let mut idx = 1u8;
        assert_eq!(drive_bootindex(&seed, true, &mut idx), "");
        assert_eq!(boot_rank(&seed), 2);
        assert!(is_cidata(&seed));
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
