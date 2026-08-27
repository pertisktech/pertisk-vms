//! Shared identifiers, VM spec, host configuration, and inventory records.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:7480";
pub const DEFAULT_VCPUS: u8 = 1;
pub const DEFAULT_MEMORY_MIB: u32 = 512;

#[derive(Debug, thiserror::Error)]
pub enum TypesError {
    #[error("invalid vm id: {0}")]
    InvalidVmId(String),
    #[error("invalid volume id: {0}")]
    InvalidVolumeId(String),
    #[error("invalid vm spec: {0}")]
    InvalidSpec(String),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VmId(Uuid);

impl VmId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for VmId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for VmId {
    type Err = TypesError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| TypesError::InvalidVmId(s.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VolumeId(Uuid);

impl VolumeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for VolumeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for VolumeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for VolumeId {
    type Err = TypesError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| TypesError::InvalidVolumeId(s.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeFormat {
    #[default]
    Raw,
    Qcow2,
}

impl VolumeFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Qcow2 => "qcow2",
        }
    }

    pub fn extension(self) -> &'static str {
        self.as_str()
    }
}

impl fmt::Display for VolumeFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for VolumeFormat {
    type Err = TypesError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "raw" => Ok(Self::Raw),
            "qcow2" => Ok(Self::Qcow2),
            other => Err(TypesError::InvalidSpec(format!(
                "unknown volume format '{other}' (raw | qcow2)"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VolumeSnapshot {
    pub name: String,
    pub created_unix: u64,
    /// Copy-based snapshot file. Absent for qcow2 internal snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VolumeRecord {
    pub id: VolumeId,
    pub name: String,
    pub format: VolumeFormat,
    pub size_bytes: u64,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backing_id: Option<VolumeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<VolumeSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IsoRecord {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateVolumeRequest {
    pub name: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub format: VolumeFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloneVolumeRequest {
    pub name: String,
    #[serde(default)]
    pub linked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResizeVolumeRequest {
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachDiskRequest {
    pub volume_id: VolumeId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachIsoRequest {
    pub iso: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportIsoRequest {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriverKind {
    Mock,
    CloudHypervisor,
}

impl DriverKind {
    pub fn default_for_platform() -> Self {
        if cfg!(target_os = "linux") {
            Self::CloudHypervisor
        } else {
            Self::Mock
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::CloudHypervisor => "cloud-hypervisor",
        }
    }
}

impl fmt::Display for DriverKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DriverKind {
    type Err = TypesError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mock" => Ok(Self::Mock),
            "cloud-hypervisor" | "ch" => Ok(Self::CloudHypervisor),
            other => Err(TypesError::InvalidSpec(format!(
                "unknown driver '{other}' (mock | cloud-hypervisor)"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    Created,
    Running,
    Stopped,
    Failed,
}

impl fmt::Display for VmState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        };
        f.write_str(s)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiskSpec {
    pub path: PathBuf,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub cdrom: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_id: Option<VolumeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iso_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetSpec {
    /// TAP device name. Allocated in phase 3 if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VmSpec {
    pub name: String,
    #[serde(default = "default_vcpus")]
    pub vcpus: u8,
    #[serde(default = "default_memory_mib")]
    pub memory_mib: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmdline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initramfs: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<DiskSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nets: Vec<NetSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_log: Option<PathBuf>,
}

fn default_vcpus() -> u8 {
    DEFAULT_VCPUS
}

fn default_memory_mib() -> u32 {
    DEFAULT_MEMORY_MIB
}

impl VmSpec {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.name.trim().is_empty() {
            return Err(TypesError::InvalidSpec("name is required".into()));
        }
        if self.vcpus == 0 {
            return Err(TypesError::InvalidSpec("vcpus must be >= 1".into()));
        }
        if self.memory_mib < 64 {
            return Err(TypesError::InvalidSpec("memory_mib must be >= 64".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VmRecord {
    pub id: VmId,
    pub spec: VmSpec,
    pub state: VmState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_socket: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_log: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
}

fn default_listen() -> String {
    DEFAULT_LISTEN.to_string()
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmmConfig {
    pub driver: DriverKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_hypervisor: Option<PathBuf>,
    pub run_dir: PathBuf,
}

impl VmmConfig {
    pub fn default_for(home: &Path) -> Self {
        Self {
            driver: DriverKind::default_for_platform(),
            cloud_hypervisor: find_in_path("cloud-hypervisor"),
            run_dir: home.join("run"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageConfig {
    pub root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qemu_img: Option<PathBuf>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("storage"),
            qemu_img: find_in_path("qemu-img"),
        }
    }
}

impl StorageConfig {
    pub fn default_for(home: &Path) -> Self {
        Self {
            root: home.join("storage"),
            qemu_img: find_in_path("qemu-img"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostConfig {
    #[serde(default)]
    pub daemon: DaemonConfig,
    pub vmm: VmmConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

impl HostConfig {
    pub fn default_for(home: &Path) -> Self {
        Self {
            daemon: DaemonConfig::default(),
            vmm: VmmConfig::default_for(home),
            storage: StorageConfig::default_for(home),
        }
    }

    pub fn resolve_paths(&mut self, home: &Path) {
        if self.storage.root.is_relative() {
            self.storage.root = home.join(&self.storage.root);
        }
        if self.vmm.run_dir.is_relative() {
            self.vmm.run_dir = home.join(&self.vmm.run_dir);
        }
        if self.storage.qemu_img.is_none() {
            self.storage.qemu_img = find_in_path("qemu-img");
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostInfo {
    pub os: String,
    pub arch: String,
    pub kvm: bool,
    pub driver: DriverKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_hypervisor: Option<PathBuf>,
    pub listen: String,
    pub data_dir: PathBuf,
    #[serde(default)]
    pub storage_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qemu_img: Option<PathBuf>,
}

pub fn default_home() -> PathBuf {
    if let Ok(path) = std::env::var("PERTISK_HOME") {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pertisk")
}

pub fn kvm_available() -> bool {
    Path::new("/dev/kvm").exists()
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        candidate.is_file().then_some(candidate)
    })
}

pub fn probe_host(config: &HostConfig, data_dir: PathBuf) -> HostInfo {
    HostInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        kvm: kvm_available(),
        driver: config.vmm.driver,
        cloud_hypervisor: config
            .vmm
            .cloud_hypervisor
            .clone()
            .or_else(|| find_in_path("cloud-hypervisor")),
        listen: config.daemon.listen.clone(),
        data_dir,
        storage_root: config.storage.root.clone(),
        qemu_img: config
            .storage
            .qemu_img
            .clone()
            .or_else(|| find_in_path("qemu-img")),
    }
}

pub fn parse_size(input: &str) -> Result<u64, TypesError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(TypesError::InvalidSpec("size is required".into()));
    }
    let split = raw
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(raw.len());
    let (digits, suffix) = raw.split_at(split);
    let n: u64 = digits
        .parse()
        .map_err(|_| TypesError::InvalidSpec(format!("invalid size '{input}'")))?;
    let mul = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        "t" | "tb" | "tib" => 1024u64.pow(4),
        other => {
            return Err(TypesError::InvalidSpec(format!(
                "unknown size suffix '{other}'"
            )));
        }
    };
    n.checked_mul(mul)
        .ok_or_else(|| TypesError::InvalidSpec("size overflow".into()))
}

pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024u64.pow(4);
    if bytes >= TIB && bytes % TIB == 0 {
        format!("{}TiB", bytes / TIB)
    } else if bytes >= GIB && bytes % GIB == 0 {
        format!("{}GiB", bytes / GIB)
    } else if bytes >= MIB && bytes % MIB == 0 {
        format!("{}MiB", bytes / MIB)
    } else if bytes >= KIB && bytes % KIB == 0 {
        format!("{}KiB", bytes / KIB)
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_id_roundtrip() {
        let id = VmId::new();
        let parsed: VmId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn spec_rejects_empty_name() {
        let spec = VmSpec {
            name: "  ".into(),
            vcpus: 1,
            memory_mib: 512,
            kernel: None,
            cmdline: None,
            initramfs: None,
            disks: vec![],
            nets: vec![],
            serial_log: None,
        };
        assert!(spec.validate().is_err());
    }

    #[test]
    fn parse_size_gib() {
        assert_eq!(parse_size("10G").unwrap(), 10 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("512M").unwrap(), 512 * 1024 * 1024);
        assert_eq!(format_size(1024 * 1024 * 1024), "1GiB");
    }
}
