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
bash -n "$OVERLAY/usr/sbin/pertisk-host-bridge"
bash -n "$OVERLAY/usr/sbin/pertisk-bootfix"
bash -n "$OVERLAY/usr/sbin/pertisk-esp-boot"
bash -n "$OVERLAY/usr/sbin/pertisk-net"
bash -n "$OVERLAY/usr/sbin/pertisk-console"
bash -n "$OVERLAY/usr/sbin/pertisk-zsh-setup"
bash -n "$OVERLAY/usr/sbin/pertisk-fix-hosts"
bash -n "$OVERLAY/usr/sbin/pertisk-apt-bootstrap"
bash -n "$OVERLAY/usr/sbin/pertisk-fix-nvme-boot"
bash -n "$OVERLAY/usr/sbin/pertisk-uefi-register"
bash -n "$ROOT/scripts/build-iso.sh"
bash -n "$ROOT/scripts/build-sbc-image.sh"
bash -n "$ROOT/scripts/flash.sh"
bash -n "$ROOT/scripts/install-node.sh"
bash -n "$ROOT/scripts/test-qemu.sh"
bash -n "$ROOT/scripts/lib.sh"
echo "ok  bash -n overlay + scripts"

[[ -f "$ROOT/Makefile" ]] || { echo "FAIL Makefile"; fail=1; }
grep -q '^release-amd release-amd64:' "$ROOT/Makefile" || { echo "FAIL Makefile release-amd"; fail=1; }
grep -q '^release-arm release-arm64:' "$ROOT/Makefile" || { echo "FAIL Makefile release-arm"; fail=1; }
grep -q '^release-sbc:' "$ROOT/Makefile" || { echo "FAIL Makefile release-sbc"; fail=1; }
echo "ok  Makefile release-amd / release-arm / release-sbc"

[[ -f "$ROOT/iso/sbc/orangepi5plus.env" ]] || { echo "FAIL sbc orangepi5plus"; fail=1; }
[[ -f "$ROOT/iso/sbc/orangepi5max.env" ]] || { echo "FAIL sbc orangepi5max"; fail=1; }
[[ -f "$ROOT/iso/sbc/rpi5.env" ]] || { echo "FAIL sbc rpi5"; fail=1; }
grep -q 'FAMILY=rockchip' "$ROOT/iso/sbc/orangepi5plus.env" || { echo "FAIL plus family"; fail=1; }
echo "ok  sbc board recipes"

out="$("$OVERLAY/usr/sbin/pertisk-install" --help)"
echo "$out" | grep -q -- '--disk' || { echo "FAIL install --help"; fail=1; }
echo "$out" | grep -q -- '--auto' || { echo "FAIL install --help auto"; fail=1; }
grep -q 'pertisk-bootfix.service' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL bootfix preset"; fail=1; }
grep -q 'install_rockchip' "$OVERLAY/usr/sbin/pertisk-install" || { echo "FAIL install rockchip"; fail=1; }
grep -q 'install_rpi' "$OVERLAY/usr/sbin/pertisk-install" || { echo "FAIL install rpi"; fail=1; }
grep -q 'pertisk-esp-boot' "$OVERLAY/usr/sbin/pertisk-install" || { echo "FAIL install esp-boot"; fail=1; }
grep -q '/boot/efi/vmlinuz\|/efi/vmlinuz\|/usr/lib/modules' "$OVERLAY/usr/sbin/pertisk-esp-boot" \
  || { echo "FAIL esp-boot finds kernel on ESP or modules"; fail=1; }
grep -q 'copy_kernel_to_target_boot\|/efi/' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must copy kernel from live /efi ESP"; fail=1; }
grep -q '/boot/vmlinuz' "$ROOT/iso/mkosi.finalize.chroot" \
  || { echo "FAIL finalize must stage kernel on ext4 /boot"; fail=1; }

grep -q 'root=UUID\|root=LABEL' "$OVERLAY/usr/sbin/pertisk-esp-boot" \
  || { echo "FAIL esp-boot must set root=UUID/LABEL"; fail=1; }
grep -q 'rootdelay=15' "$OVERLAY/usr/sbin/pertisk-esp-boot" \
  || { echo "FAIL esp-boot needs rootdelay for slow NVMe"; fail=1; }
