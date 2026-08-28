//! Pull a Linux installer kernel out of an ISO so Cloud Hypervisor can boot it
//! without rust-hypervisor-firmware loading Ubuntu/Debian shim (Secure Boot).

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::StorageError;

const SECTOR: u64 = 2048;

/// Host-side kernel/initrd extracted from an installer ISO.
#[derive(Debug, Clone)]
pub struct LinuxIsoBoot {
    pub kernel: PathBuf,
    pub initramfs: PathBuf,
    pub cmdline: String,
}

const KERNEL_CANDIDATES: &[(&[&str], &[&str], &str)] = &[
    (
        &["casper", "vmlinuz"],
        &["casper", "initrd"],
        "console=ttyS0 boot=casper ---",
    ),
    (
        &["casper", "vmlinuz"],
        &["casper", "initrd.gz"],
        "console=ttyS0 boot=casper ---",
    ),
    (
        &["install.amd", "vmlinuz"],
        &["install.amd", "initrd.gz"],
        "console=ttyS0",
    ),
    (
        &["install.amd", "linux"],
        &["install.amd", "initrd.gz"],
        "console=ttyS0",
    ),
    (
        &["boot", "vmlinuz-virt"],
        &["boot", "initramfs-virt"],
        "console=ttyS0",
    ),
    (
        &["boot", "vmlinuz-lts"],
        &["boot", "initramfs-lts"],
        "console=ttyS0",
    ),
    (
        &["images", "pxeboot", "vmlinuz"],
        &["images", "pxeboot", "initrd.img"],
        "console=ttyS0 inst.text",
    ),
];

const SHIM_DIRS: &[&[&str]] = &[
    &["efi", "ubuntu"],
    &["efi", "debian"],
    &["efi", "fedora"],
    &["efi", "centos"],
    &["efi", "redhat"],
];

const SHIM_MSG: &str = "this ISO uses UEFI Secure Boot (shim). Cloud Hypervisor firmware \
(hypervisor-fw) cannot run it (import_mok_state / security protocol Unsupported). \
Use Alpine virt, an Ubuntu live ISO with casper/vmlinuz (pertisk kernel-boots those), \
or a cloud image + cloud-init. Graphical Ubuntu Desktop / Windows need VGA (not in this VMM).";

/// Extract installer kernel/initrd when the ISO has them. `Ok(None)` means firmware boot is fine.
/// Errors when the ISO is a shim-only distro that would hang at MokList.
pub fn prepare_linux_iso_boot(
    iso: &Path,
    dest_dir: &Path,
) -> Result<Option<LinuxIsoBoot>, StorageError> {
    let mut image = IsoImage::open(iso)?;
    let Some(roots) = image.roots()? else {
        return Ok(None);
    };

    for (kernel_path, initrd_path, cmdline) in KERNEL_CANDIDATES {
        let Some((k_lba, k_size, k_dir)) = image.lookup(&roots, kernel_path) else {
            continue;
        };
        let Some((i_lba, i_size, i_dir)) = image.lookup(&roots, initrd_path) else {
            continue;
        };
        if k_dir || i_dir || k_size == 0 || i_size == 0 {
            continue;
        }
        fs::create_dir_all(dest_dir)?;
        let kernel = dest_dir.join("vmlinuz");
        let initramfs = dest_dir.join("initrd");
        let cmd_file = dest_dir.join("cmdline");
        if cache_fresh(iso, &kernel, &initramfs, &cmd_file) {
            let cmdline = fs::read_to_string(&cmd_file)?.trim().to_string();
            return Ok(Some(LinuxIsoBoot {
                kernel,
                initramfs,
                cmdline,
            }));
        }
        image.copy_file(k_lba, k_size, &kernel)?;
        image.copy_file(i_lba, i_size, &initramfs)?;
        let cmdline = resolve_cmdline(cmdline, &roots.volume_id);
        fs::write(&cmd_file, format!("{cmdline}\n"))?;
        return Ok(Some(LinuxIsoBoot {
            kernel,
            initramfs,
            cmdline,
        }));
    }

    if SHIM_DIRS
        .iter()
        .any(|p| image.lookup(&roots, p).is_some_and(|(_, _, is_dir)| is_dir))
    {
        return Err(StorageError::Message(SHIM_MSG.into()));
    }
    Ok(None)
}

