#!/usr/bin/env bash
# Build on Proxmox (or any Linux build host) and copy binaries into a pertisk appliance VM.
# Usage: ./scripts/deploy-appliance.sh [VMID] [MOUNT_OFFSET_SECTOR]
# Example: ./scripts/deploy-appliance.sh 901
#
# Only run when pertisk code changed — NOT after every guest reboot.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VMID="${1:-901}"
ROOT_PART_START="${2:-1050624}"
MNT="/mnt/pertisk-deploy-${VMID}"
ZVOL="/dev/zvol/rpool/data/vm-${VMID}-disk-0"

die() { echo "deploy-appliance: $*" >&2; exit 1; }

[[ "$(id -u)" -eq 0 ]] || die "run as root on the Proxmox host"
command -v qm >/dev/null || die "qm not found (run on Proxmox)"
[[ -b "$ZVOL" ]] || die "disk not found: $ZVOL (adjust VMID or storage pool name)"

cd "$ROOT"
echo "building release binaries"
cargo build --release -p pertisk-daemon -p pertisk-cli -p pertisk-tui

was_running=0
if qm status "$VMID" 2>/dev/null | grep -q running; then
  was_running=1
fi

echo "stopping VM $VMID"
qm stop "$VMID"
sleep 2

mkdir -p "$MNT"
mount -o "offset=$((ROOT_PART_START * 512))" "$ZVOL" "$MNT"

install -m 755 "$ROOT/target/release/pertiskd" "$MNT/usr/bin/pertiskd"
install -m 755 "$ROOT/target/release/pertisk" "$MNT/usr/bin/pertisk"
install -m 755 "$ROOT/target/release/pertisk-tui" "$MNT/usr/bin/pertisk-tui"

if [[ ! -f "$MNT/etc/fstab" ]]; then
  root_uuid="$(debugfs -R 'stats' "$ZVOL" 2>/dev/null | awk -F\" '/Filesystem UUID/ {print $2; exit}')"
  if [[ -z "$root_uuid" ]]; then
    root_uuid="$(tune2fs -l "$ZVOL" 2>/dev/null | awk '/Filesystem UUID/ {print $3}')"
  fi
  esp_uuid=""
  if command -v blkid >/dev/null; then
    mkdir -p /tmp/pertisk-esp-$$ 
    if mount -o "offset=$((2048 * 512)),sizelimit=$((1048576 * 512))" "$ZVOL" /tmp/pertisk-esp-$$ 2>/dev/null; then
      esp_uuid="$(blkid -s UUID -o value /tmp/pertisk-esp-$$ 2>/dev/null || true)"
      umount /tmp/pertisk-esp-$$
    fi
    rmdir /tmp/pertisk-esp-$$ 2>/dev/null || true
  fi
  if [[ -n "$root_uuid" ]]; then
    {
      echo "UUID=$root_uuid / ext4 defaults 0 1"
      [[ -n "$esp_uuid" ]] && echo "UUID=$esp_uuid /boot/efi vfat umask=0077 0 2"
    } >"$MNT/etc/fstab"
    echo "wrote $MNT/etc/fstab"
  fi
fi

cat >"$MNT/etc/motd" <<'EOF'
pertisk-vm node
Running from disk (no pertisk-install needed on Proxmox VM).
Console TUI:      pertisk-tui
UI:               http://<this-host>:7480/  user admin  password in /etc/pertisk/admin
EOF

umount "$MNT"
rmdir "$MNT"

echo "starting VM $VMID"
qm start "$VMID"

echo "deploy-appliance: done (VM $VMID). Reboots do not require redeploy unless you change code."
