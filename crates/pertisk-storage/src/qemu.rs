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
        self.run(&[
            "resize",
            &path.display().to_string(),
            &size.to_string(),
        ])
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
