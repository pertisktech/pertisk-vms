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

    let mut search = Vec::new();
    if let Some(p) = loopdev.find_labeled("BOOT") {
        search.push(p);
    }
    if let Some(p) = loopdev.find_labeled("cloudimg-rootfs") {
        if !search.contains(&p) {
            search.push(p);
        }
    }
    for fs in ["xfs", "ext4", "btrfs"] {
        if let Some(p) = loopdev.find_fs(fs) {
            if !search.contains(&p) {
                search.push(p);
            }
        }
    }
    if search.is_empty() {
        return Err(StorageError::Message(format!(
            "{} uses UEFI Secure Boot (shim) and Cloud Hypervisor firmware cannot boot it; \
             no /boot or root filesystem was found to kernel-boot instead",
            disk.display()
        )));
    }

    fs::create_dir_all(dest_dir)?;
    let kernel = dest_dir.join("vmlinuz");
    let initramfs = dest_dir.join("initramfs");
    let mut entry: Option<BlsEntry> = None;
    for part in &search {
        let boot_mnt = TempMount::mount(part, true)?;
        let Some(found) =
            pick_bls_entry(boot_mnt.path()).or_else(|| pick_unix_kernel(boot_mnt.path()))
        else {
            continue;
        };
        let kernel_src = resolve_boot_path(boot_mnt.path(), &found.linux)?;
        let initrd_src = resolve_boot_path(boot_mnt.path(), &found.initrd)?;
        fs::copy(&kernel_src, &kernel)?;
        fs::copy(&initrd_src, &initramfs)?;
        entry = Some(found);
        break;
    }
    let entry = entry.ok_or_else(|| {
        StorageError::Message(format!(
            "{} has shim but no kernel under /boot (BLS or vmlinuz/initrd)",
            disk.display()
        ))
    })?;

    // Ubuntu 24.10+ puts vmlinuz on LABEL=BOOT (p13) and the rootfs on
    // LABEL=cloudimg-rootfs (p1). Pin root= to the rootfs, not /boot.
    let root_part = loopdev
        .find_labeled("cloudimg-rootfs")
        .or_else(|| loopdev.find_labeled("writable"))
        .unwrap_or_else(|| search[0].clone());

    let mut cmdline = entry.options;
    if cmdline
        .split_whitespace()
        .any(|p| p.starts_with("root=LABEL="))
        || !cmdline_has_block_root(&cmdline)
    {
        cmdline = pin_root_device(&cmdline, &root_part);
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

/// Grub's `root=LABEL=cloudimg-rootfs` needs udev/initramfs. Direct CH kernel-boot
/// must name the virtio partition or the kernel panics with unknown-block(0,0).
fn cmdline_has_block_root(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|p| {
        p.starts_with("root=/dev/")
            || p.starts_with("root=UUID=")
            || p.starts_with("root=PARTUUID=")
    })
}

fn pin_root_device(cmdline: &str, part: &Path) -> String {
    let rest: Vec<&str> = cmdline
        .split_whitespace()
        .filter(|p| !p.starts_with("root=") && !p.starts_with("rootfstype="))
        .collect();
    let root = virtio_root_dev(part)
        .or_else(|| blkid_value(part, "PARTUUID").map(|uuid| format!("PARTUUID={uuid}")))
        .or_else(|| blkid_value(part, "UUID").map(|uuid| format!("UUID={uuid}")));
    let Some(root) = root else {
        return cmdline.to_string();
    };
    let mut out = format!("root={root}");
    if let Some(fstype) = blkid_value(part, "TYPE") {
        out.push_str(" rootfstype=");
        out.push_str(&fstype);
    }
    if !rest.is_empty() {
        out.push(' ');
        out.push_str(&rest.join(" "));
    }
    if !out.contains("rootwait") {
        out.push_str(" rootwait");
    }
    out
}

fn virtio_root_dev(part: &Path) -> Option<String> {
    let name = part.file_name()?.to_str()?;
    let idx = name.rfind('p')?;
    let num = &name[idx + 1..];
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("/dev/vda{num}"))
}

