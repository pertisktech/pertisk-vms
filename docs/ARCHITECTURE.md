# Pertisk architecture

Pertisk is a Proxmox-like KVM control plane. Each hypervisor runs **`pertiskd`**, which serves the web UI, REST API, and cluster protocol. Operators use the UI, CLI, TUI, or Terraform — not SSH — for day-to-day VM work.

| Surface | Bind | Notes |
| --- | --- | --- |
| HTTP API + UI | `0.0.0.0:7480` | Cluster heartbeats use this URL (`peer_url`) |
| HTTPS API + UI | `0.0.0.0:7443` | Self-signed cert; join from the UI typically uses this |
| Home | `/var/lib/pertisk` (appliance) or `~/.pertisk` | Config, state, volumes, TLS |

Writes require **majority quorum**. A node that loses quorum **fences** (stops local guests) so the majority can HA-restart them.

---

## 1. System context

```mermaid
flowchart LR
  subgraph operators [Operators]
    Browser[Web UI]
    CLI[pertisk CLI]
    TUI[pertisk-tui]
    TF[Terraform provider]
  end

  subgraph nodeA [Node A]
    DA[pertiskd]
    QA[QEMU / CH / mock]
    KA[KVM]
    SA[Disks + br0]
    DA --> QA --> KA
    DA --> SA
  end

  subgraph nodeB [Node B]
    DB[pertiskd]
    QB[QEMU / CH / mock]
    KB[KVM]
    SB[Disks + br0]
    DB --> QB --> KB
    DB --> SB
  end

  Browser -->|HTTPS 7443 / HTTP 7480| DA
  CLI --> DA
  TUI --> DA
  TF --> DA
  DA <-->|heartbeat join snapshot peer API| DB
```

Guests attach to a host bridge (`br0` after `pertisk-host-bridge`) and get LAN DHCP/SLAAC like any other machine on the switch.

---

## 2. Component diagram (crates)

```mermaid
flowchart TB
  UI[web/ui React HashRouter]
  UI -->|npm run build rust-embed| Daemon

  subgraph workspace [Rust workspace]
    Types[pertisk-types<br/>IDs specs config]
    API[pertisk-api<br/>OpenAPI login roles]
    VMM[pertisk-vmm<br/>qemu / cloud-hypervisor / mock]
    Storage[pertisk-storage<br/>volumes ISO cloud-init]
    Net[pertisk-net<br/>bridge TAP IPAM]
    Daemon[pertisk-daemon pertiskd]
    CLI[pertisk-cli]
    TUI[pertisk-tui]
  end

  Daemon --> Types
  Daemon --> API
  Daemon --> VMM
  Daemon --> Storage
  Daemon --> Net
  CLI --> API
  TUI --> API
  CLI -->|HTTP| Daemon
  TUI -->|HTTP| Daemon
```

| Crate | Role |
| --- | --- |
| `pertisk-types` | VM/volume/network/cluster records, `HostConfig`, address probing |
| `pertisk-api` | Login, roles, OpenAPI document |
| `pertisk-daemon` | Axum HTTP/HTTPS, `Service`, cluster, auth, metrics, embedded UI |
| `pertisk-vmm` | Start/stop/shutdown/restart guests |
| `pertisk-storage` | Sparse replica files or Ceph RBD; ISO; cloud-init seed |
| `pertisk-net` | Host bridges, TAP, neighbour/IP discovery |
| `pertisk-cli` / `pertisk-tui` | Operator clients |

Daemon modules inside `pertisk-daemon`:

```mermaid
flowchart LR
  HTTP[http.rs router] --> SVC[service.rs]
  SVC --> CL[cluster.rs]
  SVC --> ST[store.rs vms.json]
  SVC --> CTL[control.rs SQLite]
  SVC --> VMM[VmmBackend]
  SVC --> VOL[VolumePool]
  SVC --> NET[NetworkPool]
  HTTP --> TLS[tls.rs]
  HTTP --> STATIC[static_files.rs UI]
  HTTP --> CONS[console.rs / shell.rs]
  SVC --> MET[metrics.rs]
```

---

## 3. UI structure

The UI is a React HashRouter baked into `pertiskd` (`crates/pertisk-daemon/static/`). Layout holds inventory (`useInventory` → `/v1/host`, `/v1/cluster`, `/v1/vms`, …).

```mermaid
flowchart TB
  Login[Login /v1/login]
  Login --> Layout[Layout + resource tree]
  Layout --> DC[Datacenter /dc]
  Layout --> Node[Node /node/:id]
  Layout --> VM[Guest /vm/:id]

  DC --> DCsum[Summary]
  DC --> DCstor[Storage]
  DC --> DCtpl[Templates]
  DC --> DCnet[Networks]
  DC --> DCcl[Cluster]
  DC --> DCtf[Terraform]
  DC --> DCtasks[Task History]
  DC --> DCusers[Permissions]

  Node --> Nsum[Summary]
  Node --> Nguests[Guests]
  Node --> Nupd[Updates]
  Node --> Nrepo[Repositories]
  Node --> Nsh[Shell]
  Node --> Ntasks[Tasks]

  VM --> Vsum[Summary]
  VM --> Vcons[Console serial / VNC]
  VM --> Vhw[Hardware]
  VM --> Vopt[Options]
```

