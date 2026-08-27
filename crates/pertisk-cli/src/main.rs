use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, CloneVolumeRequest, CreateVolumeRequest, DEFAULT_LISTEN,
    DiskSpec, HostInfo, ImportIsoRequest, IsoRecord, ResizeVolumeRequest, SnapshotRequest, VmId,
    VmRecord, VmSpec, VolumeFormat, VolumeId, VolumeRecord, format_size, parse_size,
};

#[derive(Debug, Parser)]
#[command(name = "pertisk", about = "Operator CLI for pertisk-vm", version)]
struct Cli {
    #[arg(long, env = "PERTISK_URL", default_value = "http://127.0.0.1:7480")]
    url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show hypervisor host capabilities and daemon status.
    Host,
    /// VM lifecycle and disk attach.
    Vm {
        #[command(subcommand)]
        command: VmCommand,
    },
    /// Local volumes (raw / qcow2).
    Vol {
        #[command(subcommand)]
        command: VolCommand,
    },
    /// ISO library.
    Iso {
        #[command(subcommand)]
        command: IsoCommand,
    },
}

#[derive(Debug, Subcommand)]
enum VmCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 1)]
        cpus: u8,
        #[arg(long, default_value_t = 512)]
        memory: u32,
        #[arg(long)]
        kernel: Option<PathBuf>,
        #[arg(long)]
        cmdline: Option<String>,
        #[arg(long)]
        disk: Vec<PathBuf>,
    },
    Start { id: VmId },
    Stop { id: VmId },
    #[command(name = "rm")]
    Remove { id: VmId },
    List,
    Show { id: VmId },
    Disk {
        #[command(subcommand)]
        command: DiskCommand,
    },
    Cdrom {
        #[command(subcommand)]
        command: CdromCommand,
    },
}

#[derive(Debug, Subcommand)]
enum DiskCommand {
    Attach {
        vm: VmId,
        #[arg(long)]
        volume: VolumeId,
    },
    Detach {
        vm: VmId,
        #[arg(long)]
        volume: VolumeId,
    },
}

#[derive(Debug, Subcommand)]
enum CdromCommand {
    Attach {
        vm: VmId,
        #[arg(long)]
        iso: String,
    },
    Detach {
        vm: VmId,
        #[arg(long)]
        iso: String,
    },
}

#[derive(Debug, Subcommand)]
enum VolCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        size: String,
        #[arg(long, default_value = "raw")]
        format: VolumeFormat,
    },
    List,
    Show { id: VolumeId },
    #[command(name = "rm")]
    Remove { id: VolumeId },
    Resize {
        id: VolumeId,
        #[arg(long)]
        size: String,
    },
    Clone {
        id: VolumeId,
        #[arg(long)]
        name: String,
        #[arg(long)]
        linked: bool,
    },
    Snap {
        id: VolumeId,
        #[arg(long)]
        name: String,
    },
    Restore {
        id: VolumeId,
        #[arg(long)]
        snap: String,
    },
}

