#!/usr/bin/env bash
# Smoke-test phase 7 scripts (no KVM, no mkosi). Safe on macOS.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OVERLAY="$ROOT/iso/overlay"
fail=0

check() {
  local name="$1"
  shift
  if "$@"; then
    echo "ok  $name"
  else
    echo "FAIL $name"
    fail=1
  fi
}

bash -n "$OVERLAY/usr/sbin/pertisk-kvm-check"
bash -n "$OVERLAY/usr/sbin/pertisk-firstboot"
bash -n "$OVERLAY/usr/sbin/pertisk-install"
bash -n "$ROOT/scripts/build-iso.sh"
bash -n "$ROOT/scripts/flash.sh"
bash -n "$ROOT/scripts/install-node.sh"
bash -n "$ROOT/scripts/test-qemu.sh"
echo "ok  bash -n overlay + scripts"

out="$("$OVERLAY/usr/sbin/pertisk-install" --help)"
echo "$out" | grep -q -- '--disk' || { echo "FAIL install --help"; fail=1; }
echo "$out" | grep -q -- '--list' || { echo "FAIL install --help list"; fail=1; }
echo "ok  pertisk-install --help"

if [[ "$(uname -s)" != "Linux" ]]; then
  if "$OVERLAY/usr/sbin/pertisk-kvm-check" 2>/dev/null; then
    echo "FAIL kvm-check should fail without /dev/kvm"
    fail=1
  else
    echo "ok  pertisk-kvm-check fails on $(uname -s)"
  fi
fi

[[ -f "$ROOT/iso/mkosi.conf" ]] || { echo "FAIL mkosi.conf"; fail=1; }
grep -q '^Format=disk' "$ROOT/iso/mkosi.conf" || { echo "FAIL Format=disk"; fail=1; }
grep -q '^Bootloader=grub' "$ROOT/iso/mkosi.conf" || { echo "FAIL Bootloader=grub"; fail=1; }
grep -q '^KernelCommandLine=.*console=ttyS0' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL serial kernel console"; fail=1; }
grep -q '^SizeMinBytes=12G$' "$ROOT/iso/mkosi.repart/10-root.conf" \
  || { echo "FAIL 12GiB ISO storage root"; fail=1; }
grep -q '^ExecStart=-/bin/bash --login$' \
  "$OVERLAY/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf" \
  || { echo "FAIL serial root shell"; fail=1; }
grep -q '^ConditionFirstBoot=no$' \
  "$OVERLAY/etc/systemd/system/systemd-firstboot.service.d/disable-interactive.conf" \
  || { echo "FAIL interactive firstboot is disabled"; fail=1; }
grep -q '^DHCP=yes' "$OVERLAY/etc/systemd/network/20-wired-dhcp.network" \
  || { echo "FAIL wired DHCP configuration"; fail=1; }
grep -q '^enable systemd-networkd.service$' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL systemd-networkd preset"; fail=1; }
if grep -q '^ *grub-pc$' "$ROOT/iso/mkosi.conf"; then
  echo "FAIL grub-pc conflicts with grub-efi-amd64"
  fail=1
fi
grep -q 'pertiskd.service' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL preset"; fail=1; }
echo "ok  mkosi disk image + systemd preset"

exit "$fail"
