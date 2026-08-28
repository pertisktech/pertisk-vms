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
grep -q '^Format=iso' "$ROOT/iso/mkosi.conf" || { echo "FAIL Format=iso"; fail=1; }
grep -q 'pertiskd.service' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL preset"; fail=1; }
echo "ok  mkosi iso + systemd preset"

exit "$fail"
