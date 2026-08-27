#!/usr/bin/env bash
# v0.1 gate: boot Alpine via Cloud Hypervisor on a Linux KVM host.
# Usage (from repo root): PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-guest.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HOME_DIR="${PERTISK_HOME:-$HOME/.pertisk}"
CACHE="$HOME_DIR/images"
ALPINE_VER="${ALPINE_VER:-v3.21}"
ALPINE_REL="${ALPINE_REL:-3.21.3}"
URL="${PERTISK_URL:-http://127.0.0.1:7480}"
NAME="${GUEST_NAME:-alpine-$$}"
ISO_NAME="${ISO_NAME:-alpine-virt.iso}"

die() { echo "linux-guest: $*" >&2; exit 1; }

if [[ "$(uname -s)" != "Linux" ]]; then
  die "run this on Linux with KVM. This host is $(uname -s) (mock driver only)."
fi
[[ -e /dev/kvm ]] || die "/dev/kvm is missing (enable virtualization / nested KVM)."
command -v curl >/dev/null || die "curl not in PATH"
command -v cargo >/dev/null || die "cargo not in PATH (install rustup: https://rustup.rs)"

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
mkdir -p "$CACHE"
ensure_cloud_hypervisor

arch="$(uname -m)"
case "$arch" in
  x86_64|aarch64) ;;
  arm64) arch=aarch64 ;;
  *) die "unsupported arch $arch (need x86_64 or aarch64)" ;;
esac

listen="${URL#http://}"
listen="${listen#https://}"

iso_url="${ALPINE_ISO_URL:-https://dl-cdn.alpinelinux.org/alpine/${ALPINE_VER}/releases/${arch}/alpine-virt-${ALPINE_REL}-${arch}.iso}"
mkdir -p "$CACHE"
iso="$CACHE/$ISO_NAME"
kernel="$CACHE/iso-boot/vmlinuz-virt"
initramfs="$CACHE/iso-boot/initramfs-virt"
if [[ ! -s "$iso" ]]; then
  echo "fetching $iso_url"
  curl -fsSL -o "$iso" "$iso_url"
fi
if [[ ! -s "$kernel" || ! -s "$initramfs" || "$iso" -nt "$kernel" ]]; then
  echo "extracting kernel/initramfs from $ISO_NAME"
  mnt="$(mktemp -d)"
  mount -o loop,ro "$iso" "$mnt" || die "loop-mount ISO failed (need root and the iso9660 module)"
  mkdir -p "$CACHE/iso-boot"
  cp -f "$mnt/boot/vmlinuz-virt" "$kernel"
  cp -f "$mnt/boot/initramfs-virt" "$initramfs"
  umount "$mnt"
  rmdir "$mnt"
fi
[[ -s "$kernel" && -s "$initramfs" ]] || die "ISO is missing boot/vmlinuz-virt or boot/initramfs-virt"

echo "building pertiskd and pertisk"
cargo build -q -p pertisk-daemon -p pertisk-cli
pertisk="$ROOT/target/debug/pertisk"
pertiskd="$ROOT/target/debug/pertiskd"

started_daemon=0
if ! curl -fsS "$URL/v1/health" >/dev/null 2>&1; then
  echo "starting pertiskd on $URL"
  PERTISK_ADMIN_PASSWORD="${PERTISK_ADMIN_PASSWORD:-admin}" \
    "$pertiskd" --listen "$listen" --driver cloud-hypervisor &
  started_daemon=1
  trap 'if [[ "$started_daemon" -eq 1 ]]; then kill %1 2>/dev/null || true; fi' EXIT
  ready=0
  for _ in $(seq 1 80); do
    if curl -fsS "$URL/v1/health" >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 0.25
  done
  [[ "$ready" -eq 1 ]] || die "pertiskd did not become ready at $URL"
fi

"$pertisk" --url "$URL" login -u "${PERTISK_JOIN_USER:-admin}" -p "${PERTISK_ADMIN_PASSWORD:-admin}"
host="$("$pertisk" --url "$URL" host)"
echo "$host"
echo "$host" | grep -q 'kvm[[:space:]]*true' || die "daemon reports kvm=false"
echo "$host" | grep -q 'driver[[:space:]]*cloud-hypervisor' || die "daemon is not using cloud-hypervisor"

if ! "$pertisk" --url "$URL" iso list | grep -q "^${ISO_NAME}[[:space:]]"; then
  "$pertisk" --url "$URL" iso import "$iso" --name "$ISO_NAME"
fi

created="$("$pertisk" --url "$URL" vm create --name "$NAME" --cpus 1 --memory 512 \
  --kernel "$kernel" --initramfs "$initramfs" \
  --cmdline "console=ttyS0,115200 modules=loop,squashfs,virtio_blk alpine_dev=/dev/vda:iso9660 modloop=/boot/modloop-virt")"
echo "$created"
id="$(echo "$created" | awk '{print $1}')"
[[ -n "$id" ]] || die "vm create returned no id"
"$pertisk" --url "$URL" vm cdrom attach --iso "$ISO_NAME" "$id"
"$pertisk" --url "$URL" vm start "$id"
echo "guest $NAME ($id) started. serial (Ctrl-C detaches, guest keeps running):"
echo "Alpine live login is root with an empty password."
trap - EXIT
exec "$pertisk" --url "$URL" vm console "$id" --attach
