//! Node daemon: persisted inventory and VMM lifecycle.

mod cluster;
mod console;
mod control;
mod http;
mod service;
mod static_files;
mod store;
mod tls;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum_server::tls_rustls::RustlsConfig;
use pertisk_types::{HostConfig, default_home};

pub use control::{AuthUser, ControlStore};
pub use http::router;
pub use service::{DaemonError, Service};
pub use store::Store;
pub use tls::{TlsBind, tls_bind};

use crate::cluster::advertise_url;

fn install_rustls_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

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

pub async fn bind_and_serve(
    listen: &str,
    tls: Option<TlsBind>,
    service: Service,
) -> Result<(), DaemonError> {
    install_rustls_provider();
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let addr = listener.local_addr()?;
    let _ = service.set_peer_url(advertise_url(&addr.to_string(), None));
    tracing::info!(%listen, bound = %addr, driver = %service.driver(), "pertiskd listening");

    let https_addr = if let Some(tls) = &tls {
        tls::ensure_self_signed(&tls.cert, &tls.key)?;
        let https_addr: SocketAddr = tls
            .listen
            .parse()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        tracing::info!(
            listen = %tls.listen,
            cert = %tls.cert.display(),
            "pertiskd https (http/1.1 + http/2)"
        );
        Some((https_addr, tls.cert.clone(), tls.key.clone()))
    } else {
        None
    };

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

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let shutdown = service.clone();
    tokio::spawn(async move {
        shutdown_signal(shutdown).await;
        let _ = stop_tx.send(true);
    });

    let app = router(service);
    let http = {
        let mut rx = stop_rx.clone();
        axum::serve(listener, app.clone()).with_graceful_shutdown(async move {
            let _ = rx.wait_for(|stop| *stop).await;
        })
    };

    if let Some((https_addr, cert, key)) = https_addr {
        let rustls = RustlsConfig::from_pem_file(&cert, &key)
            .await
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        let handle = axum_server::Handle::new();
        let https_handle = handle.clone();
        let mut rx = stop_rx;
        tokio::spawn(async move {
            let _ = rx.wait_for(|stop| *stop).await;
            https_handle.graceful_shutdown(Some(Duration::from_secs(5)));
        });
        let https = axum_server::bind_rustls(https_addr, rustls)
            .handle(handle)
            .serve(app.into_make_service());
        tokio::select! {
            result = http => result?,
            result = https => result?,
        }
    } else {
        http.await?;
    }
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
