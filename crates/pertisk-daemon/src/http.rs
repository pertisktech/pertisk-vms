use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::extract::{
    DefaultBodyLimit, Path, Query, State,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json};
use futures_util::StreamExt;
use pertisk_api::{
    ChangePasswordRequest, CreateUserRequest, CreateVmRequest, LoginRequest, Role,
    SetPasswordRequest, openapi_json,
};
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, AttachNicRequest, CloneVolumeRequest, CloudInitIsoRequest,
    ClusterSnapshot, ConsoleInput, CreateNetworkRequest, CreateVolumeRequest, HeartbeatMessage,
    ImportIsoRequest, JoinClusterRequest, MigrateRequest, NodeRecord, ResizeVolumeRequest,
    SnapshotRequest, UpdateVmRequest, VmId, VmRecord, VolumeFormat, VolumeId, VolumeRecord,
};
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::control::AuthUser;
use crate::static_files::static_handler;
use crate::{DaemonError, Service};

pub fn router(service: Service) -> Router {
    let protected = Router::new()
        .route("/v1/session", get(session))
        .route("/v1/session/password", post(change_own_password))
        .route("/v1/host", get(host))
        .route("/v1/vms", get(list).post(create))
        .route("/v1/vms/{id}", get(show).patch(update_vm).delete(destroy))
        .route("/v1/vms/{id}/start", post(start))
        .route("/v1/vms/{id}/stop", post(stop))
        .route("/v1/vms/{id}/shutdown", post(shutdown))
        .route("/v1/vms/{id}/restart", post(restart))
        .route("/v1/vms/{id}/migrate", post(migrate))
        .route("/v1/vms/{id}/disks", post(attach_disk))
        .route(
            "/v1/vms/{id}/disks/{volume_id}",
            axum::routing::delete(detach_disk),
        )
        .route("/v1/vms/{id}/cdrom", post(attach_iso))
        .route(
            "/v1/vms/{id}/cdrom/{iso}",
            axum::routing::delete(detach_iso),
        )
        .route("/v1/vms/{id}/nics", post(attach_nic))
        .route("/v1/vms/{id}/nics/{tap}", axum::routing::delete(detach_nic))
        .route("/v1/vms/{id}/console", get(console_info))
        .route("/v1/vms/{id}/console/serial", get(console_serial))
        .route("/v1/vms/{id}/console/input", post(console_input))
        .route("/v1/vms/{id}/console/ws", get(console_ws))
        .route("/v1/vms/{id}/graphics/ws", get(graphics_ws))
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
        .route("/v1/isos/cloud-init", post(create_cloudinit_iso))
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
        .route("/v1/users/{id}/password", post(set_user_password))
        .route("/v1/cluster", get(cluster_status))
        .route("/v1/cluster/join", post(cluster_join))
        .route("/v1/cluster/leave", post(cluster_leave))
        .route("/v1/cluster/accept", post(cluster_accept))
        .route("/v1/peer/heartbeat", post(peer_heartbeat))
        .route("/v1/peer/snapshot", post(peer_snapshot))
        .route("/v1/peer/accept", post(peer_accept))
        .route("/v1/peer/run", post(peer_run))
        .route("/v1/peer/stop", post(peer_stop))
        .route("/v1/peer/shutdown", post(peer_shutdown))
        .route("/v1/peer/restart", post(peer_restart))
        .route("/v1/peer/drop", post(peer_drop))
        .route("/v1/peer/volumes/ensure", post(peer_volume_ensure))
        .route(
            "/v1/peer/volumes/{id}",
            axum::routing::delete(peer_volume_delete),
        )
        .route("/v1/peer/volumes/{id}/stat", get(peer_volume_stat))
        .route(
            "/v1/peer/volumes/{id}/blob",
            get(peer_volume_blob_get).put(peer_volume_blob_put),
        )
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024));

    let large_upload = Router::new()
        .route("/v1/isos/upload", post(upload_iso))
        .route("/v1/volumes/import", post(upload_volume_import))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024 * 1024));

    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/login", post(login))
        .route("/v1/openapi.json", get(openapi))
        .merge(protected)
        .merge(large_upload)
        .fallback(static_handler)
        .with_state(service)
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
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

