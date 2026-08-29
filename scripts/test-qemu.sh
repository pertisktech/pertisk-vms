#!/usr/bin/env bash
# Boot the Pertisk raw installer image in QEMU with UEFI and a serial console.
# Usage: ./scripts/test-qemu.sh [--image out/pertisk-node.raw] [--memory 4096] [--cpus 4] [--bridge vmbr0] [--no-kvm]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="$ROOT/out/pertisk-node.raw"
MEMORY=4096
CPUS=4
KVM=auto
BRIDGE=""

usage() {
  cat <<'EOF'
Usage: ./scripts/test-qemu.sh [options]

Options:
  --image PATH    Raw image to boot (default: out/pertisk-node.raw)
  --memory MIB    Guest memory in MiB (default: 4096)
  --cpus COUNT    Virtual CPUs (default: 4)
  --bridge NAME   Attach a temporary TAP device to NAME for LAN DHCP access
  --no-kvm        Disable KVM acceleration
  -h, --help      Show this help

Without --bridge, the guest uses QEMU user-mode networking and receives DHCP.
Bridge mode requires root and lets the guest receive an address from the LAN.
Press Ctrl-C to stop QEMU from the serial console.
EOF
}

die() { echo "test-qemu: $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="$2"; shift 2 ;;
    --memory) MEMORY="$2"; shift 2 ;;
    --cpus) CPUS="$2"; shift 2 ;;
    --bridge) BRIDGE="$2"; shift 2 ;;
    --no-kvm) KVM=no; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

[[ "$(uname -s)" == "Linux" ]] || die "Linux only"
command -v qemu-system-x86_64 >/dev/null || die "install qemu-system-x86"
[[ -f "$IMAGE" ]] || die "image not found: $IMAGE (run ./scripts/build-iso.sh first)"
[[ "$MEMORY" =~ ^[1-9][0-9]*$ ]] || die "--memory must be a positive MiB value"
[[ "$CPUS" =~ ^[1-9][0-9]*$ ]] || die "--cpus must be a positive integer"
if [[ -n "$BRIDGE" ]]; then
  [[ "$BRIDGE" =~ ^[[:alnum:]_.-]+$ ]] || die "--bridge contains invalid characters"
  [[ "$(id -u)" -eq 0 ]] || die "--bridge requires root (run with sudo)"
  command -v ip >/dev/null || die "--bridge requires iproute2"
  ip link show dev "$BRIDGE" >/dev/null 2>&1 || die "bridge not found: $BRIDGE"
fi

find_ovmf() {
  local code vars
  while IFS=: read -r code vars; do
    [[ -f "$code" && -f "$vars" ]] || continue
    OVMF_CODE="$code"
    OVMF_VARS_TEMPLATE="$vars"
    return 0
  done <<'EOF'
/usr/share/pve-edk2-firmware/OVMF_CODE.fd:/usr/share/pve-edk2-firmware/OVMF_VARS.fd
/usr/share/pve-edk2-firmware/OVMF_CODE.secboot.fd:/usr/share/pve-edk2-firmware/OVMF_VARS.ms.fd
/usr/share/edk2/ovmf/OVMF_CODE.fd:/usr/share/edk2/ovmf/OVMF_VARS.fd
/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd:/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd
/usr/share/OVMF/OVMF_CODE.fd:/usr/share/OVMF/OVMF_VARS.fd
/usr/share/OVMF/OVMF_CODE_4M.fd:/usr/share/OVMF/OVMF_VARS_4M.fd
EOF
  return 1
}

find_ovmf || die "OVMF firmware not found; install edk2-ovmf (Fedora/AlmaLinux) or ovmf (Debian/Ubuntu)"
RUNTIME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pertisk-qemu.XXXXXX")"
OVMF_VARS="$RUNTIME_DIR/OVMF_VARS.fd"
cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS"
TAP=""
cleanup() {
  [[ -z "$TAP" ]] || ip link delete "$TAP" 2>/dev/null || true
  rm -rf "$RUNTIME_DIR"
}
trap cleanup EXIT INT TERM

accel=()
if [[ "$KVM" == "auto" && -r /dev/kvm && -w /dev/kvm ]]; then
  accel=(-enable-kvm -cpu host)
else
  echo "test-qemu: KVM unavailable; booting with software emulation" >&2
fi

network=(-nic user,model=virtio-net-pci)
if [[ -n "$BRIDGE" ]]; then
  TAP="pertiskq$(( $$ % 100000 ))"
  ip tuntap add dev "$TAP" mode tap
  ip link set dev "$TAP" master "$BRIDGE"
  ip link set dev "$TAP" up
  network=(-netdev "tap,id=net0,ifname=$TAP,script=no,downscript=no" -device virtio-net-pci,netdev=net0)
  echo "test-qemu: bridging guest to $BRIDGE through $TAP"
fi

echo "test-qemu: booting $IMAGE"
echo "test-qemu: serial console is active; press Ctrl-C to stop QEMU"
qemu-system-x86_64 \
  -machine q35,accel=kvm:tcg \
  "${accel[@]}" \
  -m "$MEMORY" \
  -smp "$CPUS" \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS" \
  -drive if=virtio,format=raw,file="$IMAGE" \
  "${network[@]}" \
  -display none \
  -monitor none \
  -serial stdio
