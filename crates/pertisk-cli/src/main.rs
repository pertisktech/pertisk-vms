use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use pertisk_api::{
    AuditEvent, CreateUserRequest, CreateVmRequest, LoginRequest, Role, TaskRecord, TokenResponse, UserRecord,
};
use pertisk_types::{
    AttachDiskRequest, AttachIsoRequest, AttachNicRequest, CloneVolumeRequest, CloudInitIsoRequest,
    ClusterStatus, ConsoleInfo, CreateNetworkRequest, CreateVolumeRequest, DEFAULT_LISTEN,
    DiskSpec, HostInfo, ImportIsoRequest, IsoRecord, JoinClusterRequest, MigrateRequest, NetworkId,
    NetworkRecord, ResizeVolumeRequest, SerialChunk, SnapshotRequest, UpdateVmRequest, VmId,
    VmRecord, VmSpec, VolumeFormat, VolumeId, VolumeRecord, default_home, format_size, parse_size,
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
    /// Sign in and store an API token.
    Login {
        #[arg(long, short)]
        username: String,
        #[arg(long, short)]
        password: String,
    },
    /// Show the current session.
    Whoami,
    /// Recent tasks.
    Tasks,
    /// Audit log.
    Audit,
    /// Local users (admin).
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
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
    /// Virtual networks (bridge + IPAM).
    Net {
        #[command(subcommand)]
        command: NetCommand,
    },
    /// Cluster membership and HA.
    Cluster {
        #[command(subcommand)]
        command: ClusterCommand,
    },
}