async fn change_own_password(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    let token = bearer_token(&headers).ok_or(crate::control::ControlError::Unauthorized)?;
    service.change_own_password(&user, &req.current_password, &req.new_password, &token)?;
    Ok(Json(json!({ "ok": true })))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn query_token(uri: &axum::http::Uri) -> Option<String> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        let value = parts.next().unwrap_or("");
        if key == "token" && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn request_token(headers: &HeaderMap, uri: &axum::http::Uri) -> Option<String> {
    bearer_token(headers).or_else(|| query_token(uri))
}

fn required_role(method: &Method, path: &str) -> Role {
    if path == "/v1/session/password" {
        Role::Viewer
    } else if path.starts_with("/v1/users") {
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
    let token = request_token(req.headers(), req.uri())
        .ok_or(crate::control::ControlError::Unauthorized)?;
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
    Json(req): Json<CreateVmRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    let id = req.id.unwrap_or_default();
    let record = tracked(
        &service,
        &user,
        "vm.create",
        req.spec.name.clone(),
        service.create(id, req.spec),
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

async fn update_vm(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
    Json(req): Json<UpdateVmRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(
            &service,
            &user,
            "vm.update",
            id.to_string(),
            service.update(id, req),
        )
        .await?,
    ))
}

async fn start(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(
            &service,
            &user,
            "vm.start",
            id.to_string(),
            service.start(id),
        )
        .await?,
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

async fn shutdown(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(
            &service,
            &user,
            "vm.shutdown",
            id.to_string(),
            service.shutdown(id),
        )
        .await?,
    ))
}

async fn restart(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(
            &service,
            &user,
            "vm.restart",
            id.to_string(),
            service.restart(id),
        )
        .await?,
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
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CreateVolumeRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    let name = req.name.clone();
    Ok((
        StatusCode::CREATED,
        Json(
            tracked(
                &service,
                &user,
                "volume.create",
                name,
                service.create_volume(req),
            )
            .await?,
        ),
    ))
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

#[derive(Deserialize)]
struct VolumeImportQuery {
    name: Option<String>,
    format: Option<String>,
}

async fn upload_volume_import(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<VolumeImportQuery>,
    body: Body,
) -> Result<impl IntoResponse, DaemonError> {
    let format = match q.format.as_deref().unwrap_or("qcow2") {
        "qcow2" | "QCOW2" => VolumeFormat::Qcow2,
        "raw" | "RAW" => VolumeFormat::Raw,
        other => {
            return Err(pertisk_storage::StorageError::Message(format!(
                "unknown volume format '{other}' (raw | qcow2)"
            ))
            .into());
        }
    };
    let name = q
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| pertisk_storage::StorageError::Message("query name is required".into()))?;
    let ext = format.extension();
    let tmp = std::env::temp_dir().join(format!("pertisk-vol-{}.{ext}", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(pertisk_storage::StorageError::Io)?;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    const MAX: u64 = 8 * 1024 * 1024 * 1024;
    let write = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|err| std::io::Error::other(err.to_string()))?;
            written += chunk.len() as u64;
            if written > MAX {
                return Err(pertisk_storage::StorageError::Message(
                    "volume larger than 8GiB".into(),
                )
                .into());
            }
            file.write_all(&chunk)
                .await
                .map_err(pertisk_storage::StorageError::Io)?;
        }
        file.flush()
            .await
            .map_err(pertisk_storage::StorageError::Io)?;
        Ok::<(), DaemonError>(())
    }
    .await;
    if let Err(err) = write {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(err);
    }
    drop(file);
    if written == 0 {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(pertisk_storage::StorageError::Message("empty volume upload".into()).into());
    }
    let result = tracked(&service, &user, "volume.import", name.clone(), async {
        service.import_volume(name, format, tmp.clone()).await
    })
    .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    Ok((StatusCode::CREATED, Json(result?)))
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
    Extension(user): Extension<AuthUser>,
    Json(req): Json<ImportIsoRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    let target = req
        .name
        .clone()
        .unwrap_or_else(|| req.path.display().to_string());
    Ok((
        StatusCode::CREATED,
        Json(
            tracked(&service, &user, "iso.import", target, async {
                service.import_iso(req)
            })
            .await?,
        ),
    ))
}

