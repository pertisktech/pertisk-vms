# pertisk-node.ks - AlmaLinux 10 node install for Pertisk.
# Embedded by scripts/build-alma-iso.sh (mkksiso).
#
# Fully automated Anaconda except install disk when multiple HDD/NVMe are present.
# %pre lists non-USB disks; one disk → auto, several → console menu, then writes layout.
# After reboot, /usr/sbin/pertisk-setup asks for hostname, LAN, admin password.

cmdline
lang en_US.UTF-8
keyboard --vckeymap=us
timezone Etc/UTC --utc
# Temporary root; pertisk-setup sets the real admin UI password on first boot.
rootpw --plaintext pertisk
# Temporary DHCP for Anaconda package download; setup wizard replaces LAN config.
network --bootproto=dhcp --device=link --activate --onboot=on
url --url="https://repo.almalinux.org/almalinux/10/BaseOS/x86_64/os/"
repo --name=alma-appstream --baseurl="https://repo.almalinux.org/almalinux/10/AppStream/x86_64/os/"
# Local RPM repo populated in %pre from the boot USB (not BaseOS/AppStream).
repo --name=pertisk --baseurl=file:///run/pertisk-repo --cost=1
firewall --enabled --service=ssh --port=7443:tcp,7480:tcp
selinux --permissive
firstboot --disable
skipx
reboot
# kdump's "Crash recovery kernel arming..." often hangs first boot on mini PCs.
services --disabled=kdump

%addon com_redhat_kdump --disable
%end

# crashkernel=no: prevent Anaconda/grubby from reserving crash memory.
bootloader --location=mbr --append="console=tty0 console=ttyS0 crashkernel=no"

# Disk selection + GPT layout generated in %pre (avoids Anaconda Installation Summary).
%include /tmp/pertisk-disk.ks

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

DISK=""
DISK_BYTES=0
CANDIDATES=()
while read -r name size rem; do
  [[ -n "$name" ]] || continue
  # Keep only safe kernel disk names (sda, nvme0n1, …).
  [[ "$name" =~ ^[a-zA-Z0-9]+$ ]] || continue
  tran="$(lsblk -dn -o TRAN "/dev/${name}" 2>/dev/null | tr -d '\r' || true)"
  case "$tran" in
    usb|mmc) continue ;;
  esac
  [[ "${rem:-0}" == "1" ]] && continue
  if lsblk -dn -o TYPE "/dev/${name}" 2>/dev/null | tr -d '\r' | grep -qx rom; then
    continue
  fi
  [[ "$size" =~ ^[0-9]+$ ]] || continue
  human="$(lsblk -dn -o SIZE "/dev/${name}" 2>/dev/null | tr -d '\r[:space:]' || echo '?')"
  model="$(lsblk -dn -o MODEL "/dev/${name}" 2>/dev/null | tr -d '\r' | tr -s '[:space:]' ' ' | sed 's/^ //;s/ $//' || true)"
  model="$(printf '%s' "$model" | tr -cd 'A-Za-z0-9 ._/+-')"
  [[ -n "$model" ]] || model=disk
  CANDIDATES+=("${name}|${size}|${human}|${model}")
done < <(lsblk -dn -b -o NAME,SIZE,RM,TYPE | awk '$4=="disk"{print $1,$2,$3}')

if [[ "${#CANDIDATES[@]}" -eq 0 ]]; then
  echo "pertisk kickstart: ERROR - no suitable HDD/NVMe found" >&2
  lsblk -o NAME,SIZE,TYPE,TRAN,RM,MODEL >&2 || true
  exit 1
fi

if [[ "${#CANDIDATES[@]}" -eq 1 ]]; then
  DISK="${CANDIDATES[0]%%|*}"
  rest="${CANDIDATES[0]#*|}"
  DISK_BYTES="${rest%%|*}"
  echo "pertisk kickstart: single disk - installing to /dev/${DISK}"
