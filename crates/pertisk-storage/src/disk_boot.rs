//! Kernel-boot RHEL/Rocky/Alma cloud disks when Cloud Hypervisor firmware cannot
//! load Secure Boot shim (`BOOTX64.EFI` → FileError / import_mok_state).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::iso_boot::LinuxIsoBoot;
use crate::StorageError;

/// If the disk ESP has shim, extract BLS kernel/initrd for direct boot. `Ok(None)` = firmware OK.
pub fn prepare_shim_disk_boot(
    disk: &Path,
    dest_dir: &Path,
) -> Result<Option<LinuxIsoBoot>, StorageError> {
    if !disk.is_file() {
        return Ok(None);
    }
    let loopdev = LoopDisk::attach(disk)?;
    let Some(esp) = loopdev.find_fs("vfat") else {
        return Ok(None);
    };
    let esp_mnt = TempMount::mount(&esp, true)?;
    if !esp_has_shim(esp_mnt.path()) {
        return Ok(None);
    }

    let boot_part = loopdev
        .find_labeled("BOOT")
        .or_else(|| loopdev.find_fs("xfs"))
        .or_else(|| loopdev.find_fs("ext4"))
        .ok_or_else(|| {
            StorageError::Message(format!(
                "{} uses UEFI Secure Boot (shim) and Cloud Hypervisor firmware cannot boot it; \
                 no separate /boot filesystem was found to kernel-boot instead",
                disk.display()
            ))
        })?;

    let boot_mnt = TempMount::mount(&boot_part, true)?;
    let entry = pick_bls_entry(boot_mnt.path()).ok_or_else(|| {
        StorageError::Message(format!(
            "{} has shim but no Boot Loader Spec entry under /boot/loader/entries",
            disk.display()
        ))
    })?;

    fs::create_dir_all(dest_dir)?;
    let kernel_src = resolve_boot_path(boot_mnt.path(), &entry.linux)?;
    let initrd_src = resolve_boot_path(boot_mnt.path(), &entry.initrd)?;
    let kernel = dest_dir.join("vmlinuz");
    let initramfs = dest_dir.join("initramfs");
    fs::copy(&kernel_src, &kernel)?;
    fs::copy(&initrd_src, &initramfs)?;

    let mut cmdline = entry.options;
    if !cmdline.contains("console=") {
        cmdline.push_str(" console=ttyS0,115200n8");
    }
    // Cloud Hypervisor exits cleanly on guest reboot when reboot=k is set.
    if !cmdline.contains("reboot=") {
        cmdline.push_str(" reboot=k");
    }
    if !cmdline.contains("panic=") {
        cmdline.push_str(" panic=1");
    }

    Ok(Some(LinuxIsoBoot {
        kernel,
        initramfs,
        cmdline,
    }))
}

fn esp_has_shim(esp: &Path) -> bool {
    let efi = esp.join("EFI");
    let Ok(vendors) = fs::read_dir(&efi) else {
        return false;
    };
    for vendor in vendors.flatten() {
        let path = vendor.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(files) = fs::read_dir(&path) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("shim") && name.ends_with(".efi") {
                return true;
            }
        }
    }
    false
}

#[derive(Debug)]
struct BlsEntry {
    linux: String,
    initrd: String,
    options: String,
    version: String,
}