#[derive(Deserialize)]
struct IsoUploadQuery {
    name: Option<String>,
}

async fn upload_iso(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<IsoUploadQuery>,
    body: Body,
) -> Result<impl IntoResponse, DaemonError> {
    let tmp = std::env::temp_dir().join(format!("pertisk-iso-{}.iso", uuid::Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(pertisk_storage::StorageError::Io)?;
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    const MAX: u64 = 8 * 1024 * 1024 * 1024;
    let write = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|err| std::io::Error::other(err.to_string()))?;
            written += chunk.len() as u64;
            if written > MAX {
                return Err(
                    pertisk_storage::StorageError::Message("iso larger than 8GiB".into()).into(),
                );
            }
            file.write_all(&chunk)
                .await
                .map_err(pertisk_storage::StorageError::Io)?;
        }
        file.flush()
            .await
            .map_err(pertisk_storage::StorageError::Io)?;
        Ok::<(), DaemonError>(())
    }
    .await;
    if let Err(err) = write {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(err);
    }
    drop(file);
    if written == 0 {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(pertisk_storage::StorageError::Message("empty iso upload".into()).into());
    }
    let target = q.name.clone().unwrap_or_else(|| "upload.iso".into());
    let result = tracked(&service, &user, "iso.import", target, async {
        service.import_iso(ImportIsoRequest {
            path: tmp.clone(),
            name: q.name.clone(),
        })
    })
    .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    Ok((StatusCode::CREATED, Json(result?)))
}

async fn create_cloudinit_iso(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<CloudInitIsoRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    let target = req.name.clone();
    Ok((
        StatusCode::CREATED,
        Json(
            tracked(&service, &user, "iso.cloud-init", target, async {
                service.create_cloudinit_iso(req)
            })
            .await?,
        ),
    ))
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
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
    Json(req): Json<AttachDiskRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(&service, &user, "vm.attach-disk", id.to_string(), async {
            service.attach_disk(id, req)
        })
        .await?,
    ))
}

async fn detach_disk(
    State(service): State<Service>,
    Path((id, volume_id)): Path<(VmId, VolumeId)>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.detach_disk(id, volume_id)?))
}

async fn attach_iso(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
    Json(req): Json<AttachIsoRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(&service, &user, "vm.attach-iso", id.to_string(), async {
            service.attach_iso(id, req)
        })
        .await?,
    ))
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
    Extension(user): Extension<AuthUser>,
    Path(id): Path<VmId>,
    Json(req): Json<AttachNicRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(
        tracked(&service, &user, "vm.attach-nic", id.to_string(), async {
            service.attach_nic(id, req)
        })
        .await?,
    ))
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