/// Anaconda's initrd (`inst.*` cmdlines) only finds the install tree when `inst.stage2`
/// points at the ISO volume label.
fn resolve_cmdline(base: &str, volume_id: &str) -> String {
    if volume_id.is_empty() || !base.contains("inst.") || base.contains("inst.stage2") {
        return base.to_string();
    }
    let label = volume_id.replace(' ', "\\x20");
    format!("{base} inst.stage2=hd:LABEL={label}")
}

fn cache_fresh(iso: &Path, kernel: &Path, initrd: &Path, cmdline: &Path) -> bool {
    let Ok(iso_meta) = fs::metadata(iso) else {
        return false;
    };
    let Ok(iso_mtime) = iso_meta.modified() else {
        return false;
    };
    for p in [kernel, initrd, cmdline] {
        let Ok(meta) = fs::metadata(p) else {
            return false;
        };
        if meta.len() == 0 {
            return false;
        }
        let Ok(mtime) = meta.modified() else {
            return false;
        };
        if mtime < iso_mtime {
            return false;
        }
    }
    true
}

struct IsoImage {
    file: File,
}

struct Roots {
    iso: Option<(u32, u32)>,
    joliet: Option<(u32, u32)>,
    volume_id: String,
}

impl IsoImage {
    fn open(path: &Path) -> Result<Self, StorageError> {
        Ok(Self {
            file: File::open(path)?,
        })
    }

    fn read_at(&mut self, lba: u32, len: u32) -> Result<Vec<u8>, StorageError> {
        self.file.seek(SeekFrom::Start(u64::from(lba) * SECTOR))?;
        let mut buf = vec![0u8; len as usize];
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn roots(&mut self) -> Result<Option<Roots>, StorageError> {
        let mut iso = None;
        let mut joliet = None;
        let mut volume_id = String::new();
        for lba in 16u32..32 {
            let sector = match self.read_at(lba, SECTOR as u32) {
                Ok(s) => s,
                Err(_) => break,
            };
            if sector.len() < 6 || &sector[1..6] != b"CD001" {
                break;
            }
            match sector[0] {
                255 => break,
                1 => {
                    iso = parse_root_record(&sector);
                    volume_id = parse_volume_id(&sector);
                }
                2 if is_joliet(&sector) => joliet = parse_root_record(&sector),
                _ => {}
            }
        }
        if iso.is_none() && joliet.is_none() {
            return Ok(None);
        }
        Ok(Some(Roots {
            iso,
            joliet,
            volume_id,
        }))
    }

    fn lookup(&mut self, roots: &Roots, parts: &[&str]) -> Option<(u32, u32, bool)> {
        if let Some((lba, size)) = roots.joliet
            && let Some(found) = self.walk(lba, size, true, parts)
        {
            return Some(found);
        }
        if let Some((lba, size)) = roots.iso {
            return self.walk(lba, size, false, parts);
        }
        None
    }

    fn walk(
        &mut self,
        mut lba: u32,
        mut size: u32,
        joliet: bool,
        parts: &[&str],
    ) -> Option<(u32, u32, bool)> {
        let mut is_dir = true;
        for (i, part) in parts.iter().enumerate() {
            let data = self.read_at(lba, size).ok()?;
            let ent = parse_dir(&data, joliet)
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(part))?;
            lba = ent.lba;
            size = ent.size;
            is_dir = ent.is_dir;
            if i + 1 < parts.len() && !is_dir {
                return None;
            }
        }
        Some((lba, size, is_dir))
    }

    fn copy_file(&mut self, lba: u32, size: u32, dest: &Path) -> Result<(), StorageError> {
        self.file.seek(SeekFrom::Start(u64::from(lba) * SECTOR))?;
        let mut limited = Read::by_ref(&mut self.file).take(u64::from(size));
        let mut out = File::create(dest)?;
        let copied = std::io::copy(&mut limited, &mut out)?;
        out.flush()?;
        drop(out);
        // A short read means the ISO itself is incomplete; booting it panics the guest at
        // "Initramfs unpacking failed" / "Unable to mount root fs", so fail loudly instead.
        if copied != u64::from(size) {
            let _ = fs::remove_file(dest);
            return Err(StorageError::Message(format!(
                "ISO is truncated: {} needs {size} bytes at LBA {lba}, only {copied} readable; re-download the image",
                dest.display()
            )));
        }
        Ok(())
    }
}

