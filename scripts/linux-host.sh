#!/usr/bin/env bash
# One-time prep for a Linux KVM test node (e.g. Ryzen 16c / 64G / 2T).
# Usage: ./scripts/linux-host.sh
# Then: PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-iso-guest.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HOME_DIR="${PERTISK_HOME:-$HOME/.pertisk}"
CACHE="$HOME_DIR/images"
LISTEN="${PERTISK_LISTEN:-0.0.0.0:7480}"

die() { echo "linux-host: $*" >&2; exit 1; }

if [[ "$(uname -s)" != "Linux" ]]; then
  die "run this on Linux with KVM. This host is $(uname -s) (mock driver only)."
fi
[[ -e /dev/kvm ]] || die "/dev/kvm is missing (enable SVM/VT-x in firmware, load kvm_amd or kvm_intel)."
command -v curl >/dev/null || die "curl not in PATH"
command -v cargo >/dev/null || die "cargo not in PATH (install rustup: https://rustup.rs)"

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"

mkdir -p "$CACHE" "$HOME_DIR"

echo "=== host ==="
echo "kernel    $(uname -r)"
echo "cpu       $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | sed 's/^ //')"
echo "threads   $(nproc)"
echo "memory    $(awk '/MemTotal/ {printf "%.0f GiB\n", $2/1024/1024}' /proc/meminfo)"
echo "kvm       /dev/kvm $(ls -l /dev/kvm | awk '{print $1,$3,$4}')"
grep -E -q 'svm|vmx' /proc/cpuinfo || echo "warn: no svm/vmx in /proc/cpuinfo"
command -v qemu-img >/dev/null && echo "qemu-img  $(command -v qemu-img)" || echo "qemu-img  not found (optional; raw volumes still work)"
command -v ip >/dev/null || echo "warn: ip (iproute2) missing; bridge/TAP will stay inventory-only"

ensure_cloud_hypervisor
ensure_firmware
echo "ch        $(command -v cloud-hypervisor)"
echo "firmware  $FIRMWARE"

echo "building pertiskd and pertisk"
cargo build -q -p pertisk-daemon -p pertisk-cli

cfg="$HOME_DIR/config.toml"
if [[ ! -f "$cfg" ]]; then
  cat >"$cfg" <<EOF
[daemon]
listen = "$LISTEN"

[vmm]
driver = "cloud-hypervisor"
run_dir = "$HOME_DIR/run"
cloud_hypervisor = "$(command -v cloud-hypervisor)"
firmware = "$FIRMWARE"

[storage]
root = "$HOME_DIR/storage"
backend = "replica"
replica_count = 1

[network]
apply_host_links = true

[cluster]
name = "pertisk"
node_name = "$(hostname -s 2>/dev/null || echo node-1)"
EOF
  echo "wrote $cfg (listen $LISTEN, replica_count 1, host links on)"
else
  echo "keeping existing $cfg"
fi

echo
echo "=== next ==="
echo "  export PATH=\"$ROOT/target/debug:\$PATH\""
echo "  PERTISK_ADMIN_PASSWORD=admin pertiskd --listen $LISTEN --driver cloud-hypervisor --firmware $FIRMWARE"
echo "  pertisk login -u admin -p admin"
echo "  # ISO from file (Alpine virt uses serial; Ubuntu/Windows graphical installers need VGA/QEMU):"
echo "  pertisk iso import /path/to/alpine-virt.iso"
echo "  pertisk vm create --name alpine --cpus 4 --memory 4096 --iso alpine-virt.iso --disk-size 32G --start"
echo "  pertisk vm console <id> --attach"
echo "UI: http://<this-host>:7480/  (Storage → Import ISO, then Guests → Create)"
echo
echo "Or one-shot Alpine ISO guest:"
echo "  PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-iso-guest.sh"