grep -q '^nvme$' "$OVERLAY/etc/initramfs-tools/modules" \
  || { echo "FAIL initramfs must force-load nvme"; fail=1; }
grep -q 'MODULES=most' "$OVERLAY/etc/initramfs-tools/conf.d/pertisk-storage.conf" \
  || { echo "FAIL initramfs MODULES=most for storage"; fail=1; }
grep -q 'update-initramfs' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must rebuild initramfs before NVMe boot"; fail=1; }
grep -q 'UNPLUG' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must tell user to unplug USB"; fail=1; }
grep -q 'grub-mkimage' "$OVERLAY/usr/sbin/pertisk-esp-boot" || { echo "FAIL embedded GRUB efi"; fail=1; }
grep -q 'is_usb_or_removable' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install skips removable disks"; fail=1; }
if grep -q 'pertisk-kvm-check' "$OVERLAY/usr/sbin/pertisk-install"; then
  echo "FAIL pertisk-install must not require KVM"
  fail=1
fi
grep -q 'is_installer_media' "$OVERLAY/usr/sbin/pertisk-firstboot" \
  || { echo "FAIL firstboot installer-media detect"; fail=1; }
grep -q '^PERTISK_AUTO_INSTALL=0$' "$OVERLAY/etc/pertisk/install" \
  || { echo "FAIL default install is interactive (not silent firstboot wipe)"; fail=1; }
grep -q 'pertisk-install --auto' "$OVERLAY/usr/sbin/pertisk-console" \
  || { echo "FAIL console must document pertisk-install --auto"; fail=1; }
grep -q 'pertisk-fix-nvme-boot' "$OVERLAY/usr/sbin/pertisk-console" \
  || { echo "FAIL console must document pertisk-fix-nvme-boot"; fail=1; }
if grep -q 'offer_nvme_install' "$OVERLAY/usr/sbin/pertisk-console"; then
  echo "FAIL console must not auto-prompt NVMe install"
  fail=1
fi
grep -q 'Windows Boot Manager' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must register AMI Windows Boot Manager path"; fail=1; }
grep -q 'ESP incomplete' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must verify ESP before success"; fail=1; }
grep -q 'chmod 644' "$OVERLAY/usr/sbin/pertisk-host-bridge" \
  || { echo "FAIL host-bridge networkd files must be world-readable"; fail=1; }
if grep -q 'network-online.target' "$OVERLAY/usr/lib/systemd/system/pertisk-firstboot.service"; then
  echo "FAIL firstboot must not wait for DHCP"
  fail=1
fi
grep -q 'firmware-realtek' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL firmware-realtek (G11 2.5G NIC)"; fail=1; }
grep -q 'non-free-firmware' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL debian non-free-firmware repo"; fail=1; }
grep -q '^[[:space:]]*apt$' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL mkosi apt package (Updates tab)"; fail=1; }
grep -q '^[[:space:]]*zsh$' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL mkosi zsh package"; fail=1; }
[[ -x "$OVERLAY/usr/sbin/pertisk-zsh-setup" ]] \
  || { echo "FAIL pertisk-zsh-setup executable"; fail=1; }
[[ -f "$OVERLAY/etc/skel/.zshrc" && -f "$OVERLAY/etc/skel/.p10k.zsh" ]] \
  || { echo "FAIL skel zsh/p10k"; fail=1; }
grep -q 'powerlevel10k/powerlevel10k' "$OVERLAY/etc/skel/.zshrc" \
  || { echo "FAIL zsh Powerlevel10k theme"; fail=1; }
grep -q 'exec /bin/zsh -l' "$OVERLAY/usr/sbin/pertisk-console" \
  || { echo "FAIL console must exec zsh"; fail=1; }
grep -q 'pertisk-zsh-setup' "$ROOT/iso/mkosi.finalize.chroot" \
  || { echo "FAIL mkosi finalize must run zsh setup"; fail=1; }
[[ -f "$OVERLAY/etc/hosts" ]] && grep -q '127.0.1.1' "$OVERLAY/etc/hosts" \
  || { echo "FAIL overlay /etc/hosts must map pertisk"; fail=1; }
grep -q 'pertisk-fix-hosts' "$OVERLAY/usr/sbin/pertisk-firstboot" \
  || { echo "FAIL firstboot must fix /etc/hosts"; fail=1; }
