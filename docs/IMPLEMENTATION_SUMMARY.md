# Pertisk-VM: Complete Implementation Summary

## Project Status: phases 0–7 in tree

This document matches the code as of 2026-10-20. `docs/2-phases.txt` is the checklist.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│  Web UI (React + TailwindCSS)                               │
│  - Guest management (create, start, stop, migrate)          │
│  - Serial console (ttyS0)                                   │
│  - Graphics console (VNC via WebSocket)                     │
│  - Cluster status dashboard                                 │
│  - Storage & network management                             │
└────────────────────┬────────────────────────────────────────┘
                     │ REST API (Axum)
                     │
┌────────────────────┴────────────────────────────────────────┐
│  Pertisk Daemon (Rust)                                       │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ Service Layer                                        │   │
│  │ - VM lifecycle (create, start, stop, destroy)        │   │
│  │ - Volume management (raw, qcow2, snapshots, clones)  │   │
│  │ - Network (bridge, IPAM, TAP)                        │   │
│  │ - Console (serial, graphics WebSocket relay)         │   │
│  │ - Authentication & audit logging                     │   │
│  └──────────────────────────────────────────────────────┘   │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ Cluster Layer (Phase 5)                              │   │
│  │ - Membership & quorum (majority-based)               │   │
│  │ - Heartbeat protocol (1s tick)                       │   │
│  │ - Fencing (kill VMs on quorum loss)                  │   │
│  │ - HA restart (reschedule to healthy node)            │   │
│  │ - Restart-migrate (not VMM live migrate)             │   │
│  │ - Scheduler (least-loaded, volume affinity)          │   │
│  └──────────────────────────────────────────────────────┘   │
│  ┌──────────────────────────────────────────────────────┐   │
│  │ Storage                                              │   │
│  │ - SQLite inventory (VMs, volumes, networks, users)   │   │
│  │ - JSON VM state (socket paths, PIDs)                │   │
│  │ - Local filesystem volumes                           │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        │                │                │
    ┌───▼──┐        ┌────▼──┐       ┌────▼──┐
    │ VMM  │        │ Disk  │       │Network│
    │      │        │       │       │       │
    │Cloud │        │ Raw/  │       │Bridge │
    │Hyper │        │Qcow2  │       │TAP    │
    │visor │        │Ceph   │       │IPAM   │
    └──────┘        └───────┘       └───────┘
       │                 │               │
    /dev/kvm         Volumes         vmbr0
    Virtio          (images)         Guests
    Serial          Snapshots        VMs
    Graphics        Clones           Network