else
  # Multiple disks: plain ASCII menu on the console (no Anaconda spoke in cmdline).
  # Avoid permanent exec redirects (breaks Anaconda logging / layout).
  largest_i=1
  largest_b=0
  i=1
  for row in "${CANDIDATES[@]}"; do
    rest="${row#*|}"
    bytes="${rest%%|*}"
    if [[ "$bytes" -gt "$largest_b" ]]; then
      largest_b=$bytes
      largest_i=$i
    fi
    i=$((i + 1))
  done

  menu_file=/tmp/pertisk-disk-menu.txt
  {
    echo
    echo "========================================"
    echo "  Pertisk - select install disk"
    echo "  WARNING: chosen disk will be WIPED"
    echo "========================================"
    i=1
    for row in "${CANDIDATES[@]}"; do
      name="${row%%|*}"
      rest="${row#*|}"
      rest2="${rest#*|}"
      human="${rest2%%|*}"
      model="${rest2#*|}"
      # One simple line per disk — no column padding (breaks on serial/VGA).
      echo "  ${i}. /dev/${name}  ${human}  ${model}"
      i=$((i + 1))
    done
    echo
    echo "Enter number (default ${largest_i})"
  } >"$menu_file"

  # Show on VGA and serial if present.
  for cons in /dev/console /dev/tty0 /dev/ttyS0; do
    [[ -c "$cons" ]] || continue
    cat "$menu_file" >"$cons" 2>/dev/null || true
  done
  # Also keep a copy in the %pre log.
  cat "$menu_file"

  choice=""
  # Prefer reading from console; fall back to default if non-interactive.
  if [[ -c /dev/console ]]; then
    # stty may fail on some consoles; ignore.
    stty sane < /dev/console 2>/dev/null || true
    printf 'Disk number [%s]: ' "$largest_i" >/dev/console 2>/dev/null || true
    # shellcheck disable=SC2162
    read -r -t 300 choice < /dev/console || choice=""
  fi
  choice="${choice:-$largest_i}"
  # Strip CR and spaces from serial input.
  choice="$(printf '%s' "$choice" | tr -d '\r[:space:]')"

  if ! [[ "$choice" =~ ^[0-9]+$ ]] || [[ "$choice" -lt 1 || "$choice" -gt "${#CANDIDATES[@]}" ]]; then
    echo "pertisk kickstart: invalid disk choice '${choice}', using ${largest_i}" >&2
    choice=$largest_i
  fi
  idx=$((choice - 1))
  row="${CANDIDATES[$idx]}"
  DISK="${row%%|*}"
  rest="${row#*|}"
  DISK_BYTES="${rest%%|*}"
  echo "pertisk kickstart: installing to /dev/${DISK} (${DISK_BYTES} bytes)"
  for cons in /dev/console /dev/tty0 /dev/ttyS0; do
    [[ -c "$cons" ]] || continue
    echo "Selected /dev/${DISK}" >"$cons" 2>/dev/null || true
  done
fi

# Anaconda ignoredisk wants the kernel name without /dev/.
cat >/tmp/pertisk-disk.ks <<EOF
# Auto-generated by pertisk %pre — wipe and install on /dev/${DISK} only.
ignoredisk --only-use=${DISK}
zerombr
clearpart --all --initlabel --disklabel=gpt --drives=${DISK}
part biosboot --fstype=biosboot --size=1
part /boot/efi --fstype=efi --size=512
part /boot --fstype=xfs --size=1024
part swap --fstype=swap --size=2048
part / --fstype=xfs --size=1 --grow
EOF
cat /tmp/pertisk-disk.ks
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
# Console wizard (pertisk-setup) must run before pertiskd / web UI.
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

# Never arm a crash kernel on first boot (hangs many mini-PC / appliance boards).
systemctl disable kdump.service 2>/dev/null || true
systemctl mask kdump.service 2>/dev/null || true
if command -v grubby >/dev/null 2>&1; then
  grubby --update-kernel=ALL --remove-args="crashkernel=auto" --args="crashkernel=no" 2>/dev/null || true
fi
# Strip crashkernel from BLS/grub snippets if present.
for f in /etc/default/grub /etc/kernel/cmdline /boot/loader/entries/*.conf; do
  [[ -e "$f" ]] || continue
  if grep -q 'crashkernel=' "$f" 2>/dev/null; then
    sed -i -E 's/crashkernel=[^ ]+/crashkernel=no/g' "$f" 2>/dev/null || true
  fi
done

systemctl enable NetworkManager.service sshd.service firewalld.service \
  pertisk-firstboot.service pertisk-bootcfg.service \
  pertisk-net.service pertisk-setup.service pertiskd.service || true

# Make getty always land in the wizard when needed (more reliable than a oneshot TTY fight).
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
