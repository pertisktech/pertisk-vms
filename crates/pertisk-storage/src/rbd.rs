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
        let mib = size_bytes.div_ceil(1024 * 1024).max(1);
        self.run(&[
            "create",
            &format!("{}/{}", self.pool, name),
            "--size",
            &mib.to_string(),
        ])
    }

    pub fn remove_image(&self, name: &str) -> Result<()> {
        self.run(&["rm", &format!("{}/{}", self.pool, name)])
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
