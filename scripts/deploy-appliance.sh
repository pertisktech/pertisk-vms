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
if command -v npm >/dev/null 2>&1; then
  echo "building web ui"
  (cd "$ROOT/web/ui" && npm ci --no-audit --no-fund && npm run build)
fi
echo "building release binaries"
cargo build --release -p pertisk-daemon -p pertisk-cli -p pertisk-tui

was_running=0
if qm status "$VMID" 2>/dev/null | grep -q running; then
  was_running=1
fi

echo "stopping VM $VMID"
if qm status "$VMID" 2>/dev/null | grep -q running; then
  qm stop "$VMID"
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    qm status "$VMID" 2>/dev/null | grep -q stopped && break
    sleep 1
  done
fi
if qm status "$VMID" 2>/dev/null | grep -qv stopped; then
  die "VM $VMID did not stop — fix manually (qm stop $VMID) before deploying"
fi
sleep 1

# Drop stale host mounts from interrupted deploys (causes dirty FS / fsck failures).
for stale in "$MNT" /mnt/pertisk901; do
  if mountpoint -q "$stale" 2>/dev/null; then
    echo "unmounting stale mount $stale"
    sync
    umount "$stale"
  fi
done

mkdir -p "$MNT"
mount -o "offset=$((ROOT_PART_START * 512))" "$ZVOL" "$MNT"

install -m 755 "$ROOT/target/release/pertiskd" "$MNT/usr/bin/pertiskd"
install -m 755 "$ROOT/target/release/pertisk" "$MNT/usr/bin/pertisk"
install -m 755 "$ROOT/target/release/pertisk-tui" "$MNT/usr/bin/pertisk-tui"

# systemd units ship in the ISO overlay (keep console free of journal spam for TUI).
for unit in pertiskd.service pertisk-firstboot.service pertisk-net.service; do
  src="$ROOT/iso/overlay/usr/lib/systemd/system/$unit"
  if [[ -f "$src" ]]; then
    install -m 644 "$src" "$MNT/usr/lib/systemd/system/$unit"
  fi
done

# Console: skip getty password on VGA/serial — drop into pertisk-console (root shell).
if [[ -f "$ROOT/iso/overlay/usr/sbin/pertisk-console" ]]; then
  install -m 755 "$ROOT/iso/overlay/usr/sbin/pertisk-console" "$MNT/usr/sbin/pertisk-console"
fi
if [[ -f "$ROOT/iso/overlay/usr/sbin/pertisk-zsh-setup" ]]; then
  install -m 755 "$ROOT/iso/overlay/usr/sbin/pertisk-zsh-setup" "$MNT/usr/sbin/pertisk-zsh-setup"
fi
if [[ -f "$ROOT/iso/overlay/usr/sbin/pertisk-fix-hosts" ]]; then
  install -m 755 "$ROOT/iso/overlay/usr/sbin/pertisk-fix-hosts" "$MNT/usr/sbin/pertisk-fix-hosts"
fi
if [[ -f "$ROOT/iso/overlay/usr/sbin/pertisk-apt-bootstrap" ]]; then
  install -m 755 "$ROOT/iso/overlay/usr/sbin/pertisk-apt-bootstrap" "$MNT/usr/sbin/pertisk-apt-bootstrap"
fi
if [[ -f "$ROOT/iso/overlay/etc/hostname" ]]; then
  install -m 644 "$ROOT/iso/overlay/etc/hostname" "$MNT/etc/hostname"
fi
"$MNT/usr/sbin/pertisk-fix-hosts" "$MNT" 2>/dev/null \
  || /bin/bash "$ROOT/iso/overlay/usr/sbin/pertisk-fix-hosts" "$MNT" \
  || echo "deploy-appliance: hosts fix skipped" >&2
