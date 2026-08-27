use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json};
use axum::extract::Request;
use pertisk_api::{CreateUserRequest, LoginRequest, Role, openapi_json};
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, AttachNicRequest, CloneVolumeRequest, ClusterSnapshot,
    CreateNetworkRequest, CreateVolumeRequest, HeartbeatMessage, ImportIsoRequest, JoinClusterRequest,
    MigrateRequest, NodeRecord, ResizeVolumeRequest, SnapshotRequest, VmId, VmRecord, VmSpec,
    VolumeId, VolumeRecord,
};
use serde::Deserialize;
use serde_json::json;

use crate::control::AuthUser;
use crate::{DaemonError, Service};

pub fn router(service: Service) -> Router {
    let protected = Router::new()
        .route("/v1/session", get(session))
        .route("/v1/host", get(host))
        .route("/v1/vms", get(list).post(create))
        .route("/v1/vms/{id}", get(show).delete(destroy))
        .route("/v1/vms/{id}/start", post(start))
        .route("/v1/vms/{id}/stop", post(stop))
        .route("/v1/vms/{id}/migrate", post(migrate))
        .route("/v1/vms/{id}/disks", post(attach_disk))
        .route("/v1/vms/{id}/disks/{volume_id}", axum::routing::delete(detach_disk))
        .route("/v1/vms/{id}/cdrom", post(attach_iso))
        .route("/v1/vms/{id}/cdrom/{iso}", axum::routing::delete(detach_iso))
        .route("/v1/vms/{id}/nics", post(attach_nic))
        .route("/v1/vms/{id}/nics/{tap}", axum::routing::delete(detach_nic))
        .route("/v1/vms/{id}/console", get(console_info))
        .route("/v1/vms/{id}/console/serial", get(console_serial))
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
        .route("/v1/networks", get(list_networks).post(create_network))
        .route(
            "/v1/networks/{id}",
            get(show_network).delete(delete_network),
        )
        .route("/v1/tasks", get(list_tasks))
        .route("/v1/audit", get(list_audit))
        .route("/v1/users", get(list_users).post(create_user))
        .route("/v1/users/{id}", axum::routing::delete(delete_user))
        .route("/v1/cluster", get(cluster_status))
        .route("/v1/cluster/join", post(cluster_join))
        .route("/v1/cluster/leave", post(cluster_leave))
        .route("/v1/cluster/accept", post(cluster_accept))
        .route("/v1/peer/heartbeat", post(peer_heartbeat))
        .route("/v1/peer/snapshot", post(peer_snapshot))
        .route("/v1/peer/accept", post(peer_accept))
        .route("/v1/peer/run", post(peer_run))
        .route("/v1/peer/stop", post(peer_stop))
        .route("/v1/peer/drop", post(peer_drop))
        .route("/v1/peer/volumes/ensure", post(peer_volume_ensure))
        .route("/v1/peer/volumes/{id}", axum::routing::delete(peer_volume_delete))
        .route(
            "/v1/peer/volumes/{id}/stat",
            get(peer_volume_stat),
        )
        .route(
            "/v1/peer/volumes/{id}/blob",
            get(peer_volume_blob_get).put(peer_volume_blob_put),
        )
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024));

    Router::new()
        .route("/", get(ui))
        .route("/v1/health", get(health))
        .route("/v1/login", post(login))
        .route("/v1/openapi.json", get(openapi))
        .merge(protected)
        .with_state(service)
}

async fn ui() -> Html<&'static str> {
    Html(include_str!("../ui/index.html"))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn openapi() -> impl IntoResponse {
    Json(openapi_json())
}

async fn login(
    State(service): State<Service>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.login(&req.username, &req.password)?))
}