---

## 4. Data on a node

`$PERTISK_HOME` (appliance: `/var/lib/pertisk`):

```mermaid
flowchart TB
  HOME["/var/lib/pertisk"]
  HOME --> CFG[config.toml]
  HOME --> STATE[state/]
  STATE --> VMS[vms.json]
  STATE --> CLUS[cluster.json]
  STATE --> NETS[networks.json]
  STATE --> DB[control.db users tokens tasks audit]
  HOME --> STOR[storage/ volumes ISOs]
  HOME --> RUN[run/ qemu sockets]
  HOME --> TLSPEM[tls/ cert.pem key.pem]
```

- **Inventory** (VMs) is JSON replicated through cluster snapshots.
- **Users / tokens / tasks / audit** live in SQLite (`control.db`).
- **Volume bytes** are local sparse files (replica backend) or RBD.

---

## 5. Request flow (operator → guest)

```mermaid
sequenceDiagram
  participant UI as Web UI / CLI
  participant D as pertiskd
  participant S as Service
  participant C as Cluster
  participant V as VMM
  participant Q as QEMU

  UI->>D: Bearer token + POST /v1/vms/:id/start
  D->>D: authenticate
  D->>S: start
  S->>C: require quorum, not fenced
  S->>C: pick node / confirm local
  alt this node
    S->>V: start(record)
    V->>Q: qemu-system / cloud-hypervisor
    S->>S: persist Running in vms.json
    S->>C: replicate snapshot to peers
  else other node
    S->>D: POST peer_url/v1/peer/run
  end
  D-->>UI: VmRecord
```

Create-guest wizard (UI) is several calls: define VM → create/clone volume → attach disk → optional ISO/cloud-init → attach NIC → start.

---

## 6. Cluster join flow

`peer_url` must be a **LAN** address (not `127.0.0.1`). Unspecified listen `0.0.0.0:7480` is advertised as `http://<lan-ip>:7480`. HTTPS join (`https://<lan-ip>:7443`) is accepted with the peer’s self-signed cert.

```mermaid
sequenceDiagram
  participant B as Joining node
  participant A as Existing node

  B->>A: POST /v1/login
  A-->>B: token
  B->>A: GET /v1/cluster
  A-->>B: self_id + members
  B->>A: POST /v1/cluster/accept  NodeRecord
  Note over A: require quorum
  A->>A: add_member, bump generation
  A-->>B: ClusterSnapshot
  B->>B: apply snapshot, store A's real join URL
  loop every heartbeat_ms
    A->>B: POST /v1/peer/heartbeat
    B->>A: POST /v1/peer/heartbeat
  end
```

Quorum: `online * 2 > total` (1 of 1 yes, 1 of 2 no, 2 of 3 yes).

---

## 7. Fence and HA flow

```mermaid
flowchart TD
  TICK[cluster_tick 1s]
  TICK --> HB[send / receive heartbeats]
  HB --> Q{majority online?}
  Q -->|no| FENCE[fenced = true]
  FENCE --> STOP[stop local running guests]
  Q -->|yes| CLEAR[fenced = false]
  CLEAR --> HA[recover_ha]
  HA --> OWN{HA guest owner online?}
  OWN -->|yes| SKIP[leave running]
  OWN -->|no| PLACE[schedule on node that holds a replica]
  PLACE --> START[start_local or peer_run]
```

Runtime disk writes stay on the running node. Replicas are pushed on **stop** and before **migrate**. If the owner dies mid-run, unsynced writes after the last stop can be lost unless `storage.backend = "rbd"`.

---

## 8. Appliance boot

```mermaid
flowchart LR
  BOOT[systemd] --> FB[pertisk-firstboot]
  FB --> NET[pertisk-net DHCP]
  FB --> BR[pertisk-host-bridge br0]
  BOOT --> D[pertiskd]
  FB --> D
  D --> UI[UI on :7443 / :7480]
```

First boot copies `/etc/pertisk/config.toml` → `/var/lib/pertisk/config.toml`, sets `node_name` from the hostname when missing, writes the admin password, and seeds the `lan` network on `br0`.

---

## Related docs

- [CLUSTER_OPERATIONS.md](CLUSTER_OPERATIONS.md) — 3-node setup, quorum, fencing
- [GRAPHICS_CONSOLE.md](GRAPHICS_CONSOLE.md) — serial vs VNC
- [IMPLEMENTATION_SUMMARY.md](IMPLEMENTATION_SUMMARY.md) — feature checklist
