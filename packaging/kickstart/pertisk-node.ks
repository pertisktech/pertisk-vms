# pertisk-node.ks - AlmaLinux 10 automated node install for Pertisk.
# Embedded by scripts/build-alma-iso.sh (mkksiso).
# WARNING: clearpart wipes disks Anaconda selects for the install.

text
lang en_US.UTF-8
keyboard --vckeymap=us
timezone Etc/UTC --utc
rootpw --plaintext pertisk
network --bootproto=dhcp --device=link --activate --onboot=on
url --url="https://repo.almalinux.org/almalinux/10/BaseOS/x86_64/os/"
repo --name=alma-appstream --baseurl="https://repo.almalinux.org/almalinux/10/AppStream/x86_64/os/"
# Local RPM repo populated in %pre from the boot USB (not BaseOS/AppStream).
repo --name=pertisk --baseurl=file:///run/pertisk-repo
firewall --enabled --service=ssh
selinux --permissive
firstboot --disable
skipx
reboot

# UEFI: create ESP explicitly; --location=efi is invalid on RHEL/Alma 10 (use mbr).
zerombr
clearpart --all --initlabel
part /boot/efi --fstype=efi --size=512
part /boot --fstype=xfs --size=1024
part swap --fstype=swap --size=2048
part / --fstype=xfs --size=1 --grow
bootloader --location=mbr --append="console=tty0 console=ttyS0"

# Copy mkksiso --add payload off the boot media into a file:// repo Anaconda can use.
%pre --erroronfail --log=/tmp/pertisk-kickstart-pre.log
#!/bin/bash
set -euo pipefail
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
%end

%packages
@^minimal-environment
pertisk-vms
qemu-img
iproute
iptables
openssh-server
NetworkManager
NetworkManager-tui
-gnome*
-kde*
-firefox
-libreoffice*
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

mkdir -p /etc/NetworkManager/system-connections
if [[ ! -f /etc/NetworkManager/system-connections/pertisk-wired.nmconnection ]]; then
  cat >/etc/NetworkManager/system-connections/pertisk-wired.nmconnection <<'EOF'
[connection]
id=pertisk-wired
type=ethernet
autoconnect=true
autoconnect-priority=100

[ipv4]
method=auto

[ipv6]
method=auto
EOF
  chmod 600 /etc/NetworkManager/system-connections/pertisk-wired.nmconnection
fi

systemctl enable NetworkManager.service sshd.service \
  pertisk-firstboot.service pertisk-bootcfg.service \
  pertisk-net.service pertiskd.service || true

echo "pertisk kickstart: done"
%end