async fn session(Extension(user): Extension<AuthUser>) -> impl IntoResponse {
    Json(json!({
        "id": user.id,
        "username": user.username,
        "role": user.role,
    }))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn required_role(method: &Method, path: &str) -> Role {
    if path.starts_with("/v1/users") {
        Role::Admin
    } else if *method == Method::GET {
        Role::Viewer
    } else {
        Role::Operator
    }
}

async fn auth_middleware(
    State(service): State<Service>,
    mut req: Request,
    next: Next,
) -> Result<Response, DaemonError> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let peer = req
        .headers()
        .get("x-pertisk-peer")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if path.starts_with("/v1/peer/") {
        let secret = peer.ok_or(crate::control::ControlError::Unauthorized)?;
        if !service.peer_secret_ok(&secret) {
            return Err(crate::control::ControlError::Unauthorized.into());
        }
        req.extensions_mut().insert(AuthUser {
            id: "peer".into(),
            username: "cluster".into(),
            role: Role::Admin,
        });
        return Ok(next.run(req).await);
    }
    let token = bearer_token(req.headers()).ok_or(crate::control::ControlError::Unauthorized)?;
    let user = service.authenticate(&token)?;
    if !user.role.allows(required_role(&method, &path)) {
        return Err(crate::control::ControlError::Forbidden.into());
    }
    if method != Method::GET {
        let _ = service.audit(&user.username, &format!("{method} {path}"), None);
    }
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

async fn tracked<T>(
    service: &Service,
    user: &AuthUser,
    kind: &str,
    target: String,
    op: impl std::future::Future<Output = Result<T, DaemonError>>,
) -> Result<T, DaemonError> {
    let task = service.begin_task(&user.username, kind, Some(&target))?;
    match op.await {
        Ok(value) => {
            service.finish_task(&task.id, Ok(()))?;
            Ok(value)
        }
        Err(err) => {
            let _ = service.finish_task(&task.id, Err(err.to_string()));
            Err(err)
        }
    }
}

async fn host(State(service): State<Service>) -> impl IntoResponse {
    Json(service.host_info())
}

async fn list(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list()?))
}

async fn create(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Json(spec): Json<VmSpec>,
) -> Result<impl IntoResponse, DaemonError> {
    let record = tracked(
        &service,
        &user,
        "vm.create",
        spec.name.clone(),
        service.create(spec),
    )
    .await?;
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
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(&service, &user, "vm.start", id.to_string(), service.start(id)).await?,
    ))
}

async fn stop(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(&service, &user, "vm.stop", id.to_string(), service.stop(id)).await?,
    ))
}

async fn destroy(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    tracked(
        &service,
        &user,
        "vm.destroy",
        id.to_string(),
        service.destroy(id),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn migrate(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
    Json(req): Json<MigrateRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(
            &service,
            &user,
            "vm.migrate",
            id.to_string(),
            service.migrate(id, req.target),
        )
        .await?,
    ))
}

async fn list_volumes(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_volumes()?))
}

async fn create_volume(
    State(service): State<Service>,
    Json(req): Json<CreateVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok((StatusCode::CREATED, Json(service.create_volume(req).await?)))
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
    service.delete_volume(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resize_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
    Json(req): Json<ResizeVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.resize_volume(id, req).await?))
}

async fn clone_volume(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VolumeId>,
    Json(req): Json<CloneVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    let name = req.name.clone();
    Ok((
        StatusCode::CREATED,
        Json(
            tracked(
                &service,
                &user,
                "volume.clone",
                name,
                service.clone_volume(id, req),
            )
            .await?,
        ),
    ))
}

async fn snapshot_volume(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
    Json(req): Json<SnapshotRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.snapshot_volume(id, req).await?))
}

async fn restore_volume(
    State(service): State<Service>,
    Path((id, name)): Path<(VolumeId, String)>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.restore_volume(id, &name).await?))
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

#[derive(Deserialize)]
struct SerialQuery {
    #[serde(default)]
    from: u64,
    #[serde(default = "default_serial_max")]
    max: u64,
}

fn default_serial_max() -> u64 {
    8192
}

async fn attach_nic(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    Json(req): Json<AttachNicRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.attach_nic(id, req)?))
}

async fn detach_nic(
    State(service): State<Service>,
    Path((id, tap)): Path<(VmId, String)>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.detach_nic(id, &tap)?))
}

