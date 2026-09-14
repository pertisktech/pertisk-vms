//! Kernel-boot cloud disks when Cloud Hypervisor firmware cannot load Secure
//! Boot shim (`BOOTX64.EFI` → `import_mok_state` / FileError).
//!
//! RHEL/Rocky/Alma ship BLS under `/boot/loader/entries`. Ubuntu ships GRUB
//! (`/boot/vmlinuz` + `initrd.img`) and often only `EFI/BOOT/BOOTX64.EFI` as shim.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::StorageError;
use crate::iso_boot::LinuxIsoBoot;

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
        .or_else(|| loopdev.find_labeled("cloudimg-rootfs"))
        .or_else(|| loopdev.find_fs("xfs"))
        .or_else(|| loopdev.find_fs("ext4"))
        .or_else(|| loopdev.find_fs("btrfs"))
        .ok_or_else(|| {
            StorageError::Message(format!(
                "{} uses UEFI Secure Boot (shim) and Cloud Hypervisor firmware cannot boot it; \
                 no /boot or root filesystem was found to kernel-boot instead",
                disk.display()
            ))
        })?;

    let boot_mnt = TempMount::mount(&boot_part, true)?;
    let entry = pick_bls_entry(boot_mnt.path())
        .or_else(|| pick_unix_kernel(boot_mnt.path()))
        .ok_or_else(|| {
            StorageError::Message(format!(
                "{} has shim but no kernel under /boot (BLS or vmlinuz/initrd)",
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
    if !cmdline.contains("root=") {
        if let Some(uuid) = blkid_value(&boot_part, "UUID") {
            cmdline = format!("root=UUID={uuid} ro {cmdline}");
        }
    }
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
            if efi_file_is_shim(&file.path()) {
                return true;
            }
        }
    }
    false
}

fn efi_file_is_shim(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.starts_with("shim") && name.ends_with(".efi") {
        return true;
    }
    if name == "mmx64.efi" || name == "mmaa64.efi" || name == "mokmanager.efi" {
        return true;
    }
    if name == "bootx64.efi" || name == "bootaa64.efi" || name == "bootia32.efi" {
        return file_contains(path, b"MokList") || file_contains(path, b"shim");
    }
    false
}

fn file_contains(path: &Path, needle: &[u8]) -> bool {
    let Ok(data) = fs::read(path) else {
        return false;
    };
    data.windows(needle.len()).any(|w| w == needle)
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
            let dir = boot.join("loader/entries");
            fs::read_dir(dir)
                .ok()?
                .flatten()
                .filter_map(|f| parse_bls(&f.path()))
                .next()
        })
}

/// Ubuntu/Debian cloud images: `/boot/vmlinuz` + `initrd.img` (no BLS).
fn pick_unix_kernel(root: &Path) -> Option<BlsEntry> {
    let dirs = [root.join("boot"), root.to_path_buf()];
    for dir in &dirs {
        if let Some(entry) = kernel_in_dir(dir, root) {
            return Some(entry);
        }
    }
    None
}

fn kernel_in_dir(dir: &Path, root: &Path) -> Option<BlsEntry> {
    let linux_path =
        existing_file(dir, &["vmlinuz"]).or_else(|| newest_prefixed(dir, "vmlinuz-"))?;
    let linux_name = linux_path.file_name()?.to_string_lossy();
    let suffix = linux_name.strip_prefix("vmlinuz").unwrap_or("");
    let initrd_path = existing_file(dir, &["initrd.img", "initrd.img.old"])
        .or_else(|| {
            if suffix.is_empty() {
                None
            } else {
                let named = dir.join(format!("initrd.img{suffix}"));
                named.is_file().then_some(named)
            }
        })
        .or_else(|| {
            if suffix.is_empty() {
                None
            } else {
                let named = dir.join(format!("initramfs{suffix}.img"));
                named.is_file().then_some(named)
            }
        })
        .or_else(|| newest_prefixed(dir, "initrd.img-"))
        .or_else(|| newest_prefixed(dir, "initramfs-"))?;
    let linux = path_under(root, &linux_path)?;
    let initrd = path_under(root, &initrd_path)?;
    let options = grub_linux_options(root).unwrap_or_default();
    Some(BlsEntry {
        version: suffix.trim_start_matches('-').to_string(),
        linux,
        initrd,
        options,
    })
}

fn existing_file(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

fn newest_prefixed(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(prefix))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.pop()
}

fn path_under(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn grub_linux_options(root: &Path) -> Option<String> {
    for rel in ["boot/grub/grub.cfg", "grub/grub.cfg", "boot/grub2/grub.cfg"] {
        let text = fs::read_to_string(root.join(rel)).ok()?;
        for line in text.lines().rev() {
            let line = line.trim();
            let rest = line
                .strip_prefix("linux ")
                .or_else(|| line.strip_prefix("linux\t"))
                .or_else(|| line.strip_prefix("linuxefi "));
            let Some(rest) = rest else {
                continue;
            };
            let options = rest
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            if !options.is_empty() {
                return Some(options);
            }
        }
    }
    None
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
            return Err(StorageError::Message(
                "losetup returned empty device".into(),
            ));
        }
        // Partition nodes can lag briefly after -P.
        for _ in 0..20 {
            if Path::new(&format!("{device}p1")).exists()
                || Path::new(&format!("{device}p2")).exists()
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
        let name = self
            .device
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
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
        let path =
            std::env::temp_dir().join(format!("pertisk-mnt-{}-{}", std::process::id(), now_ms()));
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
    if value.is_empty() { None } else { Some(value) }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootx64_with_moklist_is_shim() {
        let dir = tempfile::tempdir().unwrap();
        let efi = dir.path().join("EFI/BOOT");
        fs::create_dir_all(&efi).unwrap();
        fs::write(efi.join("BOOTX64.EFI"), b"PE\0\0MokListRT shim signature").unwrap();
        assert!(esp_has_shim(dir.path()));
    }

    #[test]
    fn plain_grub_bootx64_is_not_shim() {
        let dir = tempfile::tempdir().unwrap();
        let efi = dir.path().join("EFI/BOOT");
        fs::create_dir_all(&efi).unwrap();
        fs::write(efi.join("BOOTX64.EFI"), b"GRUB bootloader only").unwrap();
        assert!(!esp_has_shim(dir.path()));
    }

    #[test]
    fn ubuntu_vmlinuz_without_bls() {
        let dir = tempfile::tempdir().unwrap();
        let boot = dir.path().join("boot");
        fs::create_dir_all(&boot).unwrap();
        fs::write(boot.join("vmlinuz-6.8.0-generic"), b"kernel").unwrap();
        fs::write(boot.join("initrd.img-6.8.0-generic"), b"initrd").unwrap();
        fs::create_dir_all(boot.join("grub")).unwrap();
        fs::write(
            boot.join("grub/grub.cfg"),
            "linux /boot/vmlinuz-6.8.0-generic root=UUID=abc ro quiet\n",
        )
        .unwrap();
        let entry = pick_unix_kernel(dir.path()).expect("ubuntu kernel");
        assert!(
            entry.linux.contains("vmlinuz-6.8.0-generic"),
            "{}",
            entry.linux
        );
        assert!(
            entry.initrd.contains("initrd.img-6.8.0-generic"),
            "{}",
            entry.initrd
        );
        assert!(entry.options.contains("root=UUID=abc"), "{}", entry.options);
    }
}