grep -q -- '--any' "$OVERLAY/etc/systemd/system/systemd-networkd-wait-online.service.d/any.conf" \
  || { echo "FAIL wait-online --any (dual NIC)"; fail=1; }
grep -q 'wipe_target_disk' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must wipe leftover NVMe"; fail=1; }
grep -q 'register_uefi_boot' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install registers NVMe in EFI NVRAM"; fail=1; }
grep -q 'pertisk-uefi-register' "$OVERLAY/usr/sbin/pertisk-install" \
  || { echo "FAIL install must call pertisk-uefi-register"; fail=1; }
grep -q 'bootmgfw.efi' "$OVERLAY/usr/sbin/pertisk-esp-boot" \
  || { echo "FAIL Microsoft boot path for AMI BIOS drop"; fail=1; }
grep -q 'Windows Boot Manager' "$OVERLAY/usr/sbin/pertisk-uefi-register" \
  || { echo "FAIL NVRAM Windows Boot Manager for AMI"; fail=1; }
# BootOrder must be NVMe-only (do not append old USB/stale order).
if grep -E 'efibootmgr -o.*"\$\{?order\},\$\{?rest\}|"\$order,\$rest' \
  "$OVERLAY/usr/sbin/pertisk-uefi-register" "$OVERLAY/usr/sbin/pertisk-install"; then
  echo "FAIL BootOrder must not append old entries (USB stays first → BIOS)"
  fail=1
fi
grep -q 'efibootmgr -o "\$order"' "$OVERLAY/usr/sbin/pertisk-uefi-register" \
  || { echo "FAIL uefi-register must set BootOrder to NVMe entries only"; fail=1; }
grep -q 'pertisk-uefi-register' "$OVERLAY/usr/sbin/pertisk-fix-nvme-boot" \
  || { echo "FAIL nvme boot repair must call uefi-register"; fail=1; }
grep -q 'EFI/Microsoft/Boot' "$OVERLAY/usr/sbin/pertisk-fix-nvme-boot" \
  || { echo "FAIL nvme boot repair script"; fail=1; }
grep -q 'enable pertisk-net.service' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL pertisk-net preset"; fail=1; }
grep -q '^PermitRootLogin yes$' "$OVERLAY/etc/ssh/sshd_config.d/pertisk.conf" \
  || { echo "FAIL ssh root password login"; fail=1; }
grep -q 'ensure_ssh_login' "$OVERLAY/usr/sbin/pertisk-firstboot" \
  || { echo "FAIL firstboot sets ssh passwords"; fail=1; }
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
[[ -f "$ROOT/iso/mkosi.conf.d/10-amd64.conf" ]] || { echo "FAIL mkosi amd64 conf"; fail=1; }
[[ -f "$ROOT/iso/mkosi.conf.d/10-arm64.conf" ]] || { echo "FAIL mkosi arm64 conf"; fail=1; }
grep -q '^Format=disk' "$ROOT/iso/mkosi.conf" || { echo "FAIL Format=disk"; fail=1; }
grep -q '^Bootloader=none' "$ROOT/iso/mkosi.conf" || { echo "FAIL Bootloader=none (FAT ESP chain)"; fail=1; }
grep -q '^UnifiedKernelImages=no' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL UnifiedKernelImages=no (EFI stub hang on mini PCs)"; fail=1; }
grep -q '^KernelCommandLine=.*console=tty0' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL HDMI kernel console"; fail=1; }
grep -q '^KernelCommandLine=.*console=ttyS0' "$ROOT/iso/mkosi.conf" \
  || { echo "FAIL serial kernel console"; fail=1; }
grep -q '^ExecStart=-/usr/sbin/pertisk-console$' \
  "$OVERLAY/etc/systemd/system/getty@tty1.service.d/autologin.conf" \
  || { echo "FAIL tty1 root shell"; fail=1; }
grep -q '^Restart=no$' \
  "$OVERLAY/etc/systemd/system/getty@tty1.service.d/autologin.conf" \
  || { echo "FAIL tty1 no restart loop"; fail=1; }
grep -q 'nomodeset' "$ROOT/iso/mkosi.conf.d/10-amd64.conf" \
  || { echo "FAIL amd64 nomodeset"; fail=1; }
