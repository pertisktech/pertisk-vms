#!/usr/bin/env bash
# Install or upgrade this Linux machine as a pertisk node (systemd).
# Re-run after git pull to upgrade; guests under /var/lib/pertisk are kept.
# Usage: sudo ./scripts/install-node.sh
#        sudo ./upgrade.sh   # same thing from repo root
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

# The web UI is compiled into pertiskd by rust-embed, so it must be built first.
if command -v npm >/dev/null 2>&1; then
  echo "building web ui"
  (cd "$ROOT/web/ui" && npm ci --no-audit --no-fund && npm run build)
else
  echo "npm not found; keeping the checked-in web/ui build in crates/pertisk-daemon/static" >&2
fi

echo "building release pertiskd + pertisk + pertisk-tui"
cargo build --release -p pertisk-daemon -p pertisk-cli -p pertisk-tui

if command -v apt-get >/dev/null 2>&1; then
  case "$(uname -m)" in
    aarch64|arm64)
      DEBIAN_FRONTEND=noninteractive apt-get install -y qemu-system-arm qemu-efi-aarch64 ipxe-qemu qemu-utils || true
      ;;
    *)
      DEBIAN_FRONTEND=noninteractive apt-get install -y qemu-system-x86 ovmf qemu-utils || true
      ;;
  esac
fi

ensure_cloud_hypervisor
ensure_firmware

install -d /usr/bin /usr/sbin /usr/lib/systemd/system /usr/lib/cloud-hypervisor /etc/pertisk /var/lib/pertisk
install -m 755 "$ROOT/target/release/pertiskd" /usr/bin/pertiskd
install -m 755 "$ROOT/target/release/pertisk" /usr/bin/pertisk
install -m 755 "$ROOT/target/release/pertisk-tui" /usr/bin/pertisk-tui
ch_src="$(command -v cloud-hypervisor)"
if [[ "$(readlink -f "$ch_src")" != "$(readlink -f /usr/bin/cloud-hypervisor)" ]]; then
  install -m 755 "$ch_src" /usr/bin/cloud-hypervisor
fi
install -m 644 "$FIRMWARE" /usr/lib/cloud-hypervisor/hypervisor-fw

cp -a "$OVERLAY/." /
chmod 755 /usr/sbin/pertisk-kvm-check /usr/sbin/pertisk-firstboot /usr/sbin/pertisk-install
chmod 644 /etc/pertisk/config.toml /etc/pertisk/daemon.env
chmod 755 /etc/pertisk

# Always refresh node config from overlay so driver defaults stay current.
cp /etc/pertisk/config.toml /var/lib/pertisk/config.toml
# Keep existing admin password in daemon.env if present; otherwise use overlay.
if [[ -f /etc/pertisk/daemon.env ]]; then
  # Prefer qemu for VGA; override legacy cloud-hypervisor env.
  if grep -q '^PERTISK_DRIVER=' /etc/pertisk/daemon.env; then
    sed -i 's/^PERTISK_DRIVER=.*/PERTISK_DRIVER=qemu/' /etc/pertisk/daemon.env
  else
    printf 'PERTISK_DRIVER=qemu\n' >>/etc/pertisk/daemon.env
  fi
fi

systemctl daemon-reload
systemctl enable pertisk-firstboot.service pertiskd.service
systemctl restart pertisk-firstboot.service || true
systemctl restart pertiskd.service || true

echo
echo "=== installed ==="
systemctl --no-pager --full status pertiskd.service || true
echo "UI: https://$(hostname -I 2>/dev/null | awk '{print $1}'):7443/"
echo "Admin password: /etc/pertisk/admin (after firstboot) or PERTISK_ADMIN_PASSWORD in /etc/pertisk/daemon.env"
