//! Public API types: auth, tasks, audit, OpenAPI.

use pertisk_types::{HostInfo, VmId, VmRecord, VmSpec};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Viewer => 1,
            Self::Operator => 2,
            Self::Admin => 3,
        }
    }

    pub fn allows(self, needed: Role) -> bool {
        self.rank() >= needed.rank()
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "viewer" => Ok(Self::Viewer),
            "operator" => Ok(Self::Operator),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown role '{other}'")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRecord {
    pub id: String,
    pub username: String,
    pub role: Role,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub token: String,
    pub username: String,
    pub role: Role,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Role,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Running,
    Done,
    Error,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Error => "error",
        };
        f.write_str(s)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub kind: String,
    pub status: TaskStatus,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_unix: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub actor: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub created_unix: u64,
}

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

/// Minimal OpenAPI 3 document for the control-plane API.
pub fn openapi_json() -> serde_json::Value {
    serde_json::json!({
        "openapi": "3.0.3",
        "info": {
            "title": "pertisk-vm API",
            "version": "0.1.0",
            "description": "Virtualization control plane (single-node or clustered)"
        },
        "servers": [{ "url": "/" }],
        "security": [{ "bearerAuth": [] }],
        "components": {
            "securitySchemes": {
                "bearerAuth": { "type": "http", "scheme": "bearer" }
            }
        },
        "paths": {
            "/v1/health": { "get": { "summary": "Liveness", "security": [] } },
            "/v1/login": { "post": { "summary": "Create an API token", "security": [] } },
            "/v1/session": { "get": { "summary": "Current user" } },
            "/v1/openapi.json": { "get": { "summary": "This document", "security": [] } },
            "/v1/host": { "get": { "summary": "Host capabilities" } },
            "/v1/vms": {
                "get": { "summary": "List VMs" },
                "post": { "summary": "Define a VM" }
            },
            "/v1/vms/{id}": {
                "get": { "summary": "Show VM" },
                "delete": { "summary": "Destroy VM" }
            },
            "/v1/vms/{id}/start": { "post": { "summary": "Start VM" } },
            "/v1/vms/{id}/stop": { "post": { "summary": "Stop VM" } },
            "/v1/vms/{id}/console": { "get": { "summary": "Console metadata" } },
            "/v1/vms/{id}/console/serial": { "get": { "summary": "Serial log chunk" } },
            "/v1/vms/{id}/disks": { "post": { "summary": "Attach volume" } },
            "/v1/vms/{id}/cdrom": { "post": { "summary": "Attach ISO" } },
            "/v1/vms/{id}/nics": { "post": { "summary": "Attach NIC" } },
            "/v1/volumes": { "get": { "summary": "List volumes" }, "post": { "summary": "Create volume (replica_count via replicas)" } },
            "/v1/volumes/{id}/clone": { "post": { "summary": "Clone volume" } },
            "/v1/peer/volumes/ensure": { "post": { "summary": "Peer: create local replica file" } },
            "/v1/peer/volumes/{id}/blob": { "get": { "summary": "Peer: read replica blob" }, "put": { "summary": "Peer: write replica blob" } },
            "/v1/networks": { "get": { "summary": "List networks" }, "post": { "summary": "Create network" } },
            "/v1/isos": { "get": { "summary": "List ISOs" }, "post": { "summary": "Import ISO" } },
            "/v1/tasks": { "get": { "summary": "Task log" } },
            "/v1/audit": { "get": { "summary": "Audit events" } },
            "/v1/users": { "get": { "summary": "List users (admin)" }, "post": { "summary": "Create user (admin)" } },
            "/v1/cluster": { "get": { "summary": "Cluster membership and quorum" } },
            "/v1/cluster/join": { "post": { "summary": "Join an existing cluster" } },
            "/v1/vms/{id}/migrate": { "post": { "summary": "Migrate VM to another node" } }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_rank() {
        assert!(Role::Admin.allows(Role::Operator));
        assert!(!Role::Viewer.allows(Role::Operator));
    }

    #[test]
    fn openapi_has_login() {
        let doc = openapi_json();
        assert_eq!(doc["openapi"], "3.0.3");
        assert!(doc["paths"]["/v1/login"].is_object());
    }
}
