#!/usr/bin/env bash
# Build a bootable, flashable raw disk image (mkosi) on Linux. Output: out/pertisk-node.raw
# Usage: ./scripts/build-iso.sh
# Flash image: sudo ./scripts/flash.sh --image out/pertisk-node.raw --disk /dev/sdX --yes
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"
OUT="$ROOT/out"
FORMAT="${PERTISK_IMAGE_FORMAT:-disk}"

die() { echo "build-iso: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || die "build the image on Linux (this host is $(uname -s))"
command -v cargo >/dev/null || die "cargo not in PATH"
command -v mkosi >/dev/null || die "install mkosi (https://github.com/systemd/mkosi). Debian: apt install mkosi"
command -v curl >/dev/null || die "curl not in PATH"
command -v npm >/dev/null || die "npm not in PATH (install Node.js to build the embedded web UI)"
[[ "$FORMAT" == "disk" ]] || die "this script builds a raw disk image only"

# shellcheck source=lib.sh
CACHE="${PERTISK_HOME:-$HOME/.pertisk}/images"
mkdir -p "$CACHE" "$OUT" "$OVERLAY/usr/bin" "$OVERLAY/usr/lib/cloud-hypervisor"
source "$ROOT/scripts/lib.sh"

echo "building web ui"
(cd "$ROOT/web/ui" && npm ci --no-audit --no-fund && npm run build)

echo "building release binaries"
cargo build --release -p pertisk-daemon -p pertisk-cli
install -m 755 "$ROOT/target/release/pertiskd" "$OVERLAY/usr/bin/pertiskd"
install -m 755 "$ROOT/target/release/pertisk" "$OVERLAY/usr/bin/pertisk"

ensure_cloud_hypervisor
ensure_firmware
install -m 755 "$(command -v cloud-hypervisor)" "$OVERLAY/usr/bin/cloud-hypervisor"
install -m 644 "$FIRMWARE" "$OVERLAY/usr/lib/cloud-hypervisor/hypervisor-fw"
chmod 755 "$OVERLAY/usr/sbin/pertisk-kvm-check" "$OVERLAY/usr/sbin/pertisk-firstboot" "$OVERLAY/usr/sbin/pertisk-install"

echo "mkosi format=$FORMAT (needs root for the image)"
( cd "$ROOT/iso" && mkosi --force --format "$FORMAT" )

echo
echo "=== image ==="
ls -lh "$OUT" 2>/dev/null || ls -lh "$ROOT/iso"/pertisk-node* 2>/dev/null || true
echo "Flash: sudo ./scripts/flash.sh --image out/pertisk-node.raw --disk /dev/sdX --yes"
echo "Then boot USB. To install to NVMe: pertisk-install --list && pertisk-install --disk /dev/nvme0n1 --yes"
