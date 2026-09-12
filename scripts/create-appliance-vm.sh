#!/usr/bin/env bash
# Create the lab Pertisk appliance VM on this Proxmox host from out/pertisk-node.raw.
# Usage: ./scripts/create-appliance-vm.sh [VMID] [DISK_GIB]
# Example: ./scripts/create-appliance-vm.sh 901 64
#
# After this, ./deploy.sh copies current binaries into the guest and starts it.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VMID="${1:-901}"
DISK_GIB="${2:-64}"
IMAGE="${PERTISK_IMAGE:-$ROOT/out/pertisk-node.raw}"
STORAGE="${PERTISK_STORAGE:-local-zfs}"
BRIDGE="${PERTISK_BRIDGE:-vmbr0}"
MEMORY_MIB="${PERTISK_MEMORY:-16384}"
CORES="${PERTISK_CORES:-8}"
NAME="${PERTISK_NAME:-pertisk}"
ZVOL="/dev/zvol/rpool/data/vm-${VMID}-disk-0"

die() { echo "create-appliance-vm: $*" >&2; exit 1; }

[[ "$(id -u)" -eq 0 ]] || die "run as root on the Proxmox host"
command -v qm >/dev/null || die "qm not found (run on Proxmox)"
[[ -f "$IMAGE" ]] || die "image not found: $IMAGE (run ./scripts/build-iso.sh amd64 first)"
[[ "$VMID" =~ ^[1-9][0-9]*$ ]] || die "VMID must be a positive integer"
[[ "$DISK_GIB" =~ ^[1-9][0-9]*$ ]] || die "DISK_GIB must be a positive integer"
[[ "$DISK_GIB" -ge 13 ]] || die "DISK_GIB must be at least 13 (image is ~12.5G)"
pvesm status --storage "$STORAGE" >/dev/null || die "storage not found: $STORAGE"
ip link show dev "$BRIDGE" >/dev/null 2>&1 || die "bridge not found: $BRIDGE"

if qm status "$VMID" >/dev/null 2>&1; then
  die "VM $VMID already exists — destroy it first if you want to recreate (qm destroy $VMID --purge)"
fi

echo "create-appliance-vm: creating VM $VMID ($NAME) on $STORAGE / $BRIDGE"
qm create "$VMID" \
  --name "$NAME" \
  --memory "$MEMORY_MIB" \
  --cores "$CORES" \
  --sockets 1 \
  --cpu host \
  --machine q35 \
  --bios ovmf \
  --scsihw virtio-scsi-single \
  --net0 "virtio,bridge=${BRIDGE}" \
  --serial0 socket \
  --vga std \
  --ostype l26 \
  --agent enabled=1 \
  --onboot 1 \
  --hotplug disk,network,usb \
  --description "Pertisk appliance. Nested KVM (cpu=host). Deploy with ./deploy.sh"

echo "create-appliance-vm: importing $IMAGE (this takes a minute)"
qm importdisk "$VMID" "$IMAGE" "$STORAGE"

echo "create-appliance-vm: attaching OS disk as scsi0 (must stay vm-${VMID}-disk-0 for deploy.sh)"
qm set "$VMID" --scsi0 "${STORAGE}:vm-${VMID}-disk-0,discard=on,ssd=1" --boot order=scsi0

# EFI disk is created after the OS disk so zvol disk-0 stays the root image.
echo "create-appliance-vm: adding EFI vars (Secure Boot off)"
qm set "$VMID" --efidisk0 "${STORAGE}:1,efitype=4m,pre-enrolled-keys=0"

if [[ "$DISK_GIB" -gt 13 ]]; then
  echo "create-appliance-vm: growing scsi0 to ${DISK_GIB}G"
  qm resize "$VMID" scsi0 "${DISK_GIB}G"
fi

[[ -b "$ZVOL" ]] || die "imported disk missing: $ZVOL"
udevadm settle --timeout=10 >/dev/null 2>&1 || true
partprobe "$ZVOL" >/dev/null 2>&1 || true
sleep 1

# Move GPT backup to the new end, then stretch partition 2 to fill the disk.
if command -v sgdisk >/dev/null; then
  sgdisk -e "$ZVOL" >/dev/null || true
  sgdisk -d 2 -n 2:1050624:0 -t 2:8304 -c 2:root-x86-64 "$ZVOL" >/dev/null
elif command -v parted >/dev/null; then
  printf 'Fix\nYes\n' | parted ---pretend-input-tty "$ZVOL" resizepart 2 100% >/dev/null || \
    parted -s "$ZVOL" resizepart 2 100%
else
  die "sgdisk or parted is required to grow the root partition"
fi
partprobe "$ZVOL" >/dev/null 2>&1 || true
udevadm settle --timeout=10 >/dev/null 2>&1 || true
sleep 1

ROOT_PART="${ZVOL}-part2"
[[ -b "$ROOT_PART" ]] || die "root partition missing after grow: $ROOT_PART"
e2fsck -f -y "$ROOT_PART"
resize2fs "$ROOT_PART"

echo "create-appliance-vm: VM $VMID ready (stopped). Next: ./deploy.sh"
echo "  UI after start: https://<guest-dhcp-ip>:7443/  admin / admin"
echo "  Nested guests need cpu=host (already set) and host nested=Y."