/// True when `fname` is a partition of loop device `loop_name` (`loop0p15` of `loop0`).
fn is_loop_partition(loop_name: &str, fname: &str) -> bool {
    let Some(rest) = fname.strip_prefix(loop_name) else {
        return false;
    };
    let Some(num) = rest.strip_prefix('p') else {
        return false;
    };
    !num.is_empty() && num.chars().all(|c| c.is_ascii_digit())
}

fn loop_partition_paths(device: &Path) -> Vec<PathBuf> {
    let name = device.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let parent = device.parent().unwrap_or(Path::new("/dev"));
    let mut parts = Vec::new();
    let Ok(rd) = fs::read_dir(parent) else {
        return parts;
    };
    for e in rd.flatten() {
        let Some(fname) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if is_loop_partition(name, &fname) {
            parts.push(e.path());
        }
    }
    parts.sort_by_key(|p| partition_index(p));
    parts
}

fn partition_index(path: &Path) -> u32 {
    path.file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit('p').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
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
        // Parallel guest starts all losetup the same way; serialize so partition
        // nodes are not scanned while another attach is still probing.
        static LOSETUP: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOSETUP.lock().unwrap_or_else(|err| err.into_inner());
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
        // p1 is often BIOS boot (no filesystem). Wait for the ESP (vfat), not
        // merely the first partition node, or we firmware-boot into shim.
        let device = PathBuf::from(device);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
        loop {
            let parts = loop_partition_paths(&device);
            let has_vfat = parts
                .iter()
                .any(|p| blkid_type(p).as_deref() == Some("vfat"));
            if has_vfat || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(Self { device })
    }

    fn parts(&self) -> Vec<PathBuf> {
        loop_partition_paths(&self.device)
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

    #[test]
    fn ubuntu_gpt_esp_is_p15() {
        assert!(is_loop_partition("loop0", "loop0p1"));
        assert!(is_loop_partition("loop0", "loop0p13"));
        assert!(is_loop_partition("loop0", "loop0p15"));
        assert!(!is_loop_partition("loop0", "loop0"));
        assert!(!is_loop_partition("loop0", "loop0p"));
        assert!(!is_loop_partition("loop1", "loop10p1"));
        assert!(!is_loop_partition("loop0", "loop0n1"));
        assert_eq!(partition_index(Path::new("/dev/loop0p15")), 15);
        assert_eq!(partition_index(Path::new("/dev/loop0p1")), 1);
    }

    #[test]
    #[ignore]
    fn ubuntu_cloud_gpt_kernel_boot() {
        let disk = std::env::var("PERTISK_UBUNTU_RAW").expect("PERTISK_UBUNTU_RAW");
        let dest = tempfile::tempdir().unwrap();
        let boot = prepare_shim_disk_boot(Path::new(&disk), dest.path())
            .unwrap()
            .expect("ubuntu shim disk should kernel-boot");
        assert!(
            boot.cmdline.contains("root=/dev/vda1"),
            "expected pinned root, got {}",
            boot.cmdline
        );
        assert!(boot.kernel.is_file(), "{}", boot.kernel.display());
        assert!(boot.initramfs.is_file(), "{}", boot.initramfs.display());
    }

    #[test]
    fn virtio_root_from_loop_partition() {
        assert_eq!(
            virtio_root_dev(Path::new("/dev/loop3p1")).as_deref(),
            Some("/dev/vda1")
        );
        assert_eq!(
            virtio_root_dev(Path::new("/dev/nbd0p15")).as_deref(),
            Some("/dev/vda15")
        );
    }

    #[test]
    fn label_root_needs_pin() {
        assert!(!cmdline_has_block_root(
            "root=LABEL=cloudimg-rootfs ro quiet"
        ));
        assert!(cmdline_has_block_root("root=UUID=abc ro"));
        assert!(cmdline_has_block_root("root=/dev/vda1 ro"));
        let pinned = pin_root_device(
            "root=LABEL=cloudimg-rootfs ro quiet",
            Path::new("/dev/loop0p1"),
        );
        assert!(pinned.starts_with("root=/dev/vda1"), "{pinned}");
        assert!(pinned.contains("ro"), "{pinned}");
        assert!(!pinned.contains("LABEL="), "{pinned}");
    }
}