async fn console_info(
    State(service): State<Service>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.console_info(id)?))
}

async fn console_serial(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    Query(query): Query<SerialQuery>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.console_serial(id, query.from, query.max)?))
}

async fn list_networks(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_networks()?))
}

async fn create_network(
    State(service): State<Service>,
    Json(req): Json<CreateNetworkRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok((StatusCode::CREATED, Json(service.create_network(req)?)))
}

async fn show_network(
    State(service): State<Service>,
    Path(id): Path<pertisk_types::NetworkId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.get_network(id)?))
}

async fn delete_network(
    State(service): State<Service>,
    Path(id): Path<pertisk_types::NetworkId>,
) -> Result<impl IntoResponse, DaemonError> {
    service.delete_network(id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_tasks(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_tasks()?))
}

async fn list_audit(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_audit()?))
}

async fn list_users(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.list_users()?))
}

async fn create_user(
    State(service): State<Service>,
    Json(req): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok((StatusCode::CREATED, Json(service.create_user(req)?)))
}

async fn delete_user(
    State(service): State<Service>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, DaemonError> {
    service.delete_user(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn cluster_status(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.cluster_status()?))
}

async fn cluster_join(
    State(service): State<Service>,
    Json(req): Json<JoinClusterRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        service
            .join_cluster(&req.peer, &req.username, &req.password)
            .await?,
    ))
}

async fn cluster_leave(State(service): State<Service>) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.leave_cluster()?))
}

async fn cluster_accept(
    State(service): State<Service>,
    Json(node): Json<NodeRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.accept_node(node).await?))
}

async fn peer_accept(
    State(service): State<Service>,
    Json(node): Json<NodeRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_accept(node)?))
}

async fn peer_heartbeat(
    State(service): State<Service>,
    Json(msg): Json<HeartbeatMessage>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.on_heartbeat(msg)?))
}

async fn peer_snapshot(
    State(service): State<Service>,
    Json(snap): Json<ClusterSnapshot>,
) -> Result<impl IntoResponse, DaemonError> {
    service.apply_snapshot(snap)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn peer_run(
    State(service): State<Service>,
    Json(record): Json<VmRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_run(record).await?))
}

async fn peer_stop(
    State(service): State<Service>,
    Json(record): Json<VmRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_stop(record).await?))
}

async fn peer_drop(
    State(service): State<Service>,
    Json(record): Json<VmRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    service.apply_drop(&record).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn peer_volume_ensure(
    State(service): State<Service>,
    Json(record): Json<VolumeRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_ensure_volume(record)?))
}

async fn peer_volume_delete(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
) -> Result<impl IntoResponse, DaemonError> {
    service.apply_delete_replica(id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn peer_volume_stat(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.volume_stat(id)?))
}

async fn peer_volume_blob_get(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
) -> Result<impl IntoResponse, DaemonError> {
    let rec = service.get_volume(id)?;
    let data = std::fs::read(&rec.path)?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        data,
    ))
}

async fn peer_volume_blob_put(
    State(service): State<Service>,
    Path(id): Path<VolumeId>,
    body: Bytes,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_volume_blob(id, &body)?))
}

impl IntoResponse for DaemonError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::NameTaken(_) => StatusCode::CONFLICT,
            Self::MustBeStopped(_, _)
            | Self::VolumeBusy(_)
            | Self::IsoBusy(_)
            | Self::NetworkBusy(_) => StatusCode::CONFLICT,
            Self::Types(_) => StatusCode::BAD_REQUEST,
            Self::Control(crate::control::ControlError::Unauthorized)
            | Self::Control(crate::control::ControlError::BadCredentials) => {
                StatusCode::UNAUTHORIZED
            }
            Self::Control(crate::control::ControlError::Forbidden) => StatusCode::FORBIDDEN,
            Self::Control(crate::control::ControlError::UserNotFound(_)) => StatusCode::NOT_FOUND,
            Self::Control(crate::control::ControlError::UserExists(_)) => StatusCode::CONFLICT,
            Self::Control(crate::control::ControlError::Message(_)) => StatusCode::BAD_REQUEST,
            Self::NoQuorum | Self::Fenced | Self::Unschedulable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Peer(_) => StatusCode::BAD_GATEWAY,
            Self::Storage(err) => storage_status(err),
            Self::Net(err) => net_status(err),
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

