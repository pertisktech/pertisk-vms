# pertisk-vm

Single-node virtualization control plane in Rust (phase 0–2).
Default driver on macOS is `mock`. Real guests need Linux KVM + Cloud Hypervisor.

```bash
cargo test --workspace

# terminal 1
cargo run -p pertisk-daemon

# terminal 2
cargo run -p pertisk-cli -- host
cargo run -p pertisk-cli -- vol create --name root --size 8M --format raw
cargo run -p pertisk-cli -- vol list
cargo run -p pertisk-cli -- vm create --name demo --cpus 1 --memory 512
cargo run -p pertisk-cli -- vm disk attach <vm> --volume <vol>
cargo run -p pertisk-cli -- iso import /path/to.iso
cargo run -p pertisk-cli -- vm cdrom attach <vm> --iso to.iso
```

Home directory: `~/.pertisk` (override with `PERTISK_HOME`).
On Linux, `pertiskd --driver cloud-hypervisor` talks to a `cloud-hypervisor` binary.
qcow2, linked clones, and internal snapshots need `qemu-img` on PATH.
