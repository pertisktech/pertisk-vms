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

Serial console is a websocket at `/v1/vms/{id}/console/ws?token=...`. The UI attaches after you click **console**. CLI: `pertisk vm console <id> --attach`.

Default bootstrap user is `admin`. Password is `PERTISK_ADMIN_PASSWORD` if set, otherwise `admin`. Restart `pertiskd` after upgrades.

Home directory: `~/.pertisk`. On Linux, `apply_host_links = true` creates bridge/TAP devices with `ip`. qcow2 needs `qemu-img`.