mkdir -p "$MNT/etc/skel" "$MNT/root"
for dots in .zshrc .p10k.zsh; do
  if [[ -f "$ROOT/iso/overlay/etc/skel/$dots" ]]; then
    install -m 644 "$ROOT/iso/overlay/etc/skel/$dots" "$MNT/etc/skel/$dots"
    install -m 644 "$ROOT/iso/overlay/etc/skel/$dots" "$MNT/root/$dots"
  fi
done
for unit in getty@tty1.service.d serial-getty@ttyS0.service.d serial-getty@ttyS2.service.d serial-getty@ttyAMA0.service.d; do
  src="$ROOT/iso/overlay/etc/systemd/system/$unit/autologin.conf"
  if [[ -f "$src" ]]; then
    mkdir -p "$MNT/etc/systemd/system/$unit"
    install -m 644 "$src" "$MNT/etc/systemd/system/$unit/autologin.conf"
  fi
done

# journald: do not forward logs to the HDMI/serial console (pertisk-tui).
mkdir -p "$MNT/etc/systemd/journald.conf.d"
if [[ -f "$ROOT/iso/overlay/etc/systemd/journald.conf.d/pertisk-no-console.conf" ]]; then
  install -m 644 "$ROOT/iso/overlay/etc/systemd/journald.conf.d/pertisk-no-console.conf" \
    "$MNT/etc/systemd/journald.conf.d/pertisk-no-console.conf"
fi

# Host networking helpers (stable DHCP client id; no DHCP on bridge slaves).
for helper in pertisk-net pertisk-host-bridge; do
  if [[ -f "$ROOT/iso/overlay/usr/sbin/$helper" ]]; then
    install -m 755 "$ROOT/iso/overlay/usr/sbin/$helper" "$MNT/usr/sbin/$helper"
  fi
done
mkdir -p "$MNT/etc/systemd/network"
if [[ -f "$ROOT/iso/overlay/etc/systemd/network/20-wired-dhcp.network" ]]; then
  # Keep disabled copy if bridge already owns DHCP.
  if [[ -f "$MNT/etc/systemd/network/20-wired-dhcp.network.disabled" ]]; then
    install -m 644 "$ROOT/iso/overlay/etc/systemd/network/20-wired-dhcp.network" \
      "$MNT/etc/systemd/network/20-wired-dhcp.network.disabled"
  elif [[ -f "$MNT/etc/systemd/network/20-wired-dhcp.network" ]]; then
    install -m 644 "$ROOT/iso/overlay/etc/systemd/network/20-wired-dhcp.network" \
      "$MNT/etc/systemd/network/20-wired-dhcp.network"
  else
    install -m 644 "$ROOT/iso/overlay/etc/systemd/network/20-wired-dhcp.network" \
      "$MNT/etc/systemd/network/20-wired-dhcp.network"
  fi
fi
# Ensure existing br0 DHCP uses MAC client-id (same lease across reboot).
if [[ -f "$MNT/etc/systemd/network/15-br0.network" ]] \
  && ! grep -q 'ClientIdentifier=mac' "$MNT/etc/systemd/network/15-br0.network"; then
  cat >"$MNT/etc/systemd/network/15-br0.network" <<'EOF'
[Match]
Name=br0

[Network]
DHCP=yes
IPv6AcceptRA=yes

[DHCPv4]
ClientIdentifier=mac
UseDNS=yes
UseRoutes=yes
EOF
fi
if [[ -f "$MNT/etc/systemd/network/15-br0-bind.network" ]] \
  && ! grep -q 'DHCP=no' "$MNT/etc/systemd/network/15-br0-bind.network"; then
  # Preserve Match Name=… from the existing bind file.
  nic="$(awk -F= '/^Name=/{print $2; exit}' "$MNT/etc/systemd/network/15-br0-bind.network" || true)"
  if [[ -n "$nic" ]]; then
    cat >"$MNT/etc/systemd/network/15-br0-bind.network" <<EOF
[Match]
Name=${nic}

[Network]
Bridge=br0
DHCP=no
IPv6AcceptRA=no
EOF
  fi
fi

