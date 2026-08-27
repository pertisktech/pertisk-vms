//! Phase 4 public HTTP types. Phase 1 serves equivalent routes from `pertisk-daemon`.

use pertisk_types::{HostInfo, VmId, VmRecord, VmSpec};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateVmRequest {
    #[serde(flatten)]
    pub spec: VmSpec,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmList {
    pub vms: Vec<VmRecord>,
}

pub fn vm_path(id: VmId) -> String {
    format!("/v1/vms/{id}")
}

pub fn host_info_tag(info: &HostInfo) -> String {
    format!("{}-{}-{}", info.os, info.arch, info.driver)
}
