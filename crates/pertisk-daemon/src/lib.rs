//! Node daemon: persisted inventory and VMM lifecycle.

mod cluster;
mod console;
mod control;
mod http;
mod service;
mod static_files;
mod store;

use std::path::{Path, PathBuf};

use pertisk_types::{HostConfig, default_home};

pub use control::{AuthUser, ControlStore};
pub use http::router;
pub use service::{DaemonError, Service};
pub use store::Store;

use crate::cluster::advertise_url;

pub fn home_dir() -> PathBuf {
    default_home()
}

pub fn load_or_init_config(home: &Path) -> Result<(HostConfig, PathBuf), DaemonError> {
    std::fs::create_dir_all(home)?;
    let path = home.join("config.toml");
    let mut config = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text)?
    } else {
        let config = HostConfig::default_for(home);
        std::fs::write(&path, toml::to_string_pretty(&config)?)?;
        config
    };
    config.resolve_paths(home);
    Ok((config, path))
}

pub async fn bind_and_serve(listen: &str, service: Service) -> Result<(), DaemonError> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let addr = listener.local_addr()?;
    let _ = service.set_peer_url(advertise_url(&addr.to_string(), None));
    tracing::info!(%listen, bound = %addr, driver = %service.driver(), "pertiskd listening");
    let ticker = service.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(ticker.heartbeat_period()).await;
            if let Err(err) = ticker.cluster_tick().await {
                tracing::warn!(error = %err, "cluster tick");
            }
        }
    });
    let recon = service.clone();
    tokio::spawn(async move {
        recon.reconcile_local_vms().await;
    });
    if let Some(peer) = service.join_peer() {
        let joiner = service.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let user = std::env::var("PERTISK_JOIN_USER").unwrap_or_else(|_| "admin".into());
            let pass = std::env::var("PERTISK_ADMIN_PASSWORD").unwrap_or_else(|_| "admin".into());
            match joiner.join_cluster(&peer, &user, &pass).await {
                Ok(status) => tracing::info!(nodes = status.members.len(), "joined cluster"),
                Err(err) => tracing::error!(error = %err, "cluster join failed"),
            }
        });
    }
    let shutdown = service.clone();
    axum::serve(listener, router(service))
        .with_graceful_shutdown(shutdown_signal(shutdown))
        .await?;
    Ok(())
}

async fn shutdown_signal(service: Service) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl+c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("pertiskd stopping; shutting down local guests");
    service.shutdown_all_local_vms().await;
}