async fn console_input(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    Json(req): Json<ConsoleInput>,
) -> Result<impl IntoResponse, DaemonError> {
    service.write_console(id, &req.text).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn console_ws(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, DaemonError> {
    let backlog = service.console_serial(id, 0, 256 * 1024)?;
    let (rx, tx) = service.subscribe_console(id).await?;
    Ok(ws.on_upgrade(move |socket| proxy_console(socket, backlog.text, rx, tx)))
}

async fn proxy_console(
    mut socket: WebSocket,
    backlog: String,
    mut rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
) {
    if !backlog.is_empty() && socket.send(Message::Text(backlog.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if tx.send(text.as_bytes().to_vec()).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if tx.send(bytes.to_vec()).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if socket.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            chunk = rx.recv() => {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        }
    }
}

async fn graphics_ws(
    State(service): State<Service>,
    Path(id): Path<VmId>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, DaemonError> {
    let console_info = service.console_info(id)?;
    let graphics_socket = console_info
        .graphics_socket
        .ok_or_else(|| DaemonError::Peer("VM has no graphics console".into()))?;
    Ok(ws.on_upgrade(move |socket| proxy_graphics(socket, graphics_socket)))
}

async fn proxy_graphics(mut ws: WebSocket, graphics_socket: std::path::PathBuf) {
    use futures_util::SinkExt;
    use tokio::net::UnixStream;

    let unix_socket = match UnixStream::connect(&graphics_socket).await {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(path = %graphics_socket.display(), %err, "graphics socket connect failed");
            let _ = ws.close().await;
            return;
        }
    };

    let (unix_read, unix_write) = unix_socket.into_split();
    let (mut ws_send, mut ws_recv) = ws.split();

    // Forward Unix socket → WebSocket (binary)
    let unix_to_ws = tokio::spawn(async move {
        let mut unix_read = unix_read;
        let mut buf = vec![0u8; 65536];
        loop {
            match unix_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let data = buf[..n].to_vec();
                    if ws_send.send(Message::Binary(data.into())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Forward WebSocket → Unix socket
    let mut unix_write = unix_write;
    while let Some(msg) = ws_recv.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                if unix_write.write_all(&data).await.is_err() {
                    break;
                }
            }
            Ok(Message::Text(text)) => {
                if unix_write.write_all(text.as_bytes()).await.is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            _ => {}
        }
    }

    unix_to_ws.abort();
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

async fn set_user_password(
    State(service): State<Service>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<String>,
    Json(req): Json<SetPasswordRequest>,
) -> Result<impl IntoResponse, DaemonError> {
    if id == user.id {
        return Err(crate::control::ControlError::Message(
            "use Change password in the user menu (current password required)".into(),
        )
        .into());
    }
    service.set_user_password(&id, &req.new_password)?;
    let _ = service.audit(&user.username, "user.password", Some(&id));
    Ok(Json(json!({ "ok": true })))
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

async fn peer_shutdown(
    State(service): State<Service>,
    Json(record): Json<VmRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_shutdown(record).await?))
}

async fn peer_restart(
    State(service): State<Service>,
    Json(record): Json<VmRecord>,
) -> Result<impl IntoResponse, DaemonError> {
    Ok(Json(service.apply_restart(record).await?))
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
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], data))
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
            Self::NoQuorum | Self::Fenced | Self::Unschedulable(_) | Self::Capacity(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
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
        Io(err) if err.raw_os_error() == Some(28) => StatusCode::INSUFFICIENT_STORAGE,
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
        let vmm =
            VmmBackend::from_config(DriverKind::Mock, None, dir.path().join("run"), None).unwrap();
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
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) }))
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
        assert!(body["version"].as_str().unwrap().contains('.'));
        let (status, body) = send(&app, Method::GET, "/v1/openapi.json", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["openapi"], "3.0.3");
        let (status, _) = send(&app, Method::GET, "/", None, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn https_health_accepts_self_signed() {
        let (svc, dir) = service();
        let cert = dir.path().join("tls/cert.pem");
        let key = dir.path().join("tls/key.pem");
        crate::tls::ensure_self_signed(&cert, &key).unwrap();
        crate::install_rustls_provider();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let rustls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key)
            .await
            .expect("load pem");
        let app = router(svc);
        tokio::spawn(async move {
            axum_server::from_tcp_rustls(listener, rustls)
                .expect("tls listener")
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        let mut last_err = None;
        for _ in 0..40 {
            match client
                .get(format!("https://127.0.0.1:{}/v1/health", addr.port()))
                .send()
                .await
            {
                Ok(res) => {
                    assert_eq!(res.status(), reqwest::StatusCode::OK);
                    return;
                }
                Err(err) => {
                    last_err = Some(err);
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            }
        }
        panic!("https health failed: {last_err:?}");
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
        assert!(host["version"].as_str().unwrap().contains('.'));
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
    async fn iso_upload_from_bytes() {
        let (svc, _dir) = service();
        let app = router(svc);
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
        let req = Request::builder()
            .method(Method::POST)
            .uri("/v1/isos/upload?name=tiny.iso")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(&b"iso-bytes"[..]))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let (status, isos) = send(&app, Method::GET, "/v1/isos", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(isos[0]["name"], "tiny.iso");
        assert_eq!(isos[0]["size_bytes"], 9);
        let (status, tasks) = send(&app, Method::GET, "/v1/tasks", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            tasks
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["kind"] == "iso.import"),
            "missing iso.import task: {tasks}"
        );
    }

    #[tokio::test]
    async fn cloudinit_iso_via_http() {
        let (svc, _dir) = service();
        let app = router(svc);
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
        let (status, iso) = send(
            &app,
            Method::POST,
            "/v1/isos/cloud-init",
            Some(token),
            Some(json!({
                "name": "web",
                "hostname": "web-1",
                "user": "ubuntu",
                "password": "ubuntu"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(iso["name"], "web-cidata.iso");
        assert!(iso["size_bytes"].as_u64().unwrap() > 2048);
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

    #[tokio::test]
    async fn change_own_password() {
        let (svc, _dir) = service();
        let app = router(svc);
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
        let (status, body) = send(
            &app,
            Method::POST,
            "/v1/session/password",
            Some(token),
            Some(json!({
                "current_password": "admin",
                "new_password": "newpass"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, _) = send(
            &app,
            Method::POST,
            "/v1/login",
            None,
            Some(json!({ "username": "admin", "password": "admin" })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = send(
            &app,
            Method::POST,
            "/v1/login",
            None,
            Some(json!({ "username": "admin", "password": "newpass" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&app, Method::GET, "/v1/session", Some(token), None).await;
        assert_eq!(status, StatusCode::OK);
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

        async fn json(
            &self,
            method: Method,
            path: &str,
            body: Option<serde_json::Value>,
        ) -> serde_json::Value {
            self.json_opt(method, path, body)
                .await
                .expect("cluster http")
        }

        async fn json_opt(
            &self,
            method: Method,
            path: &str,
            body: Option<serde_json::Value>,
        ) -> Option<serde_json::Value> {
            let mut req = self.http().request(method, format!("{}{path}", self.url));
            req = req.header("authorization", format!("Bearer {}", self.token));
            if let Some(body) = body {
                req = req.json(&body);
            }
            req.send().await.ok()?.json().await.ok()
        }

        fn http(&self) -> reqwest::Client {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        }
    }

    async fn wait_members(nodes: &[&LiveNode], want: usize) {
        for _ in 0..80 {
            for node in nodes {
                if let Some(cluster) = node.json_opt(Method::GET, "/v1/cluster", None).await
                    && cluster["quorum"].as_bool() == Some(true)
                    && cluster["members"].as_array().map(|m| m.len()).unwrap_or(0) >= want
                {
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    async fn wait_running_elsewhere(nodes: &[&LiveNode], id: &str, owner: &str) -> Option<String> {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            for node in nodes {
                let Some(vms) = node.json_opt(Method::GET, "/v1/vms", None).await else {
                    continue;
                };
                if let Some(vm) = vms.as_array().and_then(|list| {
                    list.iter()
                        .find(|vm| vm["id"] == id && vm["state"] == "running")
                }) && vm["node_id"].as_str() != Some(owner)
                {
                    return vm["node_id"].as_str().map(str::to_string);
                }
            }
        }
        None
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
        let vmm =
            VmmBackend::from_config(DriverKind::Mock, None, dir.path().join("run"), None).unwrap();
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
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
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
        wait_members(&[&a, &b, &c], 3).await;
        let cluster = a.json(Method::GET, "/v1/cluster", None).await;
        assert!(
            cluster["members"].as_array().map(|m| m.len()).unwrap_or(0) >= 3,
            "cluster did not reach 3 members: {cluster}"
        );

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
        let survivors: Vec<&LiveNode> = match owner_name.as_str() {
            "a" => vec![&b, &c],
            "b" => vec![&a, &c],
            _ => vec![&a, &b],
        };
        let found = wait_running_elsewhere(&survivors, id, &owner)
            .await
            .is_some();
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
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
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
        wait_members(&[&a, &b, &c], 3).await;

        let vol = a
            .json(
                Method::POST,
                "/v1/volumes",
                Some(json!({
                    "name": "shared",
                    "size_bytes": 65536,
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
        let survivors: Vec<&LiveNode> = match owner_name.as_str() {
            "a" => vec![&b, &c],
            "b" => vec![&a, &c],
            _ => vec![&a, &b],
        };
        let dest = wait_running_elsewhere(&survivors, id, &owner).await;
        a.kill();
        b.kill();
        c.kill();
        let dest = dest.expect("ha did not restart the vm");
        assert!(
            replicas.iter().any(|r| r.as_str() == Some(dest.as_str())),
            "ha restarted on {dest} which did not already hold a replica"
        );
    }

    #[tokio::test]
    async fn console_input_via_http() {
        let (svc, _dir) = service();
        let app = router(svc);
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
        let (status, created) = send(
            &app,
            Method::POST,
            "/v1/vms",
            Some(token),
            Some(json!({
                "name": "cons",
                "vcpus": 1,
                "memory_mib": 512
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["id"].as_str().unwrap();
        let (status, started) = send(
            &app,
            Method::POST,
            &format!("/v1/vms/{id}/start"),
            Some(token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(started["state"], "running");
        let (status, info) = send(
            &app,
            Method::GET,
            &format!("/v1/host?token={token}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(info["driver"].is_string());
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/v1/vms/{id}/console/input"),
            Some(token),
            Some(json!({ "text": "ping\n" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let mut text = String::new();
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let (status, chunk) = send(
                &app,
                Method::GET,
                &format!("/v1/vms/{id}/console/serial?from=0&max=8192"),
                Some(token),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            text = chunk["text"].as_str().unwrap_or("").to_string();
            if text.contains("started") && text.contains("ping") {
                break;
            }
        }
        assert!(text.contains("started"), "missing boot serial: {text:?}");
        assert!(text.contains("ping"), "missing console input: {text:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn console_websocket_roundtrip() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::connect_async;

        let node = spawn_node("console").await;
        let created = node
            .json(
                Method::POST,
                "/v1/vms",
                Some(json!({
                    "name": "cons",
                    "vcpus": 1,
                    "memory_mib": 512
                })),
            )
            .await;
        let id = created["id"].as_str().unwrap();
        let started = node
            .json(Method::POST, &format!("/v1/vms/{id}/start"), None)
            .await;
        assert_eq!(started["state"], "running");
        let ws_url = format!(
            "{}/v1/vms/{id}/console/ws?token={}",
            node.url.replacen("http://", "ws://", 1),
            node.token
        );
        let (ws, _) = connect_async(&ws_url).await.expect("ws connect");
        let (mut sink, mut stream) = ws.split();
        let mut got = String::new();
        for _ in 0..20 {
            match tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await {
                Ok(Some(Ok(msg))) => {
                    got.push_str(&msg.to_string());
                    if got.contains("started") {
                        break;
                    }
                }
                _ => {}
            }
        }
        assert!(
            got.contains("started"),
            "ws backlog missing boot log: {got:?}"
        );
        sink.send(tokio_tungstenite::tungstenite::Message::Text(
            "ping\n".into(),
        ))
        .await
        .unwrap();
        for _ in 0..20 {
            match tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await {
                Ok(Some(Ok(msg))) => {
                    got.push_str(&msg.to_string());
                    if got.contains("ping") {
                        break;
                    }
                }
                _ => {}
            }
        }
        node.kill();
        assert!(got.contains("ping"), "ws did not echo input: {got:?}");
    }
}
