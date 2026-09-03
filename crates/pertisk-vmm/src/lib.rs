//! VMM backends. Mock on macOS; Cloud Hypervisor or QEMU on Linux.

mod cloud_hypervisor;
mod mock;
mod qemu;
mod qga;
mod unix_http;

use std::path::PathBuf;

use pertisk_types::{DriverKind, VmId, VmRecord, VmSpec, VmState};

pub use cloud_hypervisor::CloudHypervisorDriver;
pub use mock::MockDriver;
pub use qemu::QemuDriver;
pub use qga::ipv4_by_mac as qga_ipv4_by_mac;

#[derive(Debug, thiserror::Error)]
pub enum VmmError {
    #[error("{0}")]
    Message(String),
    #[error("vm not found: {0}")]
    NotFound(VmId),
    #[error("invalid vm state {state} for {op}")]
    InvalidState { state: VmState, op: &'static str },
    #[error("cloud-hypervisor binary not found")]
    BinaryMissing,
    #[error("qemu-system-x86_64 binary not found")]
    QemuBinaryMissing,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(String),
    #[error("cloud-hypervisor api {status}: {body}")]
    Api { status: u16, body: String },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, VmmError>;

#[derive(Debug)]
pub enum VmmBackend {
    Mock(MockDriver),
    CloudHypervisor(CloudHypervisorDriver),
    Qemu(QemuDriver),
}

impl VmmBackend {
    pub fn kind(&self) -> DriverKind {
        match self {
            Self::Mock(_) => DriverKind::Mock,
            Self::CloudHypervisor(_) => DriverKind::CloudHypervisor,
            Self::Qemu(_) => DriverKind::Qemu,
        }
    }

    pub fn from_config(
        kind: DriverKind,
        cloud_hypervisor: Option<PathBuf>,
        run_dir: PathBuf,
        firmware: Option<PathBuf>,
    ) -> Result<Self> {
        match kind {
            DriverKind::Mock => Ok(Self::Mock(MockDriver::new())),
            DriverKind::CloudHypervisor => {
                let binary = cloud_hypervisor
                    .or_else(|| pertisk_types::find_in_path("cloud-hypervisor"))
                    .ok_or(VmmError::BinaryMissing)?;
                Ok(Self::CloudHypervisor(CloudHypervisorDriver::new(
                    binary,
                    run_dir,
                    firmware.or_else(pertisk_types::find_firmware),
                )))
            }
            DriverKind::Qemu => {
                let binary = pertisk_types::find_in_path("qemu-system-x86_64")
                    .or_else(|| pertisk_types::find_in_path("qemu-system-x86"))
                    .ok_or(VmmError::QemuBinaryMissing)?;
                Ok(Self::Qemu(QemuDriver::new(binary, run_dir)))
            }
        }
    }

    pub async fn create(&self, id: VmId, spec: &VmSpec) -> Result<CreateResult> {
        match self {
            Self::Mock(driver) => driver.create(id, spec).await,
            Self::CloudHypervisor(driver) => driver.create(id, spec).await,
            Self::Qemu(driver) => driver.create(id, spec).await,
        }
    }

    pub async fn start(&self, record: &VmRecord) -> Result<StartResult> {
        match self {
            Self::Mock(driver) => driver.start(record).await,
            Self::CloudHypervisor(driver) => driver.start(record).await,
            Self::Qemu(driver) => driver.start(record).await,
        }
    }

    pub async fn stop(&self, record: &VmRecord) -> Result<()> {
        match self {
            Self::Mock(driver) => driver.stop(record).await,
            Self::CloudHypervisor(driver) => driver.stop(record).await,
            Self::Qemu(driver) => driver.stop(record).await,
        }
    }

    pub async fn shutdown(&self, record: &VmRecord) -> Result<()> {
        match self {
            Self::Mock(driver) => driver.shutdown(record).await,
            Self::CloudHypervisor(driver) => driver.shutdown(record).await,
            Self::Qemu(driver) => driver.shutdown(record).await,
        }
    }

    pub async fn restart(&self, record: &VmRecord) -> Result<()> {
        match self {
            Self::Mock(driver) => driver.restart(record).await,
            Self::CloudHypervisor(driver) => driver.restart(record).await,
            Self::Qemu(driver) => driver.restart(record).await,
        }
    }

    pub async fn destroy(&self, record: &VmRecord) -> Result<()> {
        match self {
            Self::Mock(driver) => driver.destroy(record).await,
            Self::CloudHypervisor(driver) => driver.destroy(record).await,
            Self::Qemu(driver) => driver.destroy(record).await,
        }
    }

    /// Whether the hypervisor still has this guest running (process or API state).
    pub async fn is_running(&self, record: &VmRecord) -> bool {
        match self {
            Self::Mock(driver) => driver.is_running(record),
            Self::CloudHypervisor(driver) => driver.is_running(record).await,
            Self::Qemu(driver) => driver.is_running(record).await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateResult {
    pub api_socket: Option<PathBuf>,
    pub pid: Option<u32>,
    pub serial_log: Option<PathBuf>,
    pub console_socket: Option<PathBuf>,
    pub graphics_socket: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct StartResult {
    pub pid: Option<u32>,
}
