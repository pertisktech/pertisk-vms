#!/usr/bin/env bash
# v1.0 gate: Alpine ISO + disk via Cloud Hypervisor firmware on Linux KVM.
# Usage: PERTISK_ADMIN_PASSWORD=admin ./scripts/linux-iso-guest.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HOME_DIR="${PERTISK_HOME:-$HOME/.pertisk}"
CACHE="$HOME_DIR/images"
ALPINE_VER="${ALPINE_VER:-v3.21}"
ALPINE_REL="${ALPINE_REL:-3.21.3}"
URL="${PERTISK_URL:-http://127.0.0.1:7480}"
NAME="${GUEST_NAME:-alpine-iso-$$}"
ISO_NAME="${ISO_NAME:-alpine-virt.iso}"

die() { echo "linux-iso-guest: $*" >&2; exit 1; }

if [[ "$(uname -s)" != "Linux" ]]; then
  die "run this on Linux with KVM. This host is $(uname -s) (mock driver only)."
fi
[[ -e /dev/kvm ]] || die "/dev/kvm is missing (enable virtualization / nested KVM)."
command -v curl >/dev/null || die "curl not in PATH"
command -v cargo >/dev/null || die "cargo not in PATH (install rustup: https://rustup.rs)"

# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"

arch="$(uname -m)"
case "$arch" in
  x86_64)
    fw_asset="hypervisor-fw"
    iso_arch="x86_64"
    ;;
  aarch64|arm64)
    arch=aarch64
    fw_asset="hypervisor-fw-aarch64"
    iso_arch="aarch64"
    ;;
  *) die "unsupported arch $arch (need x86_64 or aarch64)" ;;
esac

listen="${URL#http://}"
listen="${listen#https://}"
mkdir -p "$CACHE"
ensure_cloud_hypervisor

fw="$CACHE/hypervisor-fw"
if [[ ! -s "$fw" ]]; then
  echo "fetching rust-hypervisor-firmware 0.5.0 ($fw_asset)"
  curl -fsSL -o "$fw" \
    "https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/0.5.0/${fw_asset}"
fi

iso="$CACHE/$ISO_NAME"
if [[ ! -s "$iso" ]]; then
  iso_url="${ALPINE_ISO_URL:-https://dl-cdn.alpinelinux.org/alpine/${ALPINE_VER}/releases/${iso_arch}/alpine-virt-${ALPINE_REL}-${iso_arch}.iso}"
  echo "fetching $iso_url"
  curl -fsSL -o "$iso" "$iso_url"
fi

echo "building pertiskd and pertisk"
cargo build -q -p pertisk-daemon -p pertisk-cli
pertisk="$ROOT/target/debug/pertisk"
pertiskd="$ROOT/target/debug/pertiskd"

if curl -fsS "$URL/v1/health" >/dev/null 2>&1; then
  echo "restarting pertiskd at $URL so this build is used"
  pkill -x pertiskd 2>/dev/null || true
  for _ in $(seq 1 40); do
    curl -fsS "$URL/v1/health" >/dev/null 2>&1 || break
    sleep 0.25
  done
fi

started_daemon=0
echo "starting pertiskd on $URL"
PERTISK_ADMIN_PASSWORD="${PERTISK_ADMIN_PASSWORD:-admin}" \
  "$pertiskd" --listen "$listen" --driver cloud-hypervisor --firmware "$fw" &
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

"$pertisk" --url "$URL" login -u "${PERTISK_JOIN_USER:-admin}" -p "${PERTISK_ADMIN_PASSWORD:-admin}"
explain_cli
host="$("$pertisk" --url "$URL" host)"
echo "$host"
echo "$host" | grep -q 'kvm[[:space:]]*true' || die "daemon reports kvm=false"
echo "$host" | grep -q 'driver[[:space:]]*cloud-hypervisor' || die "daemon is not using cloud-hypervisor"
echo "$host" | grep -q 'firmware[[:space:]]*not found' && die "no firmware (pass --firmware or install hypervisor-fw)"
show_capacity

if ! "$pertisk" --url "$URL" iso list | grep -q "^${ISO_NAME}[[:space:]]"; then
  "$pertisk" --url "$URL" iso import "$iso" --name "$ISO_NAME"
fi

vol="$("$pertisk" --url "$URL" vol create --name "${NAME}-disk" --size 2G)"
echo "$vol"
vol_id="$(echo "$vol" | awk '{print $1}')"
if ! created="$("$pertisk" --url "$URL" vm create --name "$NAME" --cpus 1 --memory 512 --firmware "$fw")"; then
  echo "$created" >&2
  show_capacity
  die "vm create failed. Stop leftover guests: $pertisk --url $URL vm stop <id>"
fi
echo "$created"
id="$(echo "$created" | awk '{print $1}')"
[[ -n "$id" && -n "$vol_id" ]] || die "create returned no id"
"$pertisk" --url "$URL" vm disk attach --volume "$vol_id" "$id"
"$pertisk" --url "$URL" vm cdrom attach --iso "$ISO_NAME" "$id"
"$pertisk" --url "$URL" vm start "$id"
echo "guest $NAME ($id) started from ISO. serial (Ctrl-C detaches):"
trap - EXIT
exec "$pertisk" --url "$URL" vm console "$id" --attach