#[derive(Debug, Subcommand)]
enum UserCommand {
    List,
    Create {
        #[arg(long, short)]
        username: String,
        #[arg(long, short)]
        password: String,
        #[arg(long, default_value = "operator")]
        role: String,
    },
    #[command(name = "rm")]
    Remove {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
enum ClusterCommand {
    Status,
    Join {
        #[arg(long)]
        peer: String,
        #[arg(long, short)]
        username: String,
        #[arg(long, short)]
        password: String,
    },
    Leave,
}

#[derive(Debug, Subcommand)]
enum VmCommand {
    Create {
        /// Numeric VM ID (3-10 digits).
        #[arg(long)]
        id: VmId,
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 1)]
        cpus: u8,
        #[arg(long, default_value_t = 512)]
        memory: u32,
        #[arg(long)]
        kernel: Option<PathBuf>,
        #[arg(long)]
        initramfs: Option<PathBuf>,
        #[arg(long)]
        firmware: Option<PathBuf>,
        #[arg(long)]
        cmdline: Option<String>,
        #[arg(long)]
        disk: Vec<PathBuf>,
        /// ISO library name, or a host path to import first.
        #[arg(long)]
        iso: Option<String>,
        /// Create and attach a new boot disk (e.g. 32G). Used with --iso.
        #[arg(long)]
        disk_size: Option<String>,
        /// Attach this network (id or name).
        #[arg(long)]
        net: Option<String>,
        /// Use graphics (VGA) console instead of serial.
        #[arg(long)]
        graphics: bool,
        /// Start the guest after create (ISO/disk attach included).
        #[arg(long)]
        start: bool,
    },
    Start {
        id: VmId,
    },
    Stop {
        id: VmId,
    },
    Migrate {
        id: VmId,
        #[arg(long)]
        target: Option<String>,
    },
    #[command(name = "rm")]
    Remove {
        id: VmId,
    },
    List,
    Show {
        id: VmId,
    },
    /// Change name, vCPU, memory, or HA while the guest is defined.
    Update {
        id: VmId,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cpus: Option<u8>,
        #[arg(long)]
        memory: Option<u32>,
        #[arg(long)]
        ha: Option<bool>,
    },
    Disk {
        #[command(subcommand)]
        command: DiskCommand,
    },
    Cdrom {
        #[command(subcommand)]
        command: CdromCommand,
    },
    Nic {
        #[command(subcommand)]
        command: NicCommand,
    },
    /// Serial console. --follow tails the log; --attach is an interactive websocket.
    Console {
        id: VmId,
        #[arg(long)]
        follow: bool,
        #[arg(long)]
        attach: bool,
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
enum NicCommand {
    Attach {
        vm: VmId,
        #[arg(long)]
        network: NetworkId,
        #[arg(long)]
        ip: Option<String>,
    },
    Detach {
        vm: VmId,
        #[arg(long)]
        tap: String,
    },
}

#[derive(Debug, Subcommand)]
enum NetCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "10.88.0.0/24")]
        cidr: String,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        bridge: Option<String>,
        #[arg(long, default_value = "nat")]
        mode: String,
        #[arg(long, default_value_t = true)]
        dhcp: bool,
        #[arg(long, default_value_t = true)]
        isolate: bool,
    },
    List,
    Show {
        id: NetworkId,
    },
    #[command(name = "rm")]
    Remove {
        id: NetworkId,
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
        #[arg(long)]
        replicas: Option<u8>,
    },
    List,
    Show {
        id: VolumeId,
    },
    #[command(name = "rm")]
    Remove {
        id: VolumeId,
    },
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
    /// Cloud-init NoCloud seed ISO (cidata). Attach as CD-ROM next to a cloud disk image.
    CloudInit {
        #[arg(long)]
        name: String,
        #[arg(long)]
        hostname: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long = "ssh-key")]
        ssh_key: Vec<String>,
        #[arg(long)]
        userdata: Option<String>,
    },
    List,
    #[command(name = "rm")]
    Remove {
        name: String,
    },
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
            println!(
                "firmware           {}",
                info.firmware
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "not found (kernel boot only)".into())
            );
            println!("listen             {}", info.listen);
            println!("data_dir           {}", info.data_dir.display());
            println!("storage            {}", info.storage_root.display());
            println!(
                "backend            {} replicas={} rbd={}",
                info.storage_backend,
                info.replica_count,
                if info.rbd { "available" } else { "not found" }
            );
            println!(
                "qemu-img           {}",
                info.qemu_img
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "not found (raw volumes only)".into())
            );
            println!(
                "host-links         {}",
                if info.apply_host_links {
                    "linux ip/tap"
                } else {
                    "inventory only"
                }
            );
            println!(
                "node               {}",
                info.node_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".into())
            );
            println!("quorum             {}", info.quorum);
            if let Ok(status) = get_json::<ClusterStatus>(&client, &cli.url, "/v1/cluster").await {
                for member in &status.members {
                    if info.node_id == Some(member.id) {
                        println!(
                            "capacity           vcpu {}/{} mem {}/{} MiB",
                            member.used_vcpus,
                            member.cpus,
                            member.used_memory_mib,
                            member.memory_mib
                        );
                    }
                }
            }
            if !info.kvm {
                eprintln!("note: /dev/kvm is missing; this machine can run the mock driver only");
            }
        }
        Command::Login { username, password } => {
            let out: TokenResponse = post_json(
                &client,
                &cli.url,
                "/v1/login",
                &LoginRequest { username, password },
            )
            .await?;
            let path = token_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &out.token)?;
            println!(
                "{} {} (token saved to {})",
                out.username,
                out.role,
                path.display()
            );
        }
        Command::Whoami => {
            let session: serde_json::Value = get_json(&client, &cli.url, "/v1/session").await?;
            println!("{}", serde_json::to_string_pretty(&session)?);
        }
        Command::Tasks => {
            let tasks: Vec<TaskRecord> = get_json(&client, &cli.url, "/v1/tasks").await?;
            if tasks.is_empty() {
                println!("no tasks");
                return Ok(());
            }
            println!("{:<36} {:<14} {:<8} {}", "ID", "KIND", "STATUS", "ACTOR");
            for task in tasks {
                println!(
                    "{:<36} {:<14} {:<8} {}",
                    task.id, task.kind, task.status, task.actor
                );
            }
        }
        Command::Audit => {
            let events: Vec<AuditEvent> = get_json(&client, &cli.url, "/v1/audit").await?;
            if events.is_empty() {
                println!("no audit events");
                return Ok(());
            }
            for event in events {
                println!(
                    "{} {} {}",
                    event.actor,
                    event.action,
                    event.target.unwrap_or_default()
                );
            }
        }
        Command::User { command } => match command {
            UserCommand::List => {
                let users: Vec<UserRecord> = get_json(&client, &cli.url, "/v1/users").await?;
                if users.is_empty() {
                    println!("no users");
                    return Ok(());
                }
                println!("{:<38} {:<16} {}", "ID", "USERNAME", "ROLE");
                for user in users {
                    println!("{:<38} {:<16} {}", user.id, user.username, user.role);
                }
            }
            UserCommand::Create {
                username,
                password,
                role,
            } => {
                let role: Role = role.parse().map_err(|err| anyhow::anyhow!("{err}"))?;
                let user: UserRecord = post_json(
                    &client,
                    &cli.url,
                    "/v1/users",
                    &CreateUserRequest {
                        username,
                        password,
                        role,
                    },
                )
                .await?;
                println!("{} {} {}", user.id, user.username, user.role);
            }
            UserCommand::Remove { id } => {
                delete(&client, &cli.url, &format!("/v1/users/{id}")).await?;
            }
        },
        Command::Cluster { command } => match command {
            ClusterCommand::Status => {
                let status: ClusterStatus = get_json(&client, &cli.url, "/v1/cluster").await?;
                println!(
                    "cluster {} gen {} leader {} quorum {} fenced {}",
                    status.name,
                    status.generation,
                    status
                        .leader_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "-".into()),
                    status.quorum,
                    status.fenced
                );
                println!(
                    "{:<38} {:<12} {:<8} {:<12} {:<16} {}",
                    "ID", "NAME", "ONLINE", "VCPU", "MEM MiB", "URL"
                );
                for member in status.members {
                    println!(
                        "{:<38} {:<12} {:<8} {:<12} {:<16} {}",
                        member.id,
                        member.name,
                        member.online,
                        format!("{}/{}", member.used_vcpus, member.cpus),
                        format!("{}/{}", member.used_memory_mib, member.memory_mib),
                        member.peer_url
                    );
                }
            }
            ClusterCommand::Join {
                peer,
                username,
                password,
            } => {
                let status: ClusterStatus = post_json(
                    &client,
                    &cli.url,
                    "/v1/cluster/join",
                    &JoinClusterRequest {
                        peer,
                        username,
                        password,
                    },
                )
                .await?;
                println!(
                    "joined {} ({} nodes, quorum {})",
                    status.name,
                    status.members.len(),
                    status.quorum
                );
            }
            ClusterCommand::Leave => {
                let status: ClusterStatus =
                    post_empty(&client, &cli.url, "/v1/cluster/leave").await?;
                println!("solo {} quorum {}", status.name, status.quorum);
            }
        },
        Command::Vm { command } => match command {
            VmCommand::Create {
                id,
                name,
                cpus,
                memory,
                kernel,
                initramfs,
                firmware,
                cmdline,
                disk,
                iso,
                disk_size,
                net,
                graphics,
                start,
            } => {
                let host: HostInfo = get_json(&client, &cli.url, "/v1/host").await?;
                let firmware = firmware.or(host.firmware.clone());
                if iso.is_some() && kernel.is_none() && firmware.is_none() {
                    bail!(
                        "ISO boot needs firmware (hypervisor-fw). Install it or pass --firmware. See scripts/linux-host.sh"
                    );
                }
                let iso_name = if let Some(iso) = iso.as_ref() {
                    Some(ensure_iso(&client, &cli.url, iso).await?)
                } else {
                    None
                };
                let spec = VmSpec {
                    name: name.clone(),
                    vcpus: cpus,
                    memory_mib: memory,
                    kernel,
                    cmdline,
                    initramfs,
                    firmware,
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
                    console_type: if graphics { pertisk_types::ConsoleType::Graphics } else { pertisk_types::ConsoleType::Serial },
                    ha: true,
                };
                let mut record: VmRecord = post_json(
                    &client,
                    &cli.url,
                    "/v1/vms",
                    &CreateVmRequest {
                        id: Some(id),
                        spec,
                    },
                )
                .await?;
                if let Some(size) = disk_size {
                    let vol: VolumeRecord = post_json(
                        &client,
                        &cli.url,
                        "/v1/volumes",
                        &CreateVolumeRequest {
                            name: format!("{name}-disk"),
                            size_bytes: parse_size(&size)?,
                            format: VolumeFormat::Raw,
                            replicas: None,
                        },
                    )
                    .await?;
                    record = post_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{}/disks", record.id),
                        &AttachDiskRequest { volume_id: vol.id },
                    )
                    .await?;
                }
                if let Some(iso_name) = iso_name {
                    record = post_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{}/cdrom", record.id),
                        &AttachIsoRequest { iso: iso_name },
                    )
                    .await?;
                }
                if let Some(net) = net {
                    let net_id = resolve_network(&client, &cli.url, &net).await?;
                    record = post_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{}/nics", record.id),
                        &AttachNicRequest {
                            network_id: net_id,
                            ip: None,
                        },
                    )
                    .await?;
                }
                if start {
                    record = post_empty(&client, &cli.url, &format!("/v1/vms/{}/start", record.id))
                        .await?;
                }
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
            VmCommand::Migrate { id, target } => {
                let record: VmRecord = post_json(
                    &client,
                    &cli.url,
                    &format!("/v1/vms/{id}/migrate"),
                    &MigrateRequest {
                        target: target.map(|s| s.parse()).transpose()?,
                    },
                )
                .await?;
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
                    "{:<38} {:<16} {:<10} {:>4} {:>8} {:>6} {:>4}",
                    "ID", "NAME", "STATE", "CPU", "MEM", "DISKS", "NIC"
                );
                for vm in vms {
                    println!(
                        "{:<38} {:<16} {:<10} {:>4} {:>8} {:>6} {:>4}",
                        vm.id,
                        vm.spec.name,
                        vm.state,
                        vm.spec.vcpus,
                        vm.spec.memory_mib,
                        vm.spec.disks.len(),
                        vm.spec.nets.len()
                    );
                }
            }
            VmCommand::Show { id } => {
                let record: VmRecord =
                    get_json(&client, &cli.url, &format!("/v1/vms/{id}")).await?;
                println!("{}", serde_json::to_string_pretty(&record)?);
            }
            VmCommand::Update {
                id,
                name,
                cpus,
                memory,
                ha,
            } => {
                let record: VmRecord = patch_json(
                    &client,
                    &cli.url,
                    &format!("/v1/vms/{id}"),
                    &UpdateVmRequest {
                        name,
                        vcpus: cpus,
                        memory_mib: memory,
                        ha,
                    },
                )
                .await?;
                print_vm(&record);
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
            VmCommand::Nic { command } => match command {
                NicCommand::Attach { vm, network, ip } => {
                    let record: VmRecord = post_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{vm}/nics"),
                        &AttachNicRequest {
                            network_id: network,
                            ip,
                        },
                    )
                    .await?;
                    print_vm(&record);
                }
                NicCommand::Detach { vm, tap } => {
                    let record: VmRecord =
                        delete_json(&client, &cli.url, &format!("/v1/vms/{vm}/nics/{tap}")).await?;
                    print_vm(&record);
                }
            },
            VmCommand::Console { id, follow, attach } => {
                if attach {
                    attach_console(&cli.url, id).await?;
                    return Ok(());
                }
                let mut from = 0u64;
                loop {
                    let chunk: SerialChunk = get_json(
                        &client,
                        &cli.url,
                        &format!("/v1/vms/{id}/console/serial?from={from}&max=8192"),
                    )
                    .await?;
                    if !chunk.text.is_empty() {
                        print!("{}", chunk.text);
                    }
                    from = chunk.next;
                    if !follow {
                        if from == 0 {
                            let info: ConsoleInfo =
                                get_json(&client, &cli.url, &format!("/v1/vms/{id}/console"))
                                    .await?;
                            if info.size == 0 {
                                println!("(empty serial log)");
                            }
                        }
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
            }
        },
        Command::Vol { command } => match command {
            VolCommand::Create {
                name,
                size,
                format,
                replicas,
            } => {
                let req = CreateVolumeRequest {
                    name,
                    size_bytes: parse_size(&size)?,
                    format,
                    replicas,
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
                    "{:<38} {:<16} {:<6} {:>8} {:>8} {:>8}",
                    "ID", "NAME", "FMT", "SIZE", "SNAPS", "REPL"
                );
                for vol in vols {
                    println!(
                        "{:<38} {:<16} {:<6} {:>8} {:>8} {:>8}",
                        vol.id,
                        vol.name,
                        vol.format,
                        format_size(vol.size_bytes),
                        vol.snapshots.len(),
                        vol.replicas
                            .len()
                            .max(usize::from(vol.replica_count.max(1)))
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
            IsoCommand::CloudInit {
                name,
                hostname,
                user,
                password,
                ssh_key,
                userdata,
            } => {
                let iso: IsoRecord = post_json(
                    &client,
                    &cli.url,
                    "/v1/isos/cloud-init",
                    &CloudInitIsoRequest {
                        name,
                        hostname,
                        user,
                        password,
                        ssh_authorized_keys: ssh_key,
                        userdata,
                    },
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
        Command::Net { command } => match command {
            NetCommand::Create {
                name,
                cidr,
                gateway,
                bridge,
                mode,
                dhcp,
                isolate,
            } => {
                let mode = match mode.as_str() {
                    "bridge" => pertisk_types::NetworkMode::Bridge,
                    "nat" => pertisk_types::NetworkMode::Nat,
                    other => bail!("unknown network mode '{other}' (nat | bridge)"),
                };
                if mode == pertisk_types::NetworkMode::Bridge && bridge.is_none() {
                    bail!("--mode bridge requires --bridge NAME (existing host bridge)");
                }
                let net: NetworkRecord = post_json(
                    &client,
                    &cli.url,
                    "/v1/networks",
                    &CreateNetworkRequest {
                        name,
                        cidr,
                        gateway,
                        bridge,
                        dhcp: if mode == pertisk_types::NetworkMode::Bridge {
                            false
                        } else {
                            dhcp
                        },
                        isolate: if mode == pertisk_types::NetworkMode::Bridge {
                            false
                        } else {
                            isolate
                        },
                        mode,
                    },
                )
                .await?;
                println!(
                    "{} {} {} {} {}",
                    net.id, net.name, net.mode, net.bridge, net.cidr
                );
            }
            NetCommand::List => {
                let nets: Vec<NetworkRecord> = get_json(&client, &cli.url, "/v1/networks").await?;
                if nets.is_empty() {
                    println!("no networks");
                    return Ok(());
                }
                println!(
                    "{:<38} {:<12} {:<8} {:<8} {:<18} {}",
                    "ID", "NAME", "MODE", "BRIDGE", "CIDR", "DHCP"
                );
                for net in nets {
                    println!(
                        "{:<38} {:<12} {:<8} {:<8} {:<18} {}",
                        net.id, net.name, net.mode, net.bridge, net.cidr, net.dhcp
                    );
                }
            }
            NetCommand::Show { id } => {
                let net: NetworkRecord =
                    get_json(&client, &cli.url, &format!("/v1/networks/{id}")).await?;
                println!("{}", serde_json::to_string_pretty(&net)?);
            }
            NetCommand::Remove { id } => {
                delete(&client, &cli.url, &format!("/v1/networks/{id}")).await?;
            }
        },
    }
    Ok(())
}

fn print_vm(record: &VmRecord) {
    println!(
        "{} {} {} {}",
        record.id,
        record.spec.name,
        record.state,
        record
            .node_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "-".into())
    );
}

fn print_vol(record: &VolumeRecord) {
    println!(
        "{} {} {} {} replicas={}",
        record.id,
        record.name,
        record.format,
        format_size(record.size_bytes),
        record.replicas.len()
    );
}

fn token_path() -> PathBuf {
    if let Ok(path) = std::env::var("PERTISK_TOKEN_FILE") {
        return PathBuf::from(path);
    }
    default_home().join("token")
}

fn load_token() -> Option<String> {
    std::env::var("PERTISK_TOKEN")
        .ok()
        .or_else(|| std::fs::read_to_string(token_path()).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn ensure_iso(client: &reqwest::Client, base: &str, iso: &str) -> Result<String> {
    let path = PathBuf::from(iso);
    if path.is_file() {
        let record: IsoRecord = post_json(
            client,
            base,
            "/v1/isos",
            &ImportIsoRequest { path, name: None },
        )
        .await?;
        return Ok(record.name);
    }
    let isos: Vec<IsoRecord> = get_json(client, base, "/v1/isos").await?;
    if isos.iter().any(|item| item.name == iso) {
        return Ok(iso.to_string());
    }
    bail!(
        "ISO '{iso}' is not in the library and is not a file. Import it first: pertisk iso import /path/to.iso"
    )
}

async fn resolve_network(client: &reqwest::Client, base: &str, net: &str) -> Result<NetworkId> {
    if let Ok(id) = net.parse::<NetworkId>() {
        return Ok(id);
    }
    let nets: Vec<NetworkRecord> = get_json(client, base, "/v1/networks").await?;
    nets.into_iter()
        .find(|n| n.name == net)
        .map(|n| n.id)
        .ok_or_else(|| anyhow::anyhow!("network '{net}' not found (id or name)"))
}

fn with_auth(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match load_token() {
        Some(token) => req.header("Authorization", format!("Bearer {token}")),
        None => req,
    }
}

fn ws_url(http: &str, path: &str, token: &str) -> String {
    let base = if let Some(rest) = http.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = http.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{http}")
    };
    format!("{}{path}?token={token}", base.trim_end_matches('/'))
}

async fn attach_console(base: &str, id: VmId) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let token = load_token().context("not logged in (pertisk login)")?;
    let url = ws_url(base, &format!("/v1/vms/{id}/console/ws"), &token);
    let (ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .with_context(|| format!("websocket {url}"))?;
    let (mut sink, mut stream) = ws.split();
    let mut stdout = tokio::io::stdout();
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        stdout.write_all(text.as_bytes()).await?;
                        stdout.flush().await?;
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) => {
                        stdout.write_all(&bytes).await?;
                        stdout.flush().await?;
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => break,
                    Some(Err(err)) => bail!("{err}"),
                    Some(Ok(_)) => {}
                }
            }
            line = stdin.next_line() => {
                match line? {
                    Some(text) => {
                        sink.send(tokio_tungstenite::tungstenite::Message::Text(
                            format!("{text}\n").into(),
                        ))
                        .await?;
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
) -> Result<T> {
    let response = with_auth(client.get(format!("{base}{path}")))
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
    let response = with_auth(client.post(format!("{base}{path}")))
        .json(body)
        .send()
        .await
        .with_context(|| format!("connecting to {base}"))?;
    read_json(response).await
}

async fn patch_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: &B,
) -> Result<T> {
    let response = with_auth(client.patch(format!("{base}{path}")))
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
    let response = with_auth(client.post(format!("{base}{path}")))
        .send()
        .await
        .with_context(|| format!("connecting to {base}"))?;
    read_json(response).await
}

async fn delete(client: &reqwest::Client, base: &str, path: &str) -> Result<()> {
    let response = with_auth(client.delete(format!("{base}{path}")))
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
    let response = with_auth(client.delete(format!("{base}{path}")))
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
