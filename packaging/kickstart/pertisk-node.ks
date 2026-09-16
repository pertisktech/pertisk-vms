# pertisk-node.ks - AlmaLinux 10 node install for Pertisk.
# Embedded by scripts/build-alma-iso.sh (mkksiso).
#
# Default: Anaconda GRAPHICAL Installation Destination (pick HDD/NVMe on HDMI).
# AlmaLinux 10 has no Fedora Anaconda Web UI.
# Serial/headless: boot with kernel arg pertisk.autodisk → wipe largest non-USB disk.
# After reboot: /usr/sbin/pertisk-setup (hostname, LAN, admin password).

graphical
lang en_US.UTF-8
keyboard --vckeymap=us
timezone Etc/UTC --utc
rootpw --plaintext pertisk
network --bootproto=dhcp --device=link --activate --onboot=on
url --url="https://repo.almalinux.org/almalinux/10/BaseOS/x86_64/os/"
repo --name=alma-appstream --baseurl="https://repo.almalinux.org/almalinux/10/AppStream/x86_64/os/"
repo --name=pertisk --baseurl=file:///run/pertisk-repo --cost=1
firewall --enabled --service=ssh --port=7443:tcp,7480:tcp
selinux --permissive
firstboot --disable
reboot
services --disabled=kdump

%addon com_redhat_kdump --disable
%end

bootloader --location=mbr --append="console=tty0 crashkernel=no"

# Optional auto disk layout (only when pertisk.autodisk on kernel cmdline).
%include /tmp/pertisk-disk.ks

%pre --erroronfail --log=/tmp/pertisk-kickstart-pre.log
#!/bin/bash
set -euo pipefail

# Always create include file (empty OK) so %include never fails.
: >/tmp/pertisk-disk.ks

mkdir -p /run/pertisk-repo
RPM="$(find /run/install /run/media /media -name 'pertisk-vms-*.rpm' 2>/dev/null | head -n1 || true)"
if [[ -z "${RPM}" ]]; then
  echo "pertisk kickstart: ERROR - pertisk-vms RPM not on boot media" >&2
  find /run/install /run/media /media -maxdepth 5 -type d 2>/dev/null || true
  exit 1
fi
echo "pertisk kickstart: found ${RPM}"
SRC="$(dirname "${RPM}")"
cp -a "${SRC}/." /run/pertisk-repo/
ls -la /run/pertisk-repo/

# One line on HDMI only — never a multi-line disk menu (that corrupts the display).
if [[ -c /dev/tty0 ]]; then
  printf '\n*** PERTISK installer 0.1.20 - use the HDMI monitor (Anaconda GUI) ***\n' >/dev/tty0 || true
fi

if ! grep -qw pertisk.autodisk /proc/cmdline 2>/dev/null; then
  echo "pertisk kickstart: GUI disk select (no pertisk.autodisk)"
  exit 0
fi

# --- Serial / unattended: largest non-USB disk ---
DISK=""
DISK_BYTES=0
while read -r name size rem; do
  [[ -n "$name" ]] || continue
  [[ "$name" =~ ^[a-zA-Z0-9]+$ ]] || continue
  tran="$(lsblk -dn -o TRAN "/dev/${name}" 2>/dev/null | tr -d '\r' || true)"
  case "$tran" in usb|mmc) continue ;; esac
  [[ "${rem:-0}" == "1" ]] && continue
  if lsblk -dn -o TYPE "/dev/${name}" 2>/dev/null | tr -d '\r' | grep -qx rom; then
    continue
  fi
  [[ "$size" =~ ^[0-9]+$ ]] || continue
  if [[ "$size" -gt "$DISK_BYTES" ]]; then
    DISK_BYTES=$size
    DISK=$name
  fi
done < <(lsblk -dn -b -o NAME,SIZE,RM,TYPE | awk '$4=="disk"{print $1,$2,$3}')

if [[ -z "${DISK}" ]]; then
  echo "pertisk kickstart: ERROR - no suitable HDD/NVMe for pertisk.autodisk" >&2
  lsblk -o NAME,SIZE,TYPE,TRAN,RM,MODEL >&2 || true
  exit 1
fi

echo "pertisk kickstart: pertisk.autodisk → /dev/${DISK}"
cat >/tmp/pertisk-disk.ks <<EOF
ignoredisk --only-use=${DISK}
zerombr
clearpart --all --initlabel --disklabel=gpt --drives=${DISK}
part biosboot --fstype=biosboot --size=1
part /boot/efi --fstype=efi --size=512
part /boot --fstype=xfs --size=1024
part swap --fstype=swap --size=2048
part / --fstype=xfs --size=1 --grow
EOF
%end

%packages
@^minimal-environment
pertisk-vms
qemu-img
qemu-kvm
iproute
iptables-nft
openssh-server
NetworkManager
NetworkManager-tui
firewalld
-kexec-tools
-kdump-utils
%end

%post --erroronfail --log=/root/pertisk-kickstart-post.log
set -euo pipefail

mkdir -p /etc/pertisk /var/lib/pertisk
printf 'almalinux\n' >/etc/pertisk/os-flavor
cat >/etc/pertisk/install <<'EOF'
# AlmaLinux Kickstart install: Anaconda already wrote the disk.
PERTISK_AUTO_INSTALL=0
EOF
touch /var/lib/pertisk/.installed-to-disk
printf '1\n' >/etc/pertisk/needs-setup
rm -f /var/lib/pertisk/.setup-done /var/lib/pertisk/.firstboot-done

mkdir -p /etc/NetworkManager/system-connections

if command -v firewall-offline-cmd >/dev/null 2>&1; then
  firewall-offline-cmd --add-service=ssh || true
  firewall-offline-cmd --add-port=7443/tcp || true
  firewall-offline-cmd --add-port=7480/tcp || true
elif command -v firewall-cmd >/dev/null 2>&1; then
  firewall-cmd --offline --add-service=ssh || true
  firewall-cmd --offline --add-port=7443/tcp || true
  firewall-cmd --offline --add-port=7480/tcp || true
fi

systemctl disable kdump.service 2>/dev/null || true
systemctl mask kdump.service 2>/dev/null || true
if command -v grubby >/dev/null 2>&1; then
  grubby --update-kernel=ALL --remove-args="crashkernel=auto" --args="crashkernel=no" 2>/dev/null || true
fi
for f in /etc/default/grub /etc/kernel/cmdline /boot/loader/entries/*.conf; do
  [[ -e "$f" ]] || continue
  if grep -q 'crashkernel=' "$f" 2>/dev/null; then
    sed -i -E 's/crashkernel=[^ ]+/crashkernel=no/g' "$f" 2>/dev/null || true
  fi
done

systemctl enable NetworkManager.service sshd.service firewalld.service \
  pertisk-firstboot.service pertisk-bootcfg.service \
  pertisk-net.service pertisk-setup.service pertiskd.service || true

mkdir -p /etc/systemd/system/getty@tty1.service.d \
  /etc/systemd/system/serial-getty@ttyS0.service.d
cat >/etc/systemd/system/getty@tty1.service.d/autologin.conf <<'EOF'
[Service]
ExecStart=
ExecStart=-/usr/sbin/pertisk-console
Restart=no
EOF
cat >/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf <<'EOF'
[Service]
ExecStart=
ExecStart=-/usr/sbin/pertisk-console
Restart=no
EOF

if [[ -x /usr/sbin/pertisk-fix-hosts ]]; then
  /usr/sbin/pertisk-fix-hosts || true
fi

echo "pertisk kickstart: done (pertisk-setup runs on first console boot)"
%end
