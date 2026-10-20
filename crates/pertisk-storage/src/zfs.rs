//! Optional ZFS zvol backend. Used when `storage.backend = "zfs"` and `zfs` is on PATH.
//!
//! A volume is one sparse zvol under `storage.zfs_pool` (default `tank/pertisk`).
//! Inventory stores `zfs:<dataset>`. QEMU and Cloud Hypervisor open `/dev/zvol/<dataset>`.
//! The zvol stays on this node: it is not copied to peers and it is not HA storage.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Result, StorageError};

#[derive(Debug)]
pub struct Zfs {
    binary: Option<PathBuf>,
    parent: String,
}

impl Zfs {
    pub fn new(parent: Option<String>) -> Self {
        Self {
            binary: pertisk_types::find_in_path("zfs"),
            parent: parent.unwrap_or_else(|| "tank/pertisk".into()),
        }
    }

    pub fn available() -> bool {
        pertisk_types::find_in_path("zfs").is_some()
    }

    pub fn parent(&self) -> &str {
        &self.parent
    }

    pub fn dataset_name(&self, name: &str) -> String {
        format!("{}/{}", self.parent.trim_matches('/'), sanitize(name))
    }

    /// Inventory path. Not a file on disk.
    pub fn image_path(&self, name: &str) -> String {
        format!("zfs:{}", self.dataset_name(name))
    }

    pub fn dataset_from_path(path: &Path) -> Result<String> {
        let text = path.to_string_lossy();
        let dataset = text
            .strip_prefix("zfs:")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                StorageError::Message(format!("not a zfs volume path: {}", path.display()))
            })?;
        Ok(dataset.to_string())
    }

    pub fn device_path(dataset: &str) -> PathBuf {
        PathBuf::from(format!("/dev/zvol/{dataset}"))
    }

    pub fn create_zvol(&self, name: &str, size_bytes: u64) -> Result<()> {
        let size = size_arg(size_bytes);
        self.run(&["create", "-p", "-s", "-V", &size, &self.dataset_name(name)])
    }

    pub fn import_raw(&self, source: &Path, name: &str, size_bytes: u64) -> Result<()> {
        let file_len = source.metadata().map(|m| m.len()).unwrap_or(0);
        self.create_zvol(name, size_bytes.max(file_len))?;
        let dataset = self.dataset_name(name);
        let device = Self::device_path(&dataset);
        let output = Command::new("dd")
            .arg(format!("if={}", source.display()))
            .arg(format!("of={}", device.display()))
            .arg("bs=1M")
            .arg("status=none")
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            let _ = self.destroy_dataset(&dataset);
            Err(StorageError::Message(format!(
                "dd onto {} failed: {}",
                device.display(),
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    pub fn resize_dataset(&self, dataset: &str, size_bytes: u64) -> Result<()> {
        let size = size_arg(size_bytes);
        self.run(&["set", &format!("volsize={size}"), dataset])
    }

    /// Snapshots of this dataset only. A clone that still depends on one will fail the destroy.
    pub fn destroy_dataset(&self, dataset: &str) -> Result<()> {
        self.run(&["destroy", "-r", dataset])
    }

    pub fn snapshot_dataset(&self, dataset: &str, snap: &str) -> Result<()> {
        self.run(&["snapshot", &format!("{dataset}@{}", sanitize(snap))])
    }

    pub fn rollback_dataset(&self, dataset: &str, snap: &str) -> Result<()> {
        self.run(&["rollback", "-r", &format!("{dataset}@{}", sanitize(snap))])
    }

    pub fn clone_dataset(&self, dataset: &str, snap: &str, dest: &str) -> Result<()> {
        self.run(&[
            "clone",
            &format!("{dataset}@{}", sanitize(snap)),
            &self.dataset_name(dest),
        ])
    }

    /// Full copy: temporary snapshot, `zfs send | zfs receive`, then drop the snapshot.
    pub fn copy_dataset(&self, dataset: &str, dest: &str) -> Result<()> {
        let snap = "pertisk-fullcopy";
        let from = format!("{dataset}@{snap}");
        self.run(&["snapshot", &from])?;
        let copied = self.send_receive(&from, &self.dataset_name(dest));
        let _ = self.run(&["destroy", &from]);
        copied
    }

    fn send_receive(&self, snapshot: &str, dest: &str) -> Result<()> {
        let bin = self
            .binary
            .as_deref()
            .ok_or_else(|| StorageError::Message("zfs binary not found".into()))?;
        let mut send = Command::new(bin)
            .args(["send", snapshot])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = send
            .stdout
            .take()
            .ok_or_else(|| StorageError::Message("zfs send produced no stdout".into()))?;
        let recv = Command::new(bin)
            .args(["receive", "-F", dest])
            .stdin(stdout)
            .output()?;
        let send_status = send.wait()?;
        if recv.status.success() && send_status.success() {
            return Ok(());
        }
        let mut detail = String::from_utf8_lossy(&recv.stderr).trim().to_string();
        if let Ok(err) = send.stderr.take().map(read_stderr).transpose() {
            if let Some(err) = err.filter(|s| !s.is_empty()) {
                if !detail.is_empty() {
                    detail.push_str("; ");
                }
                detail.push_str(&err);
            }
        }
        Err(StorageError::Message(format!(
            "zfs send {snapshot} | zfs receive {dest} failed: {detail}"
        )))
    }

    fn run(&self, args: &[&str]) -> Result<()> {
        let bin = self
            .binary
            .as_deref()
            .ok_or_else(|| StorageError::Message("zfs binary not found".into()))?;
        let output = Command::new(bin).args(args).output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(StorageError::Message(format!(
                "zfs {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}

fn read_stderr(mut pipe: std::process::ChildStderr) -> Result<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    pipe.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

fn sanitize(name: &str) -> String {
    let token: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .take(80)
        .collect();
    let token = token.trim_matches(|c| c == '-' || c == '.').to_string();
    if token.is_empty() {
        "vol".into()
    } else {
        token
    }
}

fn size_arg(size_bytes: u64) -> String {
    format!("{}M", size_bytes.div_ceil(1024 * 1024).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_path_is_a_zvol_dataset() {
        let zfs = Zfs {
            binary: None,
            parent: "tank/pertisk".into(),
        };
        assert_eq!(zfs.image_path("root disk"), "zfs:tank/pertisk/root-disk");
        assert_eq!(
            Zfs::device_path("tank/pertisk/root-disk"),
            PathBuf::from("/dev/zvol/tank/pertisk/root-disk")
        );
        assert_eq!(
            Zfs::dataset_from_path(Path::new("zfs:tank/pertisk/root-disk")).unwrap(),
            "tank/pertisk/root-disk"
        );
    }
}