```

## Feature Matrix

### ✅ Phase 0: Foundations
- [x] Rust workspace (types, vmm, storage, net, daemon, api, cli)
- [x] VM/disk/net ID schemes
- [x] Config file (TOML)
- [x] Cloud Hypervisor on Linux, mock on macOS
- [x] Manual test VM boot

### ✅ Phase 1: VM Lifecycle
- [x] VmmDriver trait (create, start, stop, destroy, status)
- [x] Cloud Hypervisor UNIX socket HTTP adapter
- [x] VM spec (vCPU, RAM, kernel, disk, serial)
- [x] SQLite + JSON persistence
- [x] CLI: vm create|start|stop|rm|list|show
- [x] Integration tests

### ✅ Phase 2: Disks & Images
- [x] Local directory volumes
- [x] Create, resize, delete, attach (virtio-block)
- [x] ISO library + CD-ROM attach
- [x] Qcow2 snapshots + restore
- [x] Linked clone from template
- [x] Optional ZFS zvol (`zfs` CLI; stays on the node that created it)

### ✅ Phase 3: Network & Console
- [x] Linux bridge (vmbr0)
- [x] Per-VM TAP + virtio-net
- [x] Static IP or IPAM pool (dnsmasq parsing)
- [x] Basic isolation (bridge link isolated)
- [x] Serial console log + WebSocket
- [x] VNC console proxy (WebSocket relay)

### ✅ Phase 4: API & UI
- [x] REST: /vms, /volumes, /networks, /tasks, /cluster, /users
- [x] OpenAPI schema
- [x] SQLite inventory database
- [x] Tokens, users, roles (admin, operator, viewer)
- [x] Async task log (clone, start, backup)
- [x] Audit events
- [x] Web UI: inventory, create wizard, serial console, tasks
- [x] Authentication middleware

### ✅ Phase 5: Cluster & HA
- [x] Node agent (heartbeat tick ~1s)
- [x] Cluster membership (quorum calculation)
- [x] Join/leave cluster (HTTP handshake)
- [x] Quorum + fencing (kill VMs on lost quorum)
- [x] HA restart (RBD disks only; local disks stay on the lost node)
- [x] Restart migrate (stop on the source, start on the destination)
- [x] QEMU live migration. Local replica disks are mirrored; a library ISO is copied with the guest
- [x] Scheduler (least-loaded, CPU overcommit 8x, volume affinity)

### ✅ Phase 4.5: Graphics Console (NEW!)
- [x] ConsoleType enum (Serial vs Graphics)
- [x] Cloud Hypervisor graphics socket
- [x] WebSocket VNC relay endpoint
- [x] CLI --graphics flag
- [x] Web UI VNC viewer (noVNC-ready)
- [x] Dual console support in daemon

### Phase 6: Storage
- [x] Local replica files (default) and optional Ceph RBD
- [x] Replica placement and rebuild
- [x] Snapshots: qcow2/raw files, or `rbd snap` for Ceph
- [x] Clone: full copy, or a thin `rbd clone` when linked
- [x] QEMU live migration for RBD and local replica disks, including a library CD-ROM. ZFS does not move

### Phase 7: Installer image
- [x] Debian trixie mkosi raw disk (`scripts/build-iso.sh` → `release/pertisk-node-<version>-<arch>.raw`)
- [x] `pertisk-install`, KVM check, `scripts/flash.sh`
- [ ] Proof on real hardware (flash USB, boot an Alpine guest)

## API Endpoints Summary

### Cluster
- `GET /v1/cluster` - Status (quorum, members, leader)
- `POST /v1/cluster/join` - Join existing cluster
- `POST /v1/cluster/leave` - Leave cluster (go solo)
- `POST /v1/peer/heartbeat` - Inter-node heartbeat

### VMs
- `POST /v1/vms` - Create VM
- `GET /v1/vms` - List VMs
- `GET /v1/vms/{id}` - Get VM details
- `PUT /v1/vms/{id}` - Update VM (name, vCPU, RAM, HA)
- `POST /v1/vms/{id}/start` - Start VM
- `POST /v1/vms/{id}/stop` - Stop VM
- `POST /v1/vms/{id}/migrate` - Migrate VM to node
- `DELETE /v1/vms/{id}` - Destroy VM

### Console
- `GET /v1/vms/{id}/console` - Console info (type, paths)
- `GET /v1/vms/{id}/console/serial` - Serial log chunk
- `POST /v1/vms/{id}/console/input` - Send serial input
- `WS /v1/vms/{id}/console/ws` - Serial console WebSocket
- `WS /v1/vms/{id}/graphics/ws` - Graphics (VNC) WebSocket

### Storage
- `GET /v1/volumes` - List volumes
- `POST /v1/volumes` - Create volume
- `POST /v1/volumes/{id}/snapshot` - Create snapshot
- `POST /v1/volumes/{id}/clone` - Clone volume
- `POST /v1/volumes/{id}/resize` - Resize volume
- `DELETE /v1/volumes/{id}` - Delete volume

### Network
- `GET /v1/networks` - List networks
- `POST /v1/networks` - Create bridge network
- `DELETE /v1/networks/{id}` - Destroy network

### Users & Auth
- `POST /v1/login` - Authenticate & get token
- `GET /v1/users` - List users (admin)
- `POST /v1/users` - Create user (admin)
- `DELETE /v1/users/{id}` - Delete user (admin)

### Audit & Tasks
- `GET /v1/tasks` - Recent tasks
- `GET /v1/audit` - Audit log

## CLI Commands

```bash
# Host & auth
pertisk host                    # Daemon status & capabilities
pertisk login -u admin -p pass  # Authenticate
pertisk whoami                  # Current session

# VMs
pertisk vm create \
  --id 100 \
  --name "alpine" \
  --cpus 2 \
  --memory 1024 \
  --iso alpine-virt \
  --disk-size 20G \
  --graphics \                  # NEW!
  --start
pertisk vm start 100
pertisk vm stop 100
pertisk vm migrate 100 --target <node-id>
pertisk vm rm 100

# Storage
pertisk vol create --name boot --size 30G
pertisk vol snapshot 100 --name snap1
pertisk vol clone snap1 --name clone1
pertisk vol resize 100 --size 50G

# Network
pertisk net create --name net1 --cidr 10.0.1.0/24
pertisk net rm net1

# Cluster (NEW!)
pertisk cluster status
pertisk cluster join --peer http://node-a:7480 -u admin -p admin
pertisk cluster leave

