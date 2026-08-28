#!/usr/bin/env bash
# Build a flashable disk image (mkosi) on Linux. Output: out/pertisk-node.raw
# Usage: ./scripts/build-iso.sh
# Flash: sudo dd if=out/pertisk-node.raw of=/dev/sdX bs=4M status=progress conv=fsync
# Live install to internal disk after booting that image: pertisk-install --disk /dev/nvme0n1 --yes
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"
OUT="$ROOT/out"

die() { echo "build-iso: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || die "build the image on Linux (this host is $(uname -s))"
command -v cargo >/dev/null || die "cargo not in PATH"
command -v mkosi >/dev/null || die "install mkosi (https://github.com/systemd/mkosi). Debian: apt install mkosi"
command -v curl >/dev/null || die "curl not in PATH"

# shellcheck source=lib.sh
CACHE="${PERTISK_HOME:-$HOME/.pertisk}/images"
mkdir -p "$CACHE" "$OUT" "$OVERLAY/usr/bin" "$OVERLAY/usr/lib/cloud-hypervisor"
source "$ROOT/scripts/lib.sh"

echo "building release binaries"
cargo build --release -p pertisk-daemon -p pertisk-cli
install -m 755 "$ROOT/target/release/pertiskd" "$OVERLAY/usr/bin/pertiskd"
install -m 755 "$ROOT/target/release/pertisk" "$OVERLAY/usr/bin/pertisk"

ensure_cloud_hypervisor
ensure_firmware
install -m 755 "$(command -v cloud-hypervisor)" "$OVERLAY/usr/bin/cloud-hypervisor"
install -m 644 "$FIRMWARE" "$OVERLAY/usr/lib/cloud-hypervisor/hypervisor-fw"
chmod 755 "$OVERLAY/usr/sbin/pertisk-kvm-check" "$OVERLAY/usr/sbin/pertisk-firstboot" "$OVERLAY/usr/sbin/pertisk-install"

echo "mkosi (needs root for the image)"
( cd "$ROOT/iso" && mkosi --force )

echo
echo "=== image ==="
ls -lh "$OUT" || ls -lh "$ROOT/iso"/pertisk-node* || true
echo "Flash the .raw/.disk image to USB, boot it, then either use it as the OS disk"
echo "or run: pertisk-install --disk /dev/nvme0n1 --yes"
