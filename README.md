# pertisk-vm

Virtualization control plane in Rust (phases 0–6).
Default driver on macOS is `mock`. Real guests need Linux KVM + Cloud Hypervisor.

Operators use the HTTP API, CLI, or web UI. Do not SSH onto the hypervisor for day-to-day VM work.

```bash
cargo test --workspace
PERTISK_ADMIN_PASSWORD=admin cargo run -p pertisk-daemon

cargo run -p pertisk-cli -- login -u admin -p admin
cargo run -p pertisk-cli -- cluster status
```

A 3-node cluster on one machine (mock):

```bash
PERTISK_HOME=/tmp/p1 cargo run -p pertisk-daemon -- --listen 127.0.0.1:7481 --node-name n1 --driver mock
PERTISK_HOME=/tmp/p2 cargo run -p pertisk-daemon -- --listen 127.0.0.1:7482 --node-name n2 --driver mock --join http://127.0.0.1:7481
PERTISK_HOME=/tmp/p3 cargo run -p pertisk-daemon -- --listen 127.0.0.1:7483 --node-name n3 --driver mock --join http://127.0.0.1:7482
```

Join from a running node: `pertisk --url http://127.0.0.1:7482 cluster join --peer http://127.0.0.1:7481 -u admin -p admin`.

Writes require majority quorum. A node that loses quorum fences itself (stops local VMs) so the majority can HA-restart them. `pertisk vm migrate <id>` moves a guest (mock starts on the destination before tearing down the source).

**Storage:** volumes are replicated as sparse files on N cluster nodes (`storage.backend = "replica"`, default `replica_count = 2`). HA and migrate prefer a node that already holds a replica, so the VM disk is already on the destination — no copy at fail time. Runtime writes land on the running node; they are pushed to other replicas on stop and before migrate. If the owner dies mid-run, unsynced writes after the last stop can be lost. Optional `backend = "rbd"` uses Ceph RBD when the `rbd` CLI is on PATH.

Web UI: [http://127.0.0.1:7480/](http://127.0.0.1:7480/)  
OpenAPI: [http://127.0.0.1:7480/v1/openapi.json](http://127.0.0.1:7480/v1/openapi.json)

The UI is a React app in `web/ui`. Rebuild it into the daemon with:

```bash
cd web/ui && npm install && npm run build
cargo run -p pertisk-daemon
```

Vite dev (proxies `/v1` to the daemon): `cd web/ui && npm run dev`.

Serial console is a websocket at `/v1/vms/{id}/console/ws?token=...`. The UI attaches after you click **console**. CLI: `pertisk vm console <id> --attach`.

Default bootstrap user is `admin`. Password is `PERTISK_ADMIN_PASSWORD` if set, otherwise `admin`. Restart `pertiskd` after upgrades.

Home directory: `~/.pertisk`. On Linux, `apply_host_links = true` creates bridge/TAP devices with `ip`. qcow2 needs `qemu-img`.

**Linux KVM guest (v0.1 gate):** install Cloud Hypervisor, then from the repo root:

```bash
PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-guest.sh
```

That fetches Alpine virt netboot (`vmlinuz-virt` + `initramfs-virt`), creates the guest with `--kernel` / `--initramfs`, starts it, and attaches serial. This Mac cannot close that gate (`/dev/kvm` is absent; the daemon stays on `mock`).

**Linux ISO guest (v1.0 gate):** same host, plus firmware. Storage → Import ISO (browser upload) then the guest wizard, or:

```bash
PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-iso-guest.sh
```

That downloads `hypervisor-fw` and an Alpine virt ISO, creates a disk, attaches the ISO, and boots Cloud Hypervisor with firmware (no kernel).
