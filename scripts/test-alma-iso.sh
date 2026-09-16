#!/usr/bin/env bash
# Smoke-test helper: document QEMU boot of the AlmaLinux Kickstart ISO.
# Full Anaconda install needs a writable second disk and a long timeout.
#
# Example (on a Linux build host with edk2-ovmf):
#
#   make release-alma-iso VERSION=0.1.0
#   qemu-img create -f qcow2 /tmp/pertisk-disk.qcow2 40G
#   qemu-system-x86_64 -enable-kvm -m 4096 -smp 4 \
#     -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/ovmf/OVMF_CODE.fd \
#     -drive if=pflash,format=raw,file=/tmp/OVMF_VARS.fd \
#     -cdrom release/pertisk-node-0.1.0-x86_64.iso \
#     -drive file=/tmp/pertisk-disk.qcow2,if=virtio,format=qcow2 \
#     -boot d -serial stdio -display none
#
# After install + reboot, remove -cdrom / -boot d and confirm:
#   systemctl is-active pertiskd
#   curl -k https://<guest-ip>:7443/v1/health
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ISO="${1:-}"

if [[ -z "$ISO" ]]; then
  ISO="$(ls -1t "$ROOT"/release/pertisk-node-*-x86_64.iso 2>/dev/null | head -n1 || true)"
fi

cat <<EOF
pertisk AlmaLinux ISO smoke test
================================
ISO: ${ISO:-"(build with: make release-alma-iso VERSION=0.1.0)"}

1) Create a blank target disk:
     qemu-img create -f qcow2 /tmp/pertisk-disk.qcow2 40G

2) Copy OVMF vars (once):
     cp /usr/share/edk2/ovmf/OVMF_VARS.fd /tmp/OVMF_VARS.fd
     # Debian/Ubuntu path may be: /usr/share/OVMF/OVMF_VARS_4M.fd

3) Boot the installer (text mode, serial console):
     qemu-system-x86_64 -enable-kvm -m 4096 -smp 4 \\
       -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/ovmf/OVMF_CODE.fd \\
       -drive if=pflash,format=raw,file=/tmp/OVMF_VARS.fd \\
       -cdrom ${ISO:-release/pertisk-node-VERSION-x86_64.iso} \\
       -drive file=/tmp/pertisk-disk.qcow2,if=virtio,format=qcow2 \\
       -boot d -serial stdio -display none

4) After Anaconda finishes and reboots, start from disk only and verify pertiskd.

Hardware: UEFI PC, Secure Boot OFF, boot the ISO from USB.
EOF
