#!/usr/bin/env bash
# Boot the Pertisk raw installer image in QEMU with UEFI and a serial console.
# Usage: ./scripts/test-qemu.sh [--image out/pertisk-node.raw] [--memory 4096] [--cpus 4] [--no-kvm]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="$ROOT/out/pertisk-node.raw"
MEMORY=4096
CPUS=4
KVM=auto

usage() {
  cat <<'EOF'
Usage: ./scripts/test-qemu.sh [options]

Options:
  --image PATH    Raw image to boot (default: out/pertisk-node.raw)
  --memory MIB    Guest memory in MiB (default: 4096)
  --cpus COUNT    Virtual CPUs (default: 4)
  --no-kvm        Disable KVM acceleration
  -h, --help      Show this help

The guest is attached to QEMU user-mode networking and receives DHCP.
Exit QEMU with Ctrl-a x from the serial console.
EOF
}

die() { echo "test-qemu: $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="$2"; shift 2 ;;
    --memory) MEMORY="$2"; shift 2 ;;
    --cpus) CPUS="$2"; shift 2 ;;
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

find_ovmf() {
  local code vars
  for code in \
    /usr/share/edk2/ovmf/OVMF_CODE.fd \
    /usr/share/edk2/ovmf/OVMF_CODE.secboot.fd \
    /usr/share/OVMF/OVMF_CODE.fd; do
    vars="${code/OVMF_CODE/OVMF_VARS}"
    [[ -f "$code" && -f "$vars" ]] || continue
    printf '%s\n%s\n' "$code" "$vars"
    return 0
  done
  return 1
}

mapfile -t ovmf < <(find_ovmf) || die "install edk2-ovmf (Fedora/AlmaLinux) or ovmf (Debian/Ubuntu)"
OVMF_CODE="${ovmf[0]}"
OVMF_VARS_TEMPLATE="${ovmf[1]}"
RUNTIME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pertisk-qemu.XXXXXX")"
OVMF_VARS="$RUNTIME_DIR/OVMF_VARS.fd"
cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS"
trap 'rm -rf "$RUNTIME_DIR"' EXIT

accel=()
if [[ "$KVM" == "auto" && -r /dev/kvm && -w /dev/kvm ]]; then
  accel=(-enable-kvm -cpu host)
else
  echo "test-qemu: KVM unavailable; booting with software emulation" >&2
fi

echo "test-qemu: booting $IMAGE"
echo "test-qemu: serial console is active; exit QEMU with Ctrl-a x"
exec qemu-system-x86_64 \
  -machine q35,accel=kvm:tcg \
  "${accel[@]}" \
  -m "$MEMORY" \
  -smp "$CPUS" \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file="$OVMF_VARS" \
  -drive if=virtio,format=raw,file="$IMAGE" \
  -nic user,model=virtio-net-pci \
  -nographic
