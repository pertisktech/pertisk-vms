use std::path::{Path, PathBuf};
use std::process::Command;

use pertisk_types::VolumeFormat;

use crate::{Result, StorageError};

#[derive(Debug)]
pub struct QemuImg {
    binary: Option<PathBuf>,
}

impl QemuImg {
    pub fn new(binary: Option<PathBuf>) -> Self {
        Self {
            binary: binary.or_else(|| pertisk_types::find_in_path("qemu-img")),
        }
    }

    pub fn available(&self) -> bool {
        self.binary.is_some()
    }

    pub fn binary(&self) -> Option<&Path> {
        self.binary.as_deref()
    }

    fn bin(&self) -> Result<&Path> {
        self.binary.as_deref().ok_or(StorageError::QemuImgRequired)
    }

    pub fn create_qcow2(&self, path: &Path, size: u64) -> Result<()> {
        self.run(&[
            "create",
            "-f",
            "qcow2",
            &path.display().to_string(),
            &size.to_string(),
        ])
    }

    pub fn resize(&self, path: &Path, size: u64) -> Result<()> {
        let path_s = path.display().to_string();
        let size_s = format_qemu_size(size);
        // `-f qcow2` avoids probe failures; `NG` avoids 32-bit parsers choking on raw bytes.
        self.run(&["resize", "-f", "qcow2", &path_s, &size_s])
    }

    pub fn linked_clone(
        &self,
        backing: &Path,
        backing_format: VolumeFormat,
        dest: &Path,
    ) -> Result<()> {
        self.run(&[
            "create",
            "-f",
            "qcow2",
            "-F",
            backing_format.as_str(),
            "-b",
            &backing.display().to_string(),
            &dest.display().to_string(),
        ])
    }

    pub fn snapshot_create(&self, image: &Path, name: &str) -> Result<()> {
        self.run(&["snapshot", "-c", name, &image.display().to_string()])
    }

    pub fn snapshot_apply(&self, image: &Path, name: &str) -> Result<()> {
        self.run(&["snapshot", "-a", name, &image.display().to_string()])
    }

    /// Virtual size from `qemu-img info` (qcow2 sparse file length is smaller).
    pub fn virtual_size(&self, path: &Path) -> Option<u64> {
        let bin = self.binary.as_deref()?;
        let output = Command::new(bin)
            .args(["info", "--output=json", &path.display().to_string()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        v.get("virtual-size").and_then(|x| x.as_u64())
    }

    fn run(&self, args: &[&str]) -> Result<()> {
        let bin = self.bin()?;
        let output = Command::new(bin).args(args).output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(StorageError::Message(format!(
                "qemu-img {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

/// qemu-img SIZE with a unit suffix so ARM/32-bit builds don't overflow on raw bytes.
pub(crate) fn format_qemu_size(size: u64) -> String {
    const G: u64 = 1024 * 1024 * 1024;
    const M: u64 = 1024 * 1024;
    const K: u64 = 1024;
    if size > 0 && size % G == 0 {
        format!("{}G", size / G)
    } else if size > 0 && size % M == 0 {
        format!("{}M", size / M)
    } else if size > 0 && size % K == 0 {
        format!("{}K", size / K)
    } else {
        size.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::format_qemu_size;

    #[test]
    fn qemu_size_uses_gib_suffix() {
        assert_eq!(format_qemu_size(50 * 1024 * 1024 * 1024), "50G");
        assert_eq!(format_qemu_size(75 * 1024 * 1024 * 1024), "75G");
        assert_eq!(format_qemu_size(1024 * 1024), "1M");
        assert_eq!(format_qemu_size(4097), "4097");
    }
}
