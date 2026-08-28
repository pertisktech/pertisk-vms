#!/usr/bin/env bash
# Build a flashable ISO (mkosi) on Linux. Output: out/pertisk-node.iso (or .raw if FORMAT=disk)
# Usage: ./scripts/build-iso.sh
#        PERTISK_IMAGE_FORMAT=disk ./scripts/build-iso.sh
# Flash ISO: sudo ./scripts/flash.sh --image out/pertisk-node.iso --disk /dev/sdX --yes
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"
OUT="$ROOT/out"
FORMAT="${PERTISK_IMAGE_FORMAT:-iso}"

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

echo "mkosi format=$FORMAT (needs root for the image)"
if ! ( cd "$ROOT/iso" && mkosi --force --format "$FORMAT" ); then
  if [[ "$FORMAT" == "iso" ]]; then
    echo "mkosi --format iso failed; retrying as disk image" >&2
    ( cd "$ROOT/iso" && mkosi --force --format disk )
  else
    exit 1
  fi
fi

echo
echo "=== image ==="
ls -lh "$OUT" 2>/dev/null || ls -lh "$ROOT/iso"/pertisk-node* 2>/dev/null || true
echo "Flash: sudo ./scripts/flash.sh --image out/pertisk-node.iso --disk /dev/sdX --yes"
echo "Then boot USB. To install to NVMe: pertisk-install --list && pertisk-install --disk /dev/nvme0n1 --yes"