#[derive(Debug, Subcommand)]
enum IsoCommand {
    Import {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    List,
    #[command(name = "rm")]
    Remove { name: String },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    match cli.command {
        Command::Host => {
            let info: HostInfo = get_json(&client, &cli.url, "/v1/host").await?;
            println!("os                 {}", info.os);
            println!("arch               {}", info.arch);
            println!("kvm                {}", info.kvm);
            println!("driver             {}", info.driver);
            println!(
                "cloud-hypervisor   {}",
                info.cloud_hypervisor
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "not found".into())
            );
            println!("listen             {}", info.listen);
            println!("data_dir           {}", info.data_dir.display());
            println!("storage            {}", info.storage_root.display());
            println!(
                "qemu-img           {}",
                info.qemu_img
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "not found (raw volumes only)".into())
            );
            if !info.kvm {
                eprintln!(
                    "note: /dev/kvm is missing; this machine can run the mock driver only"
                );
            }
        }
        Command::Vm { command } => match command {
            VmCommand::Create {
                name,
                cpus,
                memory,
                kernel,
                cmdline,
                disk,
            } => {
                let spec = VmSpec {
                    name,
                    vcpus: cpus,
                    memory_mib: memory,
                    kernel,
                    cmdline,
                    initramfs: None,
                    disks: disk
                        .into_iter()
                        .map(|path| DiskSpec {
                            path,
                            readonly: false,
                            cdrom: false,
                            volume_id: None,
                            iso_name: None,
                        })
                        .collect(),
                    nets: vec![],
                    serial_log: None,
                };
                let record: VmRecord = post_json(&client, &cli.url, "/v1/vms", &spec).await?;
                print_vm(&record);
            }
            VmCommand::Start { id } => {
                let record: VmRecord =
                    post_empty(&client, &cli.url, &format!("/v1/vms/{id}/start")).await?;
                print_vm(&record);
            }
            VmCommand::Stop { id } => {
                let record: VmRecord =
                    post_empty(&client, &cli.url, &format!("/v1/vms/{id}/stop")).await?;
                print_vm(&record);
            }
            VmCommand::Remove { id } => {
                delete(&client, &cli.url, &format!("/v1/vms/{id}")).await?;
            }
            VmCommand::List => {
                let vms: Vec<VmRecord> = get_json(&client, &cli.url, "/v1/vms").await?;
                if vms.is_empty() {
                    println!("no vms");
                    return Ok(());
                }
                println!(
                    "{:<38} {:<16} {:<10} {:>4} {:>8} {:>6}",
                    "ID", "NAME", "STATE", "CPU", "MEM", "DISKS"
                );
                for vm in vms {
                    println!(
                        "{:<38} {:<16} {:<10} {:>4} {:>8} {:>6}",
                        vm.id,
                        vm.spec.name,
                        vm.state,
                        vm.spec.vcpus,
                        vm.spec.memory_mib,
                        vm.spec.disks.len()
                    );
                }
            }
            VmCommand::Show { id } => {
                let record: VmRecord = get_json(&client, &cli.url, &format!("/v1/vms/{id}")).await?;
                println!("{}", serde_json::to_string_pretty(&record)?);
            }
            VmCommand::Disk { command } => match command {
                DiskCommand::Attach { vm, volume } => {
                    let record: VmRecord = post_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{vm}/disks"),
                        &AttachDiskRequest { volume_id: volume },
                    )
                    .await?;
                    print_vm(&record);
                }
                DiskCommand::Detach { vm, volume } => {
                    let record: VmRecord =
                        delete_json(&client, &cli.url, &format!("/v1/vms/{vm}/disks/{volume}"))
                            .await?;
                    print_vm(&record);
                }
            },
            VmCommand::Cdrom { command } => match command {
                CdromCommand::Attach { vm, iso } => {
                    let record: VmRecord = post_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{vm}/cdrom"),
                        &AttachIsoRequest { iso },
                    )
                    .await?;
                    print_vm(&record);
                }
                CdromCommand::Detach { vm, iso } => {
                    let record: VmRecord =
                        delete_json(&client, &cli.url, &format!("/v1/vms/{vm}/cdrom/{iso}"))
                            .await?;
                    print_vm(&record);
                }
            },
        },
        Command::Vol { command } => match command {
            VolCommand::Create { name, size, format } => {
                let req = CreateVolumeRequest {
                    name,
                    size_bytes: parse_size(&size)?,
                    format,
                };
                let vol: VolumeRecord = post_json(&client, &cli.url, "/v1/volumes", &req).await?;
                print_vol(&vol);
            }
            VolCommand::List => {
                let vols: Vec<VolumeRecord> = get_json(&client, &cli.url, "/v1/volumes").await?;
                if vols.is_empty() {
                    println!("no volumes");
                    return Ok(());
                }
                println!(
                    "{:<38} {:<16} {:<6} {:>8} {:>8}",
                    "ID", "NAME", "FMT", "SIZE", "SNAPS"
                );
                for vol in vols {
                    println!(
                        "{:<38} {:<16} {:<6} {:>8} {:>8}",
                        vol.id,
                        vol.name,
                        vol.format,
                        format_size(vol.size_bytes),
                        vol.snapshots.len()
                    );
                }
            }
            VolCommand::Show { id } => {
                let vol: VolumeRecord =
                    get_json(&client, &cli.url, &format!("/v1/volumes/{id}")).await?;
                println!("{}", serde_json::to_string_pretty(&vol)?);
            }
            VolCommand::Remove { id } => {
                delete(&client, &cli.url, &format!("/v1/volumes/{id}")).await?;
            }
            VolCommand::Resize { id, size } => {
                let vol: VolumeRecord = post_json(
                    &client,
                    &cli.url,
                    &format!("/v1/volumes/{id}/resize"),
                    &ResizeVolumeRequest {
                        size_bytes: parse_size(&size)?,
                    },
                )
                .await?;
                print_vol(&vol);
            }
            VolCommand::Clone { id, name, linked } => {
                let vol: VolumeRecord = post_json(
                    &client,
                    &cli.url,
                    &format!("/v1/volumes/{id}/clone"),
                    &CloneVolumeRequest { name, linked },
                )
                .await?;
                print_vol(&vol);
            }
            VolCommand::Snap { id, name } => {
                let vol: VolumeRecord = post_json(
                    &client,
                    &cli.url,
                    &format!("/v1/volumes/{id}/snapshots"),
                    &SnapshotRequest { name },
                )
                .await?;
                print_vol(&vol);
            }
            VolCommand::Restore { id, snap } => {
                let vol: VolumeRecord = post_empty(
                    &client,
                    &cli.url,
                    &format!("/v1/volumes/{id}/snapshots/{snap}/restore"),
                )
                .await?;
                print_vol(&vol);
            }
        },
        Command::Iso { command } => match command {
            IsoCommand::Import { path, name } => {
                let iso: IsoRecord = post_json(
                    &client,
                    &cli.url,
                    "/v1/isos",
                    &ImportIsoRequest { path, name },
                )
                .await?;
                println!("{} {}", iso.name, format_size(iso.size_bytes));
            }
            IsoCommand::List => {
                let isos: Vec<IsoRecord> = get_json(&client, &cli.url, "/v1/isos").await?;
                if isos.is_empty() {
                    println!("no isos");
                    return Ok(());
                }
                println!("{:<24} {:>8}", "NAME", "SIZE");
                for iso in isos {
                    println!("{:<24} {:>8}", iso.name, format_size(iso.size_bytes));
                }
            }
            IsoCommand::Remove { name } => {
                delete(&client, &cli.url, &format!("/v1/isos/{name}")).await?;
            }
        },
    }
    Ok(())
}

