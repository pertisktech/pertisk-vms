use std::path::PathBuf;

use clap::Parser;
use pertisk_daemon::{
    ControlStore, Service, Store, bind_and_serve, home_dir, load_or_init_config, tls_bind,
};
use pertisk_net::NetworkPool;
use pertisk_storage::VolumePool;
use pertisk_vmm::VmmBackend;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "pertiskd", about = "pertisk-vm node daemon")]
struct Args {
    /// Override PERTISK_HOME (~/.pertisk by default).
    #[arg(long, env = "PERTISK_HOME")]
    home: Option<PathBuf>,
    /// Override VMM driver (mock | cloud-hypervisor | qemu).
    #[arg(long, env = "PERTISK_DRIVER")]
    driver: Option<pertisk_types::DriverKind>,
    /// rust-hypervisor-firmware path for disk/ISO boot.
    #[arg(long, env = "PERTISK_FIRMWARE")]
    firmware: Option<PathBuf>,
    /// Listen address.
    #[arg(long, env = "PERTISK_LISTEN")]
    listen: Option<String>,
    /// HTTPS listen address (empty / off disables TLS).
    #[arg(long, env = "PERTISK_TLS_LISTEN")]
    tls_listen: Option<String>,
    /// PEM certificate for HTTPS (default $PERTISK_HOME/tls/cert.pem).
    #[arg(long, env = "PERTISK_TLS_CERT")]
    tls_cert: Option<PathBuf>,
    /// PEM private key for HTTPS (default $PERTISK_HOME/tls/key.pem).
    #[arg(long, env = "PERTISK_TLS_KEY")]
    tls_key: Option<PathBuf>,
    /// Join an existing cluster peer URL on startup.
    #[arg(long, env = "PERTISK_JOIN")]
    join: Option<String>,
    /// Cluster node name.
    #[arg(long, env = "PERTISK_NODE_NAME")]
    node_name: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("pertisk_daemon=info,pertisk_vmm=info")),
        )
        .init();

    let args = Args::parse();
    let home = args.home.unwrap_or_else(home_dir);
    let (mut config, config_path) = load_or_init_config(&home)?;
    if let Some(driver) = args.driver {
        config.vmm.driver = driver;
    }
    if let Some(firmware) = args.firmware {
        config.vmm.firmware = Some(firmware);
    }
    if let Some(listen) = args.listen {
        config.daemon.listen = listen;
    }
    if let Some(tls_listen) = args.tls_listen {
        config.daemon.tls_listen = Some(tls_listen);
    }
    if let Some(tls_cert) = args.tls_cert {
        config.daemon.tls_cert = Some(tls_cert);
    }
    if let Some(tls_key) = args.tls_key {
        config.daemon.tls_key = Some(tls_key);
    }
    if let Some(join) = args.join {
        config.cluster.join = Some(join);
    }
    if let Some(node_name) = args.node_name {
        config.cluster.node_name = Some(node_name);
    }

    tracing::info!(home = %home.display(), config = %config_path.display(), "starting pertiskd");

    let store = Store::open(home.join("state/vms.json"))?;
    let volumes = VolumePool::open(config.storage.root.clone(), config.storage.qemu_img.clone())?;
    let networks = NetworkPool::open(home.join("state"), config.network.apply_host_links)?;
    let admin_password = std::env::var("PERTISK_ADMIN_PASSWORD").ok();
    let control = ControlStore::open(home.join("state/control.db"), admin_password.as_deref())?;
    let vmm = VmmBackend::from_config(
        config.vmm.driver,
        config.vmm.cloud_hypervisor.clone(),
        config.vmm.run_dir.clone(),
        config.vmm.firmware.clone(),
    )?;
    let listen = config.daemon.listen.clone();
    let tls = tls_bind(&home, &config.daemon);
    let service = Service::new(vmm, store, volumes, networks, control, config, home);
    bind_and_serve(&listen, tls, service).await?;
    Ok(())
}
