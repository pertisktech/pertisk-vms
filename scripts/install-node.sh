#!/usr/bin/env bash
# Turn this Linux machine into a pertisk node (systemd). No ISO required.
# Usage: sudo ./scripts/install-node.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"

die() { echo "install-node: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || die "Linux only"
[[ "$(id -u)" -eq 0 ]] || die "run as root (sudo $0)"
command -v cargo >/dev/null || die "cargo not in PATH"
[[ -d "$OVERLAY" ]] || die "missing $OVERLAY"

# shellcheck source=lib.sh
CACHE="${PERTISK_HOME:-/var/lib/pertisk}/images"
mkdir -p "$CACHE"
# lib.sh expects CACHE and die
source "$ROOT/scripts/lib.sh"

SKIP_KVM="${PERTISK_SKIP_KVM:-0}"
if [[ "$SKIP_KVM" != "1" ]]; then
  bash "$OVERLAY/usr/sbin/pertisk-kvm-check" || die "KVM not usable (set PERTISK_SKIP_KVM=1 to package only)"
fi

echo "building release pertiskd + pertisk"
cargo build --release -p pertisk-daemon -p pertisk-cli

ensure_cloud_hypervisor
ensure_firmware

install -d /usr/bin /usr/sbin /usr/lib/systemd/system /usr/lib/cloud-hypervisor /etc/pertisk /var/lib/pertisk
install -m 755 "$ROOT/target/release/pertiskd" /usr/bin/pertiskd
install -m 755 "$ROOT/target/release/pertisk" /usr/bin/pertisk
install -m 755 "$(command -v cloud-hypervisor)" /usr/bin/cloud-hypervisor
install -m 644 "$FIRMWARE" /usr/lib/cloud-hypervisor/hypervisor-fw

cp -a "$OVERLAY/." /
chmod 755 /usr/sbin/pertisk-kvm-check /usr/sbin/pertisk-firstboot /usr/sbin/pertisk-install
chmod 644 /etc/pertisk/config.toml /etc/pertisk/daemon.env
chmod 755 /etc/pertisk

if [[ ! -f /var/lib/pertisk/config.toml ]]; then
  cp /etc/pertisk/config.toml /var/lib/pertisk/config.toml
fi

systemctl daemon-reload
systemctl enable pertisk-firstboot.service pertiskd.service
systemctl restart pertisk-firstboot.service || true
systemctl restart pertiskd.service || true

echo
echo "=== installed ==="
systemctl --no-pager --full status pertiskd.service || true
echo "UI: http://$(hostname -I 2>/dev/null | awk '{print $1}'):7480/"
echo "Admin password: /etc/pertisk/admin (after firstboot) or PERTISK_ADMIN_PASSWORD in /etc/pertisk/daemon.env"
