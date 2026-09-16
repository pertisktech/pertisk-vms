# pertisk-node.ks - AlmaLinux 10 automated node install for Pertisk.
# Embedded by scripts/build-alma-iso.sh (mkksiso).
# Default: network install from boot.iso. For offline DVD/minimal, use: cdrom
# WARNING: clearpart wipes disks Anaconda selects for the install.

text
lang en_US.UTF-8
keyboard --vckeymap=us
timezone Etc/UTC --utc
rootpw --plaintext pertisk
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

# Boot.iso + url= means /run/install/repo is the *network* mirror, not the USB.
# mkksiso --add files stay on the boot media — copy them before chroot.
%post --nochroot --erroronfail --log=/mnt/sysimage/root/pertisk-kickstart-nochroot.log
set -euo pipefail

echo "pertisk kickstart: searching boot media for RPM"
RPM=""
while IFS= read -r -d '' f; do
  RPM="$f"
  break
done < <(find /run/install /run/media /media /mnt -name 'pertisk-vms-*.rpm' 2>/dev/null -print0)

if [[ -z "$RPM" ]]; then
  echo "pertisk kickstart: ERROR - pertisk-vms RPM not on boot media" >&2
  find /run/install /run/media /media -maxdepth 5 -type d 2>/dev/null || true
  ls -la / 2>/dev/null || true
  exit 1
fi

echo "pertisk kickstart: found $RPM"
mkdir -p /mnt/sysimage/root/pertisk-rpms
cp -f "$RPM" /mnt/sysimage/root/pertisk-rpms/
%end

%post --erroronfail --log=/root/pertisk-kickstart-post.log
set -euo pipefail

RPM="$(ls -1 /root/pertisk-rpms/pertisk-vms-*.rpm | head -n1)"
[[ -f "$RPM" ]] || {
  echo "pertisk kickstart: ERROR - RPM missing under /root/pertisk-rpms" >&2
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
  pertisk-firstboot.service pertisk-bootcfg.service \
  pertisk-net.service pertiskd.service

echo "pertisk kickstart: done"
%end
