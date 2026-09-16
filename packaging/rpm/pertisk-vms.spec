# AlmaLinux / RHEL node package for pertisk-vms.
# Built by scripts/build-rpm.sh (stages files into BUILDROOT).

Name:           pertisk-vms
Version:        %{?pertisk_version}%{!?pertisk_version:0.1.0}
Release:        1%{?dist}
Summary:        Pertisk virtualization control plane (node daemon + CLI)
License:        Proprietary
URL:            https://github.com/pertisktech/pertisk-vms
BuildArch:      x86_64
Requires:       qemu-img
Requires:       iproute
Requires:       iptables
Requires:       openssh-server
Requires:       NetworkManager
Requires:       /usr/bin/ssh-keygen

%description
Pertisk node appliance: pertiskd HTTP/TLS API, CLI, TUI, Cloud Hypervisor
binary, and systemd units for first boot and networking.

%install
# Populated by scripts/build-rpm.sh into the rpmbuild BUILDROOT.
install -d %{buildroot}
cp -a %{_sourcedir}/payload/. %{buildroot}/

%files
%defattr(-,root,root,-)
/usr/bin/pertiskd
/usr/bin/pertisk
/usr/bin/pertisk-tui
/usr/bin/cloud-hypervisor
/usr/lib/cloud-hypervisor/hypervisor-fw
/usr/sbin/pertisk-firstboot
/usr/sbin/pertisk-kvm-check
/usr/sbin/pertisk-host-bridge
/usr/sbin/pertisk-bootfix
/usr/sbin/pertisk-net
/usr/sbin/pertisk-console
/usr/sbin/pertisk-fix-hosts
/usr/sbin/pertisk-fix-dns
/usr/lib/systemd/system/pertiskd.service
/usr/lib/systemd/system/pertisk-firstboot.service
/usr/lib/systemd/system/pertisk-net.service
/usr/lib/systemd/system/pertisk-bootfix.service
/usr/lib/systemd/system-preset/50-pertisk.preset
%config(noreplace) /etc/pertisk/config.toml
%config(noreplace) /etc/pertisk/daemon.env
%config(noreplace) /etc/pertisk/install
%config(noreplace) /etc/pertisk/join
/etc/pertisk/os-flavor
/etc/pertisk/version
/etc/modules-load.d/pertisk-net.conf
/etc/ssh/sshd_config.d/pertisk.conf
/etc/NetworkManager/conf.d/99-pertisk.conf
/etc/systemd/system/getty@tty1.service.d/autologin.conf
/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf
/etc/systemd/journald.conf.d/pertisk-no-console.conf

%post
# Alma: sshd unit name; enable Pertisk stack.
if command -v systemctl >/dev/null 2>&1; then
  systemctl enable NetworkManager.service >/dev/null 2>&1 || true
  systemctl enable sshd.service >/dev/null 2>&1 || true
  systemctl enable pertisk-firstboot.service >/dev/null 2>&1 || true
  systemctl enable pertisk-bootfix.service >/dev/null 2>&1 || true
  systemctl enable pertisk-net.service >/dev/null 2>&1 || true
  systemctl enable pertiskd.service >/dev/null 2>&1 || true
fi
# Do not package /etc/{hosts,hostname,motd}: setup and systemd own those files.
# Writing them here avoids DNF "file conflicts" during Anaconda.
if [ ! -s /etc/hostname ]; then
  printf 'pertisk\n' >/etc/hostname 2>/dev/null || true
fi
if [ -x /usr/sbin/pertisk-fix-hosts ]; then
  /usr/sbin/pertisk-fix-hosts >/dev/null 2>&1 || true
fi
cat >/etc/motd 2>/dev/null <<'EOF' || true
pertisk-vm node (AlmaLinux)
Already installed to disk by Anaconda — no pertisk-install step.
UI: http://<ip>:7480/  admin (see /etc/pertisk/admin)
SSH: root / pertisk
EOF
# Mark disk already installed so firstboot never runs live→NVMe copy.
mkdir -p /var/lib/pertisk
touch /var/lib/pertisk/.installed-to-disk 2>/dev/null || true
exit 0

%preun
if [ "$1" -eq 0 ] && command -v systemctl >/dev/null 2>&1; then
  systemctl disable --now pertiskd.service >/dev/null 2>&1 || true
  systemctl disable pertisk-firstboot.service >/dev/null 2>&1 || true
  systemctl disable pertisk-net.service >/dev/null 2>&1 || true
  systemctl disable pertisk-bootfix.service >/dev/null 2>&1 || true
fi
exit 0

%changelog
* Wed Sep 16 2026 Pertisk <ops@pertisk.tech> - 0.1.0-1
- Initial AlmaLinux 10 Kickstart RPM packaging.
