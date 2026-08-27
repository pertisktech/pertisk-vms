//! Node daemon: persisted inventory and VMM lifecycle.

mod http;
mod service;
mod store;

use std::path::{Path, PathBuf};

use pertisk_types::{HostConfig, default_home};

pub use http::router;
pub use service::{DaemonError, Service};
pub use store::Store;

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
    service: Service,
) -> Result<(), DaemonError> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, driver = %service.driver(), "pertiskd listening");
    axum::serve(listener, router(service))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
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
}
