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

# mkosi images ship trixie.sources; overlay also copies debian.sources.
rm -f /etc/apt/sources.list.d/trixie.sources /etc/apt/sources.list.d/debian-debug.sources
if [[ -f "$OVERLAY/etc/apt/sources.list.d/debian.sources" ]]; then
  mkdir -p /etc/apt/sources.list.d
  install -m 644 "$OVERLAY/etc/apt/sources.list.d/debian.sources" \
    /etc/apt/sources.list.d/debian.sources
fi

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
      DEBIAN_FRONTEND=noninteractive apt-get install -y qemu-system-arm qemu-efi-aarch64 ipxe-qemu qemu-utils \
        zsh git zsh-autosuggestions zsh-syntax-highlighting fonts-powerline || true
      ;;
    *)
      DEBIAN_FRONTEND=noninteractive apt-get install -y qemu-system-x86 ovmf qemu-utils \
        zsh git zsh-autosuggestions zsh-syntax-highlighting fonts-powerline || true
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

# Orange Pi / Armbian / Raspberry Pi OS use NetworkManager. Copying the
# mkosi systemd-networkd overlay there takes the LAN NIC away from NM.
if [[ -d /etc/NetworkManager || -x /usr/bin/nmcli ]]; then
  rsync -a \
    --exclude 'etc/systemd/network/' \
    --exclude 'usr/lib/systemd/system-preset/50-pertisk.preset' \
    "$OVERLAY/" /
  install -m 644 "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk-sbc.preset" \
    /usr/lib/systemd/system-preset/50-pertisk-sbc.preset
else
  cp -a "$OVERLAY/." /
fi
chmod 755 /usr/sbin/pertisk-kvm-check /usr/sbin/pertisk-firstboot \
  /usr/sbin/pertisk-install /usr/sbin/pertisk-host-bridge /usr/sbin/pertisk-bootfix \
  /usr/sbin/pertisk-esp-boot /usr/sbin/pertisk-net \
  /usr/sbin/pertisk-console /usr/sbin/pertisk-fix-nvme-boot /usr/sbin/pertisk-uefi-register \
  /usr/sbin/pertisk-zsh-setup /usr/sbin/pertisk-fix-hosts /usr/sbin/pertisk-apt-bootstrap \
  /usr/sbin/pertisk-fix-dns
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

if [[ -x /usr/sbin/pertisk-fix-hosts ]]; then
  /usr/sbin/pertisk-fix-hosts || echo "install-node: hosts fix skipped" >&2
fi
if [[ -x /usr/sbin/pertisk-apt-bootstrap ]]; then
  /usr/sbin/pertisk-apt-bootstrap --sources-only \
    || echo "install-node: apt sources cleanup skipped" >&2
fi
if [[ -x /usr/sbin/pertisk-fix-dns ]]; then
  /usr/sbin/pertisk-fix-dns || echo "install-node: dns fix skipped" >&2
fi
if [[ -x /usr/sbin/pertisk-zsh-setup ]]; then
  /usr/sbin/pertisk-zsh-setup || echo "install-node: zsh setup skipped" >&2
fi
if [[ -x /usr/sbin/pertisk-apt-bootstrap && ! -x /usr/bin/apt-get && ! -x /bin/apt-get ]]; then
  /usr/sbin/pertisk-apt-bootstrap || echo "install-node: apt bootstrap skipped" >&2
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
