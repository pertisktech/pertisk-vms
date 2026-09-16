# pertisk-node.ks — AlmaLinux 10 automated node install for Pertisk.
# Embedded into the installer ISO by scripts/build-alma-iso.sh (mkksiso).
# Default base is AlmaLinux boot.iso (network install). For offline DVD/minimal
# media, replace the url/repo lines with: cdrom
# WARNING: clearpart wipes disks Anaconda selects for the install.

# Unattended; any missing answer aborts (pairs with inst.cmdline on the ISO).
cmdline
lang en_US.UTF-8
keyboard us
timezone UTC --utc
rootpw --plaintext pertisk
# Device activated in initramfs via ip=dhcp; this persists config for the installed system.
network --bootproto=dhcp --device=link --activate --onboot=on
url --url="https://repo.almalinux.org/almalinux/10/BaseOS/x86_64/os/"
repo --name=AppStream --baseurl="https://repo.almalinux.org/almalinux/10/AppStream/x86_64/os/"
firewall --enabled --service=ssh
selinux --permissive
firstboot --disable
skipx
reboot

zerombr
clearpart --all --initlabel
autopart --type=plain --nohome
bootloader --location=mbr --append="console=tty0 console=ttyS0"

%packages
@^minimal-environment
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

echo "pertisk kickstart: locating RPM on install media"
RPM=""
for d in \
  /run/install/repo/pertisk \
  /mnt/install/pertisk \
  /run/install/sources/mount-000010-cdrom/pertisk \
  /tmp/pertisk-rpms
do
  if compgen -G "$d"/pertisk-vms-*.rpm >/dev/null 2>&1; then
    RPM="$(ls -1 "$d"/pertisk-vms-*.rpm | head -n1)"
    break
  fi
done

if [[ -z "$RPM" ]]; then
  for m in /run/media/*/* /media/*; do
    if compgen -G "$m"/pertisk/pertisk-vms-*.rpm >/dev/null 2>&1; then
      RPM="$(ls -1 "$m"/pertisk/pertisk-vms-*.rpm | head -n1)"
      break
    fi
  done
fi

[[ -n "$RPM" && -f "$RPM" ]] || {
  echo "pertisk kickstart: ERROR — pertisk-vms RPM not found on install media" >&2
  ls -laR /run/install 2>/dev/null || true
  exit 1
}

echo "pertisk kickstart: installing $RPM"
dnf install -y "$RPM"

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
  pertisk-firstboot.service pertisk-bootfix.service \
  pertisk-net.service pertiskd.service

echo "pertisk kickstart: done"
%end
