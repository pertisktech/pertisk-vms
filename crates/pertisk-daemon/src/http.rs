use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, CloneVolumeRequest, CreateVolumeRequest, ImportIsoRequest,
    ResizeVolumeRequest, SnapshotRequest, VmId, VmSpec, VolumeId,
};
use serde_json::json;

use crate::{DaemonError, Service};

pub fn router(service: Service) -> Router {
    Router::new()
        .route("/v1/host", get(host))
        .route("/v1/vms", get(list).post(create))
        .route("/v1/vms/{id}", get(show).delete(destroy))
        .route("/v1/vms/{id}/start", post(start))
        .route("/v1/vms/{id}/stop", post(stop))
        .route("/v1/vms/{id}/disks", post(attach_disk))
        .route("/v1/vms/{id}/disks/{volume_id}", axum::routing::delete(detach_disk))
        .route("/v1/vms/{id}/cdrom", post(attach_iso))
        .route("/v1/vms/{id}/cdrom/{iso}", axum::routing::delete(detach_iso))
        .route("/v1/volumes", get(list_volumes).post(create_volume))
        .route("/v1/volumes/{id}", get(show_volume).delete(delete_volume))
        .route("/v1/volumes/{id}/resize", post(resize_volume))
        .route("/v1/volumes/{id}/clone", post(clone_volume))
        .route("/v1/volumes/{id}/snapshots", post(snapshot_volume))
        .route(
            "/v1/volumes/{id}/snapshots/{name}/restore",
            post(restore_volume),
        )
        .route("/v1/isos", get(list_isos).post(import_iso))
        .route("/v1/isos/{name}", axum::routing::delete(delete_iso))
        .with_state(service)
}

async fn host(State(service): State<Service>) -> impl IntoResponse {
    Json(service.host_info())
}

async fn list(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list()?))
}

async fn create(
    State(service): State<Service>,
    Json(spec): Json<VmSpec>,
) -> Result<impl IntoResponse, DaemonError> {
    let record = service.create(spec).await?;
    Ok((StatusCode::CREATED, Json(record)))
}

async fn show(
    State(service): State<Service>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.get(id)?))
}

async fn start(
    State(service): State<Service>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.start(id).await?))
}

async fn stop(
    State(service): State<Service>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.stop(id).await?))
}

async fn destroy(
    State(service): State<Service>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    service.destroy(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_volumes(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_volumes()?))
}

async fn create_volume(
    State(service): State<Service>,
    Json(req): Json<CreateVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok((StatusCode::CREATED, Json(service.create_volume(req)?)))
}

async fn show_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.get_volume(id)?))
}

async fn delete_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
) -> Result<impl IntoResponse, DaemonError> {
    service.delete_volume(id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resize_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
    Json(req): Json<ResizeVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.resize_volume(id, req)?))
}

async fn clone_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
    Json(req): Json<CloneVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok((StatusCode::CREATED, Json(service.clone_volume(id, req)?)))
}

async fn snapshot_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
    Json(req): Json<SnapshotRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.snapshot_volume(id, req)?))
}

async fn restore_volume(
    State(service): State<Service>,
    Path((id, name)): Path<(VolumeId, String)>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.restore_volume(id, &name)?))
}

async fn list_isos(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_isos()?))
}

async fn import_iso(
    State(service): State<Service>,
    Json(req): Json<ImportIsoRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok((StatusCode::CREATED, Json(service.import_iso(req)?)))
}

async fn delete_iso(
    State(service): State<Service>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, DaemonError> {
    service.delete_iso(&name)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn attach_disk(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    Json(req): Json<AttachDiskRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.attach_disk(id, req)?))
}

async fn detach_disk(
    State(service): State<Service>,
    Path((id, volume_id)): Path<(VmId, VolumeId)>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.detach_disk(id, volume_id)?))
}

async fn attach_iso(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    Json(req): Json<AttachIsoRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.attach_iso(id, req)?))
}

async fn detach_iso(
    State(service): State<Service>,
    Path((id, iso)): Path<(VmId, String)>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.detach_iso(id, &iso)?))
}

impl IntoResponse for DaemonError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::NameTaken(_) => StatusCode::CONFLICT,
            Self::MustBeStopped(_, _) | Self::VolumeBusy(_) | Self::IsoBusy(_) => {
                StatusCode::CONFLICT
            }
            Self::Types(_) => StatusCode::BAD_REQUEST,
            Self::Storage(err) => storage_status(err),
            Self::Vmm(pertisk_vmm::VmmError::InvalidState { .. }) => StatusCode::CONFLICT,
            Self::Vmm(pertisk_vmm::VmmError::NotFound(_)) => StatusCode::NOT_FOUND,
            Self::Vmm(pertisk_vmm::VmmError::BinaryMissing) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

fn storage_status(err: &pertisk_storage::StorageError) -> StatusCode {
    use pertisk_storage::StorageError::*;
    match err {
        NotFound(_) | IsoNotFound(_) | SnapshotNotFound(_) => StatusCode::NOT_FOUND,
        NameTaken(_) | IsoExists(_) | SnapshotExists(_) => StatusCode::CONFLICT,
        InvalidIsoName(_) | CannotShrink { .. } | Message(_) => StatusCode::BAD_REQUEST,
        QemuImgRequired | LinkedRequiresQemu => StatusCode::BAD_REQUEST,
        Io(_) | Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