fn net_status(err: &pertisk_net::NetError) -> StatusCode {
    use pertisk_net::NetError::*;
    match err {
        NotFound(_) => StatusCode::NOT_FOUND,
        NameTaken(_) | PoolExhausted(_) => StatusCode::CONFLICT,
        Invalid(_) | Host(_) => StatusCode::BAD_REQUEST,
        Io(_) | Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlStore, Service, Store};
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use pertisk_api::{CreateUserRequest, Role};
    use pertisk_net::NetworkPool;
    use pertisk_storage::VolumePool;
    use pertisk_types::{DriverKind, HostConfig};
    use pertisk_vmm::VmmBackend;
    use tower::ServiceExt;

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vms.json")).unwrap();
        let volumes = VolumePool::open(dir.path().join("storage"), None).unwrap();
        let networks = NetworkPool::open(dir.path().join("net"), false).unwrap();
        let control = ControlStore::open(dir.path().join("control.db"), Some("admin")).unwrap();
        let config = HostConfig::default_for(dir.path());
        let vmm = VmmBackend::from_config(DriverKind::Mock, None, dir.path().join("run")).unwrap();
        (
            Service::new(
                vmm,
                store,
                volumes,
                networks,
                control,
                config,
                dir.path().to_path_buf(),
            ),
            dir,
        )
    }

    async fn send(
        app: &Router,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let req = if let Some(body) = body {
            builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        };
        let response = app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            serde_json::json!(null)
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) })
            })
        };
        (status, json)
    }

    #[tokio::test]
    async fn health_and_openapi_are_public() {
        let (svc, _dir) = service();
        let app = router(svc);
        let (status, body) = send(&app, Method::GET, "/v1/health", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        let (status, body) = send(&app, Method::GET, "/v1/openapi.json", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["openapi"], "3.0.3");
        let (status, _) = send(&app, Method::GET, "/", None, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_routes_require_token() {
        let (svc, _dir) = service();
        let app = router(svc);
        let (status, body) = send(&app, Method::GET, "/v1/host", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body["error"].as_str().unwrap().contains("unauthorized"));
        let (status, _) = send(
            &app,
            Method::POST,
            "/v1/login",
            None,
            Some(json!({ "username": "admin", "password": "wrong" })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, login) = send(
            &app,
            Method::POST,
            "/v1/login",
            None,
            Some(json!({ "username": "admin", "password": "admin" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let token = login["token"].as_str().unwrap();
        let (status, host) = send(&app, Method::GET, "/v1/host", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(host["driver"].is_string());
        let (status, created) = send(
            &app,
            Method::POST,
            "/v1/vms",
            Some(token),
            Some(json!({
                "name": "ui-demo",
                "vcpus": 1,
                "memory_mib": 512
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, tasks) = send(&app, Method::GET, "/v1/tasks", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(tasks[0]["kind"], "vm.create");
        let (status, audit) = send(&app, Method::GET, "/v1/audit", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!audit.as_array().unwrap().is_empty());
        let _ = created;
    }

    #[tokio::test]
    async fn viewer_cannot_mutate() {
        let (svc, _dir) = service();
        svc.create_user(CreateUserRequest {
            username: "view".into(),
            password: "viewpass".into(),
            role: Role::Viewer,
        })
        .unwrap();
        let app = router(svc);
        let (status, login) = send(
            &app,
            Method::POST,
            "/v1/login",
            None,
            Some(json!({ "username": "view", "password": "viewpass" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let token = login["token"].as_str().unwrap();
        let (status, _) = send(&app, Method::GET, "/v1/vms", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(
            &app,
            Method::POST,
            "/v1/vms",
            Some(token),
            Some(json!({
                "name": "nope",
                "vcpus": 1,
                "memory_mib": 512
            })),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = send(&app, Method::GET, "/v1/users", Some(token), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    struct LiveNode {
        _dir: tempfile::TempDir,
        url: String,
        serve: tokio::task::JoinHandle<()>,
        tick: tokio::task::JoinHandle<()>,
        token: String,
    }

    impl LiveNode {
        fn kill(&self) {
            self.serve.abort();
            self.tick.abort();
        }

        async fn json(&self, method: Method, path: &str, body: Option<serde_json::Value>) -> serde_json::Value {
            let mut req = self.http().request(method, format!("{}{path}", self.url));
            req = req.header("authorization", format!("Bearer {}", self.token));
            if let Some(body) = body {
                req = req.json(&body);
            }
            req.send().await.unwrap().json().await.unwrap()
        }

        fn http(&self) -> reqwest::Client {
            reqwest::Client::new()
        }
    }

    async fn spawn_node(name: &str) -> LiveNode {
        let dir = tempfile::tempdir().unwrap();
        let mut config = HostConfig::default_for(dir.path());
        config.cluster.node_name = Some(name.into());
        config.cluster.heartbeat_ms = 100;
        config.cluster.offline_after_ms = 400;
        config.cluster.cpus = Some(8);
        config.cluster.memory_mib = Some(16_384);
        let store = Store::open(dir.path().join("vms.json")).unwrap();
        let volumes = VolumePool::open(dir.path().join("storage"), None).unwrap();
        let networks = NetworkPool::open(dir.path().join("net"), false).unwrap();
        let control = ControlStore::open(dir.path().join("control.db"), Some("admin")).unwrap();
        let vmm = VmmBackend::from_config(DriverKind::Mock, None, dir.path().join("run")).unwrap();
        let svc = Service::new(
            vmm,
            store,
            volumes,
            networks,
            control,
            config,
            dir.path().to_path_buf(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}");
        svc.set_peer_url(url.clone()).unwrap();
        let serve_svc = svc.clone();
        let serve = tokio::spawn(async move {
            axum::serve(listener, router(serve_svc)).await.unwrap();
        });
        let tick_svc = svc.clone();
        let tick = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let _ = tick_svc.cluster_tick().await;
            }
        });
        let client = reqwest::Client::new();
        let login: serde_json::Value = client
            .post(format!("{url}/v1/login"))
            .json(&json!({ "username": "admin", "password": "admin" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        LiveNode {
            _dir: dir,
            url,
            serve,
            tick,
            token: login["token"].as_str().unwrap().to_string(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn three_node_ha_restarts_elsewhere() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        let c = spawn_node("c").await;
        let _ = b
            .json(
                Method::POST,
                "/v1/cluster/join",
                Some(json!({
                    "peer": a.url,
                    "username": "admin",
                    "password": "admin"
                })),
            )
            .await;
        let _ = c
            .json(
                Method::POST,
                "/v1/cluster/join",
                Some(json!({
                    "peer": a.url,
                    "username": "admin",
                    "password": "admin"
                })),
            )
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let cluster = a.json(Method::GET, "/v1/cluster", None).await;
        assert!(cluster["quorum"].as_bool().unwrap());
        assert!(cluster["members"].as_array().unwrap().len() >= 3);

        let created = a
            .json(
                Method::POST,
                "/v1/vms",
                Some(json!({
                    "name": "ha-demo",
                    "vcpus": 1,
                    "memory_mib": 512
                })),
            )
            .await;
        let id = created["id"].as_str().unwrap();
        let started = a
            .json(Method::POST, &format!("/v1/vms/{id}/start"), None)
            .await;
        assert_eq!(started["state"], "running");
        let owner = started["node_id"].as_str().unwrap().to_string();
        let cluster = a.json(Method::GET, "/v1/cluster", None).await;
        let owner_name = cluster["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == owner)
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
        match owner_name.as_str() {
            "a" => a.kill(),
            "b" => b.kill(),
            _ => c.kill(),
        }
        let survivor = if owner_name == "a" { &b } else { &a };
        let mut found = false;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let vms = survivor.json(Method::GET, "/v1/vms", None).await;
            if let Some(vm) = vms.as_array().and_then(|list| {
                list.iter().find(|vm| vm["id"] == id && vm["state"] == "running")
            }) && vm["node_id"].as_str() != Some(owner.as_str())
            {
                found = true;
                break;
            }
        }
        a.kill();
        b.kill();
        c.kill();
        assert!(found, "ha did not restart the vm on a surviving node");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn replicated_volume_ha_prefers_replica_holder() {
        let a = spawn_node("a").await;
        let b = spawn_node("b").await;
        let c = spawn_node("c").await;
        let _ = b
            .json(
                Method::POST,
                "/v1/cluster/join",
                Some(json!({
                    "peer": a.url,
                    "username": "admin",
                    "password": "admin"
                })),
            )
            .await;
        let _ = c
            .json(
                Method::POST,
                "/v1/cluster/join",
                Some(json!({
                    "peer": a.url,
                    "username": "admin",
                    "password": "admin"
                })),
            )
            .await;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let cluster = a.json(Method::GET, "/v1/cluster", None).await;
            if cluster["quorum"].as_bool() == Some(true)
                && cluster["members"].as_array().map(|m| m.len()).unwrap_or(0) >= 3
            {
                break;
            }
        }

        let vol = a
            .json(
                Method::POST,
                "/v1/volumes",
                Some(json!({
                    "name": "shared",
                    "size_bytes": 8_388_608,
                    "replicas": 2
                })),
            )
            .await;
        let vol_id = vol["id"].as_str().unwrap().to_string();
        let mut replicas = vol["replicas"].as_array().cloned().unwrap_or_default();
        for _ in 0..20 {
            if replicas.len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let vol = a
                .json(Method::GET, &format!("/v1/volumes/{vol_id}"), None)
                .await;
            replicas = vol["replicas"].as_array().cloned().unwrap_or_default();
        }
        assert!(
            replicas.len() >= 2,
            "expected two replica nodes, got {replicas:?}"
        );

        let created = a
            .json(
                Method::POST,
                "/v1/vms",
                Some(json!({
                    "name": "ha-disk",
                    "vcpus": 1,
                    "memory_mib": 512
                })),
            )
            .await;
        let id = created["id"].as_str().unwrap();
        a.json(
            Method::POST,
            &format!("/v1/vms/{id}/disks"),
            Some(json!({
                "volume_id": vol_id,
                "boot": true
            })),
        )
        .await;
        let started = a
            .json(Method::POST, &format!("/v1/vms/{id}/start"), None)
            .await;
        assert_eq!(started["state"], "running");
        let owner = started["node_id"].as_str().unwrap().to_string();
        assert!(
            replicas.iter().any(|r| r.as_str() == Some(owner.as_str())),
            "vm started on a node without the volume replica"
        );

        let cluster = a.json(Method::GET, "/v1/cluster", None).await;
        let owner_name = cluster["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == owner)
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
        match owner_name.as_str() {
            "a" => a.kill(),
            "b" => b.kill(),
            _ => c.kill(),
        }
        let survivor = if owner_name == "a" { &b } else { &a };
        let mut dest = None;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let vms = survivor.json(Method::GET, "/v1/vms", None).await;
            if let Some(vm) = vms.as_array().and_then(|list| {
                list.iter().find(|vm| vm["id"] == id && vm["state"] == "running")
            }) && vm["node_id"].as_str() != Some(owner.as_str())
            {
                dest = vm["node_id"].as_str().map(|s| s.to_string());
                break;
            }
        }
        a.kill();
        b.kill();
        c.kill();
        let dest = dest.expect("ha did not restart the vm");
        assert!(
            replicas.iter().any(|r| r.as_str() == Some(dest.as_str())),
            "ha restarted on {dest} which did not already hold a replica"
        );
    }
}