sync
if [[ ! -f "$MNT/etc/fstab" ]]; then
  root_uuid=""
  esp_uuid=""
  if command -v blkid >/dev/null; then
    root_uuid="$(blkid -s UUID -o value "${ZVOL}-part2" 2>/dev/null || true)"
    esp_uuid="$(blkid -s UUID -o value "${ZVOL}-part1" 2>/dev/null || true)"
  fi
  if [[ -z "$root_uuid" ]]; then
    root_uuid="$(tune2fs -l "${ZVOL}-part2" 2>/dev/null | awk '/Filesystem UUID/ {print $3}')"
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
UI:               https://<this-host>:7443/  user admin  password in /etc/pertisk/admin
EOF

# Operator SSH keys: clones + this appliance's own root login.
mkdir -p "$MNT/etc/pertisk/ssh" "$MNT/root/.ssh"
chmod 700 "$MNT/root/.ssh"
if [[ -f /root/.ssh/authorized_keys ]]; then
  grep -E '^(ssh-|ecdsa-|sk-ssh-|sk-ecdsa-)' /root/.ssh/authorized_keys \
    | tee "$MNT/etc/pertisk/ssh/authorized_keys" >"$MNT/root/.ssh/authorized_keys" || true
  chmod 600 "$MNT/etc/pertisk/ssh/authorized_keys" "$MNT/root/.ssh/authorized_keys"
  echo "installed operator SSH keys into /root/.ssh/authorized_keys"
fi

mkdir -p "$MNT/etc/apt/sources.list.d"
if [[ -f "$ROOT/iso/overlay/etc/apt/sources.list.d/debian.sources" ]]; then
  install -m 644 "$ROOT/iso/overlay/etc/apt/sources.list.d/debian.sources" \
    "$MNT/etc/apt/sources.list.d/debian.sources"
fi

# chroot: real DNS (guest resolv.conf is often 127.0.0.53), then apt, then zsh.
need_chroot=0
[[ -x "$MNT/usr/sbin/pertisk-apt-bootstrap" || -x "$MNT/usr/sbin/pertisk-zsh-setup" ]] && need_chroot=1
if [[ "$need_chroot" -eq 1 ]]; then
  for d in proc sys dev; do
    mkdir -p "$MNT/$d"
    mount --bind "/$d" "$MNT/$d" 2>/dev/null || true
  done
  printf 'nameserver 1.1.1.1\nnameserver 8.8.8.8\n' >"$MNT/etc/resolv.conf.pertisk-deploy"
  if [[ -e "$MNT/etc/resolv.conf" || -L "$MNT/etc/resolv.conf" ]]; then
    mount --bind "$MNT/etc/resolv.conf.pertisk-deploy" "$MNT/etc/resolv.conf" 2>/dev/null || true
  else
    cp "$MNT/etc/resolv.conf.pertisk-deploy" "$MNT/etc/resolv.conf"
  fi
  run_chroot() {
    local bin="$1"
    [[ -x "$MNT$bin" ]] || return 0
    if command -v timeout >/dev/null 2>&1; then
      timeout 300 chroot "$MNT" "$bin" || echo "deploy-appliance: $bin skipped" >&2
    else
      chroot "$MNT" "$bin" || echo "deploy-appliance: $bin skipped" >&2
    fi
  }
  run_chroot /usr/sbin/pertisk-apt-bootstrap
  run_chroot /usr/sbin/pertisk-zsh-setup
  umount "$MNT/etc/resolv.conf" 2>/dev/null || true
  umount "$MNT/dev" "$MNT/sys" "$MNT/proc" 2>/dev/null || true
  rm -f "$MNT/etc/resolv.conf.pertisk-deploy"
fi

umount "$MNT"
rmdir "$MNT" 2>/dev/null || true

echo "starting VM $VMID"
qm start "$VMID"

echo "deploy-appliance: done (VM $VMID)."
echo "  UI: https://<appliance-ip>:7443/  (admin / admin; self-signed cert)"
echo "  Reboots do not need redeploy unless you change pertisk source."
