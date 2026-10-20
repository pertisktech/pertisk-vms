//! Optional Ceph RBD backend. Used when `storage.backend = "rbd"` and `rbd` is on PATH.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Result, StorageError};

#[derive(Debug)]
pub struct Rbd {
    binary: Option<PathBuf>,
    pool: String,
}

impl Rbd {
    pub fn new(pool: Option<String>) -> Self {
        Self {
            binary: pertisk_types::find_in_path("rbd"),
            pool: pool.unwrap_or_else(|| "rbd".into()),
        }
    }

    pub fn available() -> bool {
        pertisk_types::find_in_path("rbd").is_some()
    }

    pub fn pool(&self) -> &str {
        &self.pool
    }

    pub fn binary(&self) -> Option<&Path> {
        self.binary.as_deref()
    }

    pub fn image_path(&self, name: &str) -> String {
        format!("rbd:{}/{}", self.pool, name)
    }

    pub fn create_image(&self, name: &str, size_bytes: u64) -> Result<()> {
        let mib = size_mib(size_bytes);
        self.run(&[
            "create",
            &format!("{}/{}", self.pool, name),
            "--size",
            &mib,
        ])
    }

    /// Import a raw file as a new image. The caller converts qcow2 first.
    pub fn import_file(&self, source: &Path, name: &str) -> Result<()> {
        self.run(&[
            "import",
            &source.display().to_string(),
            &format!("{}/{}", self.pool, name),
        ])
    }

    /// Deep-copy one image to another name in the same pool.
    pub fn copy_image(&self, src: &str, dest: &str) -> Result<()> {
        self.run(&[
            "copy",
            &format!("{}/{}", self.pool, src),
            &format!("{}/{}", self.pool, dest),
        ])
    }

    pub fn resize(&self, name: &str, size_bytes: u64) -> Result<()> {
        let mib = size_mib(size_bytes);
        self.run(&[
            "resize",
            &format!("{}/{}", self.pool, name),
            "--size",
            &mib,
        ])
    }

    pub fn remove_image(&self, name: &str) -> Result<()> {
        let image = format!("{}/{}", self.pool, name);
        // Snapshots block `rbd rm`. A clone that still depends on one will fail the rm below.
        let _ = self.run(&["snap", "purge", &image]);
        self.run(&["rm", &image])
    }

    pub fn snapshot_create(&self, name: &str, snap: &str) -> Result<()> {
        self.run(&[
            "snap",
            "create",
            &format!("{}/{}@{}", self.pool, name, snap),
        ])
    }

    pub fn snapshot_rollback(&self, name: &str, snap: &str) -> Result<()> {
        self.run(&[
            "snap",
            "rollback",
            &format!("{}/{}@{}", self.pool, name, snap),
        ])
    }

    /// Older Ceph requires a protected snapshot before `rbd clone`. Newer releases ignore it.
    pub fn snapshot_protect(&self, name: &str, snap: &str) -> Result<()> {
        self.run(&[
            "snap",
            "protect",
            &format!("{}/{}@{}", self.pool, name, snap),
        ])
    }

    pub fn clone_image(&self, src: &str, snap: &str, dest: &str) -> Result<()> {
        self.run(&[
            "clone",
            &format!("{}/{}@{}", self.pool, src, snap),
            &format!("{}/{}", self.pool, dest),
        ])
    }

    fn run(&self, args: &[&str]) -> Result<()> {
        let bin = self
            .binary
            .as_deref()
            .ok_or_else(|| StorageError::Message("rbd binary not found".into()))?;
        let output = Command::new(bin).args(args).output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(StorageError::Message(format!(
                "rbd {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }
}

fn size_mib(size_bytes: u64) -> String {
    size_bytes.div_ceil(1024 * 1024).max(1).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_path_format() {
        let rbd = Rbd {
            binary: None,
            pool: "vms".into(),
        };
        assert_eq!(rbd.image_path("root"), "rbd:vms/root");
    }
}