fn print_vm(record: &VmRecord) {
    println!("{} {} {}", record.id, record.spec.name, record.state);
}

fn print_vol(record: &VolumeRecord) {
    println!(
        "{} {} {} {}",
        record.id,
        record.name,
        record.format,
        format_size(record.size_bytes)
    );
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
) -> Result<T> {
    let response = client
        .get(format!("{base}{path}"))
        .send()
        .await
        .with_context(|| {
            format!("connecting to {base} (is pertiskd running on {DEFAULT_LISTEN}?)")
        })?;
    read_json(response).await
}

async fn post_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: &B,
) -> Result<T> {
    let response = client
        .post(format!("{base}{path}"))
        .json(body)
        .send()
        .await
        .with_context(|| format!("connecting to {base}"))?;
    read_json(response).await
}

async fn post_empty<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
) -> Result<T> {
    let response = client
        .post(format!("{base}{path}"))
        .send()
        .await
        .with_context(|| format!("connecting to {base}"))?;
    read_json(response).await
}

async fn delete(client: &reqwest::Client, base: &str, path: &str) -> Result<()> {
    let response = client
        .delete(format!("{base}{path}"))
        .send()
        .await
        .with_context(|| format!("connecting to {base}"))?;
    if response.status() == reqwest::StatusCode::NO_CONTENT || response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    bail!("{status}: {text}");
}

async fn delete_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
) -> Result<T> {
    let response = client
        .delete(format!("{base}{path}"))
        .send()
        .await
        .with_context(|| format!("connecting to {base}"))?;
    read_json(response).await
}

async fn read_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(msg) = err.get("error").and_then(|v| v.as_str())
        {
            bail!("{status}: {msg}");
        }
        bail!("{status}: {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("decoding response: {text}"))
}
