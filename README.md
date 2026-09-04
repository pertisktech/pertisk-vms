# pertisk-vm

Virtualization control plane in Rust (phases 0–7).
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

Guest power: `POST /v1/vms/{id}/start|stop|shutdown|restart`. **Stop** force-kills the hypervisor; **shutdown** sends ACPI (waits up to 120s); **restart** hard-resets (QEMU) or stop+start (Cloud Hypervisor). CLI: `pertisk vm shutdown|restart <id>`.

**Terminal console:** on the node, run `pertisk-tui` (serial/SSH). It shows LAN IP(s), the admin password, and lets you start/stop/shutdown/restart guests. Default bootstrap password is **`admin`** unless `PERTISK_ADMIN_PASSWORD` is set before first boot.

Default bootstrap user is `admin`. Password is `PERTISK_ADMIN_PASSWORD` if set, otherwise `admin`. Restart `pertiskd` after upgrades.

Home directory: `~/.pertisk`. On Linux, `apply_host_links = true` creates bridge/TAP devices with `ip`. qcow2 needs `qemu-img`.

**Linux KVM guest (v0.1 gate):** install Cloud Hypervisor, then from the repo root:

```bash
PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-guest.sh
```

That fetches Alpine virt netboot (`vmlinuz-virt` + `initramfs-virt`), creates the guest with `--kernel` / `--initramfs`, starts it, and attaches serial. This Mac cannot close that gate (`/dev/kvm` is absent; the daemon stays on `mock`).

**Linux ISO guest (v1.0 gate):** same host, plus firmware. On a test box (e.g. 16c / 64G):

```bash
./scripts/linux-host.sh
PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-iso-guest.sh
```

`linux-host.sh` checks KVM, fetches Cloud Hypervisor + `hypervisor-fw`, and writes `~/.pertisk/config.toml` (`0.0.0.0:7480`, replica_count 1). Then either the script above, Storage → Import ISO + guest wizard, or:

```bash
pertisk iso import /path/to/alpine-virt.iso
pertisk vm create --name alpine --cpus 4 --memory 4096 --iso alpine-virt.iso --disk-size 32G --start
pertisk vm console <id> --attach
```

Alpine virt / cloud images use **serial**. Graphical Ubuntu/Windows installers need VGA (not in Cloud Hypervisor).

Ubuntu/Debian **installer ISOs** cannot EFI-boot: Cloud Hypervisor firmware (`hypervisor-fw`) does not implement shim/Secure Boot (`import_mok_state Unsupported`). On start, pertisk extracts `casper/vmlinuz` (or Debian `install.amd`) and kernel-boots with `console=ttyS0`. After install, detach the ISO and start again to boot the disk. Prefer Alpine virt, or a cloud image + cloud-init.

**Cloud-init ISO** (Linux cloud images, not Proxmox/ESXi installers): Storage → Import ISO → Cloud-init, or:

```bash
pertisk iso cloud-init --name web-1 --hostname web-1 --user ubuntu --password ubuntu
pertisk vm cdrom attach --iso web-1-cidata.iso <id>
```

Attach that seed last; firmware boots an installer ISO or the OS disk, not the cidata volume.

**Node install (phase 7):** Debian/Armbian + pertiskd, flashed like Proxmox. No tarball, no `br0` by hand.

Pick the image for the **machine**, not the CPU architecture:

| Machine | Image | Notes |
|---|---|---|
| x86_64 UEFI PC | `pertisk-node-VERSION-amd64.raw.xz` | USB live → `pertisk-install` to NVMe |
| UEFI ARM server | `pertisk-node-VERSION-arm64.raw.xz` | Same as amd64; GRUB EFI |
| Orange Pi 5 Plus | `pertisk-node-VERSION-orangepi5plus.img.xz` | Vendor U-Boot + kernel |
| Orange Pi 5 Max | `pertisk-node-VERSION-orangepi5max.img.xz` | Not the Plus image |
| Raspberry Pi 5 | `pertisk-node-VERSION-rpi5.img.xz` | 64-bit; needs `/dev/kvm` |

**Do not** flash `pertisk-node-*-arm64.raw` onto Orange Pi or Raspberry Pi. That file is generic Debian EFI/GRUB and will not boot vendor firmware.

**SBC (download → dd → boot):**

```bash
VER=0.1.2
BOARD=orangepi5plus   # or orangepi5max / rpi5
curl -fL -O https://github.com/pertisktech/pertisk-vms/releases/download/${VER}/pertisk-node-${VER}-${BOARD}.img.xz
xzcat pertisk-node-${VER}-${BOARD}.img.xz | sudo dd of=/dev/sdX bs=4M status=progress conv=fsync
```

Insert the SD, boot, open **http://board-ip:7480/** (`admin` / password in `/etc/pertisk/admin`). First boot creates LAN `br0` and a `lan` network on it.

Move the OS to NVMe (one command, then pull the SD):

```bash
pertisk-install --disk /dev/nvme0n1 --yes
```

Power off, remove the SD, power on. If it does not start, put the SD back.

Build a board image on Linux: `sudo make release-sbc BOARD=orangepi5plus VERSION=0.1.2` (or `--base-image` with a vendor `.img` if the Armbian URL is missing).

RK3588 is mixed A55+A76; QEMU pins guests to one cluster. Raspberry Pi 5 is all A76 (prefer kernel 6.6).

**x86_64 UEFI PC:**

```bash
make release-amd VERSION=0.1.0
sudo ./scripts/flash.sh --image release/pertisk-node-0.1.0-amd64.raw --disk /dev/sdX --yes
```

Then boot USB (UEFI) and `pertisk-install --disk /dev/nvme0n1 --yes`. Later upgrades: `sudo ./upgrade.sh`.

Admin password: `/etc/pertisk/admin`. Existing Ubuntu/Debian with KVM can still use `sudo ./upgrade.sh` instead of flashing.

See `node.txt`. Guests stay in `/var/lib/pertisk`.

Test the image in QEMU before flashing (AlmaLinux: `dnf install qemu-system-x86 edk2-ovmf`):

```bash
ip link add name br0 type bridge
ip link set br0 up
ip addr flush dev eth0
ip link set eth0 master br0
ip addr add 10.1.1.16/24 dev br0
ip route add default via 10.1.1.10 dev br0
./scripts/test-qemu.sh --bridge br0
```

The appliance opens a local root shell on its physical or QEMU serial console. Debian's interactive first-boot wizard is disabled; Pertisk initializes itself automatically. The Pertisk web account remains `admin`; its password is in `/etc/pertisk/admin`.

To test remote LAN access through a Proxmox bridge, run `sudo ./scripts/test-qemu.sh --bridge vmbr0`. The guest receives an address from the bridge's DHCP network.

Boot the USB in UEFI mode. List disks, then install to NVMe:

```bash
pertisk-install --list
pertisk-install --disk /dev/nvme0n1 --yes
```

Install refuses if `/dev/kvm` is missing. Optional cluster join: put `PERTISK_JOIN=http://<peer>:7480` in `/etc/pertisk/join` before first boot.