grep -q '^SizeMinBytes=6G$' "$ROOT/iso/mkosi.repart/10-root.conf" \
  || { echo "FAIL 6GiB live USB root (must fit 8GB sticks)"; fail=1; }
grep -q '^ExecStart=-/usr/sbin/pertisk-console$' \
  "$OVERLAY/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf" \
  || { echo "FAIL serial root shell"; fail=1; }
grep -q '^ExecStart=-/usr/sbin/pertisk-console$' \
  "$OVERLAY/etc/systemd/system/serial-getty@ttyAMA0.service.d/autologin.conf" \
  || { echo "FAIL arm serial root shell"; fail=1; }
grep -q '^ExecStart=-/usr/sbin/pertisk-console$' \
  "$OVERLAY/etc/systemd/system/serial-getty@ttyS2.service.d/autologin.conf" \
  || { echo "FAIL rk3588 serial root shell"; fail=1; }
grep -q '^ConditionFirstBoot=no$' \
  "$OVERLAY/etc/systemd/system/systemd-firstboot.service.d/disable-interactive.conf" \
  || { echo "FAIL interactive firstboot is disabled"; fail=1; }
grep -q '^DHCP=yes' "$OVERLAY/etc/systemd/network/20-wired-dhcp.network" \
  || { echo "FAIL wired DHCP configuration"; fail=1; }
grep -q '^RequiredForOnline=no$' "$OVERLAY/etc/systemd/network/20-wired-dhcp.network" \
  || { echo "FAIL unplugged NIC must not block boot"; fail=1; }
grep -q '^enable systemd-networkd.service$' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL systemd-networkd preset"; fail=1; }
if grep -R -q --include='*.conf' '^ *grub-pc$' "$ROOT/iso"; then
  echo "FAIL grub-pc conflicts with grub-efi"
  fail=1
fi
grep -q '^CopyFiles=/efi:/' "$ROOT/iso/mkosi.repart/00-esp.conf" \
  || { echo "FAIL ESP copies /efi only"; fail=1; }
grep -q '^Label=pertisk-root$' "$ROOT/iso/mkosi.repart/10-root.conf" \
  || { echo "FAIL root label pertisk-root"; fail=1; }
grep -q 'grub-efi-amd64-bin' "$ROOT/iso/mkosi.conf.d/10-amd64.conf" \
  || { echo "FAIL grub-efi-amd64-bin (monolithic GRUB)"; fail=1; }
grep -q 'grub-efi-arm64' "$ROOT/iso/mkosi.conf.d/10-arm64.conf" \
  || { echo "FAIL grub-efi-arm64"; fail=1; }
grep -q 'linux-image-amd64' "$ROOT/iso/mkosi.conf.d/10-amd64.conf" \
  || { echo "FAIL linux-image-amd64"; fail=1; }
grep -q 'linux-image-arm64' "$ROOT/iso/mkosi.conf.d/10-arm64.conf" \
  || { echo "FAIL linux-image-arm64"; fail=1; }
grep -q 'pertiskd.service' "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk.preset" \
  || { echo "FAIL preset"; fail=1; }
[[ -f "$OVERLAY/etc/apt/apt.conf.d/90pertisk" ]] || { echo "FAIL apt unattended options"; fail=1; }
grep -q '/v1/updates' "$ROOT/crates/pertisk-api/src/lib.rs" \
  || { echo "FAIL OpenAPI updates path"; fail=1; }
grep -q '/v1/node/shell/ws' "$ROOT/crates/pertisk-api/src/lib.rs" \
  || { echo "FAIL OpenAPI node shell path"; fail=1; }
echo "ok  mkosi disk image + systemd preset"

wf="$ROOT/.github/workflows/release.yml"
grep -q 'rm -rf dist' "$wf" || { echo "FAIL release.yml must wipe dist/"; fail=1; }
grep -q 'rm -rf packages' "$wf" || { echo "FAIL release.yml must wipe packages/"; fail=1; }
grep -Fq 'find dist -type f ! -name "*${VERSION}*"' "$wf" \
  || { echo "FAIL release.yml must reject leftover dist files"; fail=1; }
echo "ok  release.yml does not mix versioned assets"

exit "$fail"