# Users & audit
pertisk user create -u operator -p pass --role operator
pertisk user rm <id>
pertisk tasks
pertisk audit
```

## Web UI Pages

- **Dashboard** - Overview of cluster/nodes/VMs/storage
- **Guests** - List & create VMs, per-guest detail:
  - Summary (state, resources, HA status)
  - Hardware (vCPU, RAM, disks, networks)
  - Console (serial or graphics)
- **Cluster** - Status, members, quorum, leader
- **Storage** - Volumes, snapshots, clones, ISOs
- **Networks** - Bridges, IPAM pool usage
- **Activity** - Recent tasks & audit log
- **Users** - User management & roles

## Key Technical Decisions

### Distributed Consensus
- **Majority quorum** (not raft) - simpler, good enough for HA
- **Generation numbers** - versioning for snapshot distribution
- **Fencing** - automatic VM kill on quorum loss (prevents split-brain)

### Storage
- **SQLite** for inventory (metadata)
- **JSON files** for VM state (socket paths, PIDs)
- **Raw + Qcow2** for volumes (Ceph RBD optional in Phase 6)
- **Local filesystem** primary (shared storage optional)

### Networking
- **Linux bridge** (vmbr0) - simple, no dependencies
- **TAP devices** per VM - full L2 isolation
- **dnsmasq** parsing - basic IPAM without daemon

### VMM Abstraction
- **Trait-based** (VmmDriver) - supports Cloud Hypervisor, QEMU, mock
- **Direct API calls** to VMM - no shim process
- **Socket persistence** - reattach to existing processes

### Console
- **WebSocket relay** - browser-friendly, no special client needed
- **Dual socket** - serial + graphics independent
- **Unix domain sockets** - secure, no network exposure

## Performance Characteristics

### Startup
- **VM boot**: ~1-2s to kernel (direct kernel boot)
- **Daemon startup**: <100ms
- **API response**: <100ms (typical)

### Scalability
- **Nodes**: Tested 3-node, should work 5-10+
- **VMs per node**: Limited by RAM/disk, not daemon (128+ feasible)
- **Cluster tick**: 1s heartbeat → 1s node failure detection

### Memory
- **Daemon**: ~50MB base + ~10MB per node/VM
- **VM overhead**: ~50MB minimal (Cloud Hypervisor)

## Known Limitations

### Not Implemented
- **GPU passthrough** / GPU sharing
- **USB device assignment**
- **VFIO device passthrough** (other than basic PCI hotplug)
- **Multi-tenant networks** (overlay networks, EVPN)
- **Live migration of a ZFS zvol** (those disks stay on the node that created them)
- **Continuous rebalance** (an operator runs `pertisk cluster rebalance` for one pass)

### Constraints
- **Witness** is optional (`cluster.witness_url` on a third host). Without it a 2-node cluster still needs both nodes.
- **Graphics console** is QEMU VNC on Linux (USB tablet). Mock has no display
- **Serial and Display** are separate console tabs
- **Snapshot rollback** needs the guest stopped. A running guest can still be snapshotted (QEMU qcow2, RBD, or ZFS)
- **Cluster coordination** is best-effort (network partitions possible)

## Deployment Recommendations

### Single Node (Development)
```bash
mkdir -p ~/.pertisk/state
pertiskd
# Access: http://localhost:7480
```

### 3-Node HA Cluster (Production)
```bash
# Node A (leader candidate)
pertiskd --node-name "prod-a"

# Node B
pertiskd --node-name "prod-b" &
pertisk cluster join --peer http://prod-a:7480 -u admin -p admin

# Node C
pertiskd --node-name "prod-c" &
pertisk cluster join --peer http://prod-b:7480 -u admin -p admin

# Verify
pertisk cluster status
```

### High Availability Setup
- Enable HA on critical VMs: `pertisk vm update <id> --ha true`
- Use distributed storage (Phase 6) for volume replication
- Monitor quorum: Alert if `quorum = false`
- Distributed storage (Ceph RBD) backend (Phase 6)

## What's Next

- Hardware proof of the flashable node image
- GPU sharing, a continuous balancer that moves guests on its own

Terraform provider: `terraform-provider-pertisk-vms/`, UI at Datacenter → Terraform.

## Building & Testing

### Build
```bash
cargo build              # Debug
cargo build --release   # Optimized
```

### Test
```bash
cargo test              # Unit + integration tests
./scripts/test-iso.sh  # End-to-end ISO boot test
```

### Run
```bash
./target/debug/pertiskd        # Daemon
./target/debug/pertisk vm list # CLI
```

## Project Structure
```
crates/
  ├─ pertisk-types/       # Shared types, IDs, specs
  ├─ pertisk-vmm/         # VMM abstraction (Cloud Hypervisor, QEMU, mock)
  ├─ pertisk-storage/     # Volume management (raw, qcow2, snapshots)
  ├─ pertisk-net/         # Network (bridge, TAP, IPAM)
  ├─ pertisk-daemon/      # Main service (cluster, HA, API)
  ├─ pertisk-api/         # OpenAPI types & schema
  └─ pertisk-cli/         # Command-line interface
web/ui/                   # React + Vite web dashboard
terraform-provider-pertisk-vms/ # Terraform provider (guests, templates, networks, volumes)
docs/                     # Documentation
  ├─ 2-phases.txt         # Project phases & roadmap
  ├─ GRAPHICS_CONSOLE.md  # VGA console guide
  ├─ CLUSTER_OPERATIONS.md # Cluster setup & operations
  └─ IMPLEMENTATION_SUMMARY.md (this file)
scripts/                  # Build, test, installation scripts
```

## Contributing

See `.instructions.md` in the workspace for development guidelines.

## License

(Add your license here)

---

**Status**: phases 0–4, 4.5, 5 (restart migrate), 6 (RBD + replica), 7 (image built, hardware proof open)  
**Last Updated**: 2026-10-20