fn pick_bls_entry(boot: &Path) -> Option<BlsEntry> {
    let dir = boot.join("loader/entries");
    let mut entries = Vec::new();
    for file in fs::read_dir(dir).ok()?.flatten() {
        let path = file.path();
        if path.extension().and_then(|e| e.to_str()) != Some("conf") {
            continue;
        }
        if let Some(entry) = parse_bls(&path) {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| b.version.cmp(&a.version));
    entries
        .into_iter()
        .find(|e| !e.version.contains("rescue"))
        .or_else(|| {
            // fall back to any entry
            let dir = boot.join("loader/entries");
            fs::read_dir(dir)
                .ok()?
                .flatten()
                .filter_map(|f| parse_bls(&f.path()))
                .next()
        })
}

fn parse_bls(path: &Path) -> Option<BlsEntry> {
    let text = fs::read_to_string(path).ok()?;
    let mut linux = None;
    let mut initrd = None;
    let mut options = String::new();
    let mut version = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("linux ") {
            linux = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("initrd ") {
            initrd = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("options ") {
            options = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("version ") {
            version = rest.trim().to_string();
        }
    }
    Some(BlsEntry {
        linux: linux?,
        initrd: initrd?,
        options,
        version,
    })
}

fn resolve_boot_path(boot: &Path, rel: &str) -> Result<PathBuf, StorageError> {
    let rel = rel.trim().trim_start_matches('/');
    let candidates = [
        boot.join(rel),
        boot.join(rel.strip_prefix("boot/").unwrap_or(rel)),
    ];
    for path in candidates {
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(StorageError::Message(format!(
        "boot file not found: {rel} under {}",
        boot.display()
    )))
}

struct LoopDisk {
    device: PathBuf,
}

impl LoopDisk {
    fn attach(disk: &Path) -> Result<Self, StorageError> {
        let out = Command::new("losetup")
            .args(["-f", "--show", "-P"])
            .arg(disk)
            .output()
            .map_err(|err| StorageError::Message(format!("losetup: {err}")))?;
        if !out.status.success() {
            return Err(StorageError::Message(format!(
                "losetup failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let device = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if device.is_empty() {
            return Err(StorageError::Message("losetup returned empty device".into()));
        }
        // Partition nodes can lag briefly after -P.
        for _ in 0..20 {
            if Path::new(&format!("{device}p1")).exists() || Path::new(&format!("{device}p2")).exists()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(Self {
            device: PathBuf::from(device),
        })
    }

    fn parts(&self) -> Vec<PathBuf> {
        let name = self.device.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let mut parts = Vec::new();
        for i in 1..=8 {
            let p = self.device.with_file_name(format!("{name}p{i}"));
            if p.exists() {
                parts.push(p);
            }
        }
        parts
    }

    fn find_fs(&self, want: &str) -> Option<PathBuf> {
        for part in self.parts() {
            if blkid_type(&part).as_deref() == Some(want) {
                return Some(part);
            }
        }
        None
    }

    fn find_labeled(&self, label: &str) -> Option<PathBuf> {
        for part in self.parts() {
            if blkid_label(&part).as_deref() == Some(label) {
                return Some(part);
            }
        }
        None
    }
}

impl Drop for LoopDisk {
    fn drop(&mut self) {
        let _ = Command::new("losetup").arg("-d").arg(&self.device).status();
    }
}

struct TempMount {
    path: PathBuf,
}

impl TempMount {
    fn mount(dev: &Path, read_only: bool) -> Result<Self, StorageError> {
        let path = std::env::temp_dir().join(format!(
            "pertisk-mnt-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&path)?;
        let mut cmd = Command::new("mount");
        if read_only {
            cmd.args(["-o", "ro"]);
        }
        let out = cmd
            .arg(dev)
            .arg(&path)
            .output()
            .map_err(|err| StorageError::Message(format!("mount: {err}")))?;
        if !out.status.success() {
            let _ = fs::remove_dir(&path);
            return Err(StorageError::Message(format!(
                "mount {} failed: {}",
                dev.display(),
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempMount {
    fn drop(&mut self) {
        let _ = Command::new("umount").arg(&self.path).status();
        let _ = fs::remove_dir(&self.path);
    }
}

fn blkid_type(dev: &Path) -> Option<String> {
    blkid_value(dev, "TYPE")
}

fn blkid_label(dev: &Path) -> Option<String> {
    blkid_value(dev, "LABEL")
}

fn blkid_value(dev: &Path, key: &str) -> Option<String> {
    let out = Command::new("blkid")
        .args(["-o", "value", "-s", key])
        .arg(dev)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