fn is_joliet(sector: &[u8]) -> bool {
    sector.len() > 90
        && sector[88] == 0x25
        && sector[89] == 0x2f
        && matches!(sector[90], 0x40 | 0x43 | 0x45)
}

fn parse_volume_id(sector: &[u8]) -> String {
    if sector.len() < 72 {
        return String::new();
    }
    String::from_utf8_lossy(&sector[40..72]).trim().to_string()
}

fn parse_root_record(sector: &[u8]) -> Option<(u32, u32)> {
    if sector.len() < 190 {
        return None;
    }
    let rec = &sector[156..];
    if rec[0] < 34 {
        return None;
    }
    let lba = u32::from_le_bytes(rec[2..6].try_into().ok()?);
    let size = u32::from_le_bytes(rec[10..14].try_into().ok()?);
    if lba == 0 || size == 0 {
        return None;
    }
    Some((lba, size))
}

struct DirEnt {
    name: String,
    lba: u32,
    size: u32,
    is_dir: bool,
}

fn parse_dir(data: &[u8], joliet: bool) -> Vec<DirEnt> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let len = data[i] as usize;
        if len == 0 {
            i = (i / 2048 + 1) * 2048;
            continue;
        }
        if i + len > data.len() {
            break;
        }
        let rec = &data[i..i + len];
        let ident_len = rec.get(32).copied().unwrap_or(0) as usize;
        if rec.len() < 33 + ident_len {
            break;
        }
        let ident = &rec[33..33 + ident_len];
        if ident == [0] || ident == [1] {
            i += len;
            continue;
        }
        let name = if joliet {
            decode_joliet(ident)
        } else {
            decode_iso9660(ident)
        };
        if let (Ok(lba_b), Ok(size_b)) = (rec[2..6].try_into(), rec[10..14].try_into()) {
            let lba = u32::from_le_bytes(lba_b);
            let size = u32::from_le_bytes(size_b);
            let is_dir = rec[25] & 0x02 != 0;
            if !name.is_empty() {
                out.push(DirEnt {
                    name,
                    lba,
                    size,
                    is_dir,
                });
            }
        }
        i += len;
    }
    out
}

fn decode_iso9660(ident: &[u8]) -> String {
    let raw = String::from_utf8_lossy(ident);
    let trimmed = raw.split(';').next().unwrap_or(&raw).trim_end_matches('.');
    trimmed.to_string()
}

fn decode_joliet(ident: &[u8]) -> String {
    let units: Vec<u16> = ident
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let raw = String::from_utf16_lossy(&units);
    raw.split(';')
        .next()
        .unwrap_or(&raw)
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso9660::{cidata_iso, tree_iso};

    #[test]
    fn cidata_has_no_linux_boot() {
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("cidata.iso");
        std::fs::write(&iso, cidata_iso(b"#cloud-config\n", b"instance-id: i-1\n")).unwrap();
        let boot = prepare_linux_iso_boot(&iso, &dir.path().join("out")).unwrap();
        assert!(boot.is_none());
    }

    #[test]
    fn extracts_ubuntu_casper_kernel() {
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("ubuntu.iso");
        std::fs::write(
            &iso,
            tree_iso(&[
                ("casper/vmlinuz", b"kernel-bytes"),
                ("casper/initrd", b"initrd-bytes"),
                ("efi/ubuntu/grubx64.efi", b"shim-would-be-here"),
            ]),
        )
        .unwrap();
        let boot = prepare_linux_iso_boot(&iso, &dir.path().join("out"))
            .unwrap()
            .expect("casper kernel");
        assert_eq!(std::fs::read(&boot.kernel).unwrap(), b"kernel-bytes");
        assert_eq!(std::fs::read(&boot.initramfs).unwrap(), b"initrd-bytes");
        assert!(boot.cmdline.contains("console=ttyS0"));
    }

    #[test]
    fn shim_without_kernel_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("ubuntu.iso");
        std::fs::write(&iso, tree_iso(&[("efi/ubuntu/grubx64.efi", b"shim")])).unwrap();
        let err = prepare_linux_iso_boot(&iso, &dir.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("shim"), "{err}");
    }
}
