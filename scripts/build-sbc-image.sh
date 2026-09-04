#!/usr/bin/env bash
# Bake pertisk into a board OS image (Armbian / Raspberry Pi OS / Orange Pi OS).
# Output: release/pertisk-node-<version>-<board>.img.xz
#
# Linux only. The generic mkosi arm64.raw is UEFI GRUB and will not boot RK3588
# or Raspberry Pi firmware — use this instead of flashing pertisk-node-*-arm64.raw.
#
# Usage:
#   ./scripts/build-sbc-image.sh orangepi5plus 0.1.2
#   ./scripts/build-sbc-image.sh --board rpi5 --base-image /path/to.img
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"
RELEASE_DIR="$ROOT/release"
CACHE="${PERTISK_HOME:-$HOME/.pertisk}/images"

BOARD=""
VERSION="${PERTISK_VERSION:-}"
BASE_IMAGE=""

die() { echo "build-sbc: $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage: ./scripts/build-sbc-image.sh [BOARD] [VERSION]

Boards: orangepi5plus  orangepi5max  rpi5

  ./scripts/build-sbc-image.sh orangepi5plus 0.1.2
  ./scripts/build-sbc-image.sh --board orangepi5max --base-image ./orangepi.img

Flash: xzcat release/pertisk-node-VERSION-BOARD.img.xz | sudo dd of=/dev/sdX bs=4M status=progress conv=fsync
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --board) BOARD="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --base-image) BASE_IMAGE="$2"; shift 2 ;;
    orangepi5plus|orangepi5max|rpi5)
      BOARD="$1"
      shift
      ;;
    *)
      if [[ -z "$VERSION" && "$1" != -* ]]; then
        VERSION="$1"
        shift
      else
        die "unknown argument: $1"
      fi
      ;;
  esac
done

cargo_version() {
  awk '/^\[workspace.package\]/{p=1} p && /^version =/{gsub(/"/,"",$3); print $3; exit}' "$ROOT/Cargo.toml"
}

git_version() {
  git -C "$ROOT" describe --tags --always --dirty 2>/dev/null | sed 's/^v//' || true
}

[[ "$(uname -s)" == "Linux" ]] || die "build on Linux (this host is $(uname -s))"
[[ "$(id -u)" -eq 0 ]] || die "run as root (loop mounts + chroot)"
[[ -n "$BOARD" ]] || die "pass a board (see --help)"
ENVF="$ROOT/iso/sbc/${BOARD}.env"
[[ -f "$ENVF" ]] || die "missing $ENVF"
# shellcheck disable=SC1090
source "$ENVF"
VERSION="${VERSION:-$(git_version)}"
VERSION="${VERSION:-$(cargo_version)}"
[[ -n "$VERSION" ]] || die "set VERSION"
VERSION="${VERSION#v}"

command -v curl >/dev/null || die "curl not in PATH"
command -v cargo >/dev/null || die "cargo not in PATH"
command -v rsync >/dev/null || die "rsync not in PATH"
command -v npm >/dev/null || die "npm not in PATH"

# shellcheck source=lib.sh
source "$ROOT/scripts/lib.sh"
mkdir -p "$CACHE" "$RELEASE_DIR" "$OVERLAY/usr/bin" "$OVERLAY/usr/lib/cloud-hypervisor"

echo "building web ui"
(cd "$ROOT/web/ui" && npm ci --no-audit --no-fund && npm run build)

RUST_TARGET="aarch64-unknown-linux-gnu"
BINDIR="$ROOT/target/aarch64-unknown-linux-gnu/release"
host_m="$(uname -m)"
CARGO_PACKAGES=(-p pertisk-daemon -p pertisk-cli -p pertisk-tui)
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER \
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER \
  CC_x86_64_unknown_linux_gnu CC_aarch64_unknown_linux_gnu \
  AR_x86_64_unknown_linux_gnu AR_aarch64_unknown_linux_gnu
if [[ "$host_m" == aarch64 || "$host_m" == arm64 ]]; then
  BINDIR="$ROOT/target/release"
  cargo build --release --locked "${CARGO_PACKAGES[@]}"
else
  chmod +x "$ROOT/scripts/ci-ensure-zig.sh"
  "$ROOT/scripts/ci-ensure-zig.sh"
  case "$host_m" in
    x86_64|amd64) _za=x86_64 ;;
    *) _za=aarch64 ;;
  esac
  _zd="${HOME}/.local/zig/zig-linux-${_za}-${ZIG_VERSION:-0.13.0}"
  [[ -x "${_zd}/zig" ]] && export PATH="${_zd}:${PATH}"
  [[ -d "${HOME}/.cargo/bin" ]] && export PATH="${HOME}/.cargo/bin:${PATH}"
  command -v rustup >/dev/null && rustup target add "$RUST_TARGET"
  cargo zigbuild --release --locked --target "$RUST_TARGET" "${CARGO_PACKAGES[@]}"
fi
install -m 755 "$BINDIR/pertiskd" "$OVERLAY/usr/bin/pertiskd"
install -m 755 "$BINDIR/pertisk" "$OVERLAY/usr/bin/pertisk"
install -m 755 "$BINDIR/pertisk-tui" "$OVERLAY/usr/bin/pertisk-tui"
ensure_cloud_hypervisor aarch64
ensure_firmware aarch64
install -m 755 "$CLOUD_HYPERVISOR" "$OVERLAY/usr/bin/cloud-hypervisor"
install -m 644 "$FIRMWARE" "$OVERLAY/usr/lib/cloud-hypervisor/hypervisor-fw"
printf '%s\n' "$VERSION" >"$OVERLAY/etc/pertisk/version"

decompress_image() {
  local src="$1" dest="$2"
  if xz -t "$src" >/dev/null 2>&1; then
    xzcat "$src" >"$dest"
  elif gzip -t "$src" >/dev/null 2>&1; then
    gzip -dc "$src" >"$dest"
  elif command -v unzip >/dev/null && unzip -t "$src" >/dev/null 2>&1; then
    unzip -p "$src" '*.img' >"$dest" || unzip -p "$src" '*.raw' >"$dest"
  else
    cp --sparse=always "$src" "$dest"
  fi
}

WORKDIR="$(mktemp -d /tmp/pertisk-sbc.XXXXXX)"
cleanup() {
  set +e
  if [[ -n "${ROOTMNT:-}" ]]; then
    umount -R "$ROOTMNT" 2>/dev/null
  fi
  if [[ -n "${LOOP:-}" ]]; then
    losetup -d "$LOOP" 2>/dev/null
  fi
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

IMG="$WORKDIR/disk.img"
if [[ -n "$BASE_IMAGE" ]]; then
  [[ -f "$BASE_IMAGE" ]] || die "base image not found: $BASE_IMAGE"
  echo "using $BASE_IMAGE"
  decompress_image "$BASE_IMAGE" "$IMG"
else
  [[ -n "${IMAGE_URL:-}" ]] || die "no IMAGE_URL in $ENVF; pass --base-image"
  DL="$CACHE/$(basename "${IMAGE_URL}")-${BOARD}"
  echo "downloading $IMAGE_URL"
  curl -fL --retry 3 --retry-delay 2 -o "$DL.partial" "$IMAGE_URL"
  mv "$DL.partial" "$DL"
  if command -v file >/dev/null && file "$DL" | grep -qi 'html\|ascii text'; then
    die "download was HTML, not an image. Pass --base-image with the vendor .img"
  fi
  decompress_image "$DL" "$IMG"
fi

echo "growing image +2GiB for qemu packages"
truncate -s +2G "$IMG"
LOOP="$(losetup -Pf --show "$IMG")"
# Wait for partitions.
for _ in 1 2 3 4 5; do
  [[ -b "${LOOP}p2" || -b "${LOOP}p1" ]] && break
  sleep 0.5
  partprobe "$LOOP" || true
done
[[ -b "${LOOP}p1" ]] || die "no partitions on base image"

ROOTPART=""
BOOTPART=""
for part in "${LOOP}p2" "${LOOP}p1" "${LOOP}p3"; do
  [[ -b "$part" ]] || continue
  fstype="$(blkid -s TYPE -o value "$part" 2>/dev/null || true)"
  case "$fstype" in
    ext4|ext3|btrfs|xfs)
      [[ -z "$ROOTPART" ]] && ROOTPART="$part"
      ;;
    vfat|fat32|fat16)
      BOOTPART="$part"
      ;;
  esac
done
[[ -n "$ROOTPART" ]] || die "no ext4 root partition found"

if command -v growpart >/dev/null 2>&1; then
  growpart "$LOOP" "${ROOTPART##*p}" || true
elif command -v parted >/dev/null 2>&1; then
  parted -s "$LOOP" resizepart "${ROOTPART##*p}" 100% || true
fi
fstype="$(blkid -s TYPE -o value "$ROOTPART")"
if [[ "$fstype" == ext4 || "$fstype" == ext3 ]]; then
  e2fsck -fp "$ROOTPART" || true
  resize2fs "$ROOTPART" || true
fi

ROOTMNT="$WORKDIR/root"
mkdir -p "$ROOTMNT"
mount "$ROOTPART" "$ROOTMNT"
if [[ -n "$BOOTPART" ]]; then
  mkdir -p "$ROOTMNT/boot"
  if [[ -d "$ROOTMNT/boot/firmware" ]]; then
    mount "$BOOTPART" "$ROOTMNT/boot/firmware" || mount "$BOOTPART" "$ROOTMNT/boot"
  else
    mount "$BOOTPART" "$ROOTMNT/boot"
  fi
fi

echo "installing pertisk overlay (no systemd-networkd)"
rsync -a \
  --exclude 'etc/systemd/network/' \
  --exclude 'usr/lib/systemd/system-preset/50-pertisk.preset' \
  "$OVERLAY/" "$ROOTMNT/"
install -m 644 "$OVERLAY/usr/lib/systemd/system-preset/50-pertisk-sbc.preset" \
  "$ROOTMNT/usr/lib/systemd/system-preset/50-pertisk-sbc.preset"
chmod 755 "$ROOTMNT/usr/sbin/pertisk-kvm-check" \
  "$ROOTMNT/usr/sbin/pertisk-firstboot" \
  "$ROOTMNT/usr/sbin/pertisk-install" \
  "$ROOTMNT/usr/sbin/pertisk-host-bridge"
mkdir -p "$ROOTMNT/etc/pertisk" "$ROOTMNT/var/lib/pertisk"
printf '%s\n' "$BOARD" >"$ROOTMNT/etc/pertisk/board"
printf '%s\n' "$FAMILY" >"$ROOTMNT/etc/pertisk/family"
cp "$ROOTMNT/etc/pertisk/config.toml" "$ROOTMNT/var/lib/pertisk/config.toml"

# Headless: no vendor first-run wizard. Keep Armbian resize.
rm -f "$ROOTMNT/root/.not_logged_in_yet" \
  "$ROOTMNT/boot/orangepi_first_run.txt" \
  "$ROOTMNT/boot/armbian_first_run.txt" \
  "$ROOTMNT/boot/firmware/orangepi_first_run.txt"
touch "$ROOTMNT/boot/ssh" 2>/dev/null || true
touch "$ROOTMNT/boot/firmware/ssh" 2>/dev/null || true
echo pertisk >"$ROOTMNT/etc/hostname"
if [[ -f "$ROOTMNT/etc/hosts" ]] && ! grep -q 'pertisk' "$ROOTMNT/etc/hosts"; then
  sed -i 's/127.0.1.1.*/127.0.1.1\tpertisk/' "$ROOTMNT/etc/hosts" || true
fi

QEMU_STATIC=""
if [[ "$host_m" != aarch64 && "$host_m" != arm64 ]]; then
  chmod +x "$ROOT/scripts/ci-ensure-qemu-binfmt.sh"
  # shellcheck source=ci-ensure-qemu-binfmt.sh
  source "$ROOT/scripts/ci-ensure-qemu-binfmt.sh"
  for q in /usr/bin/qemu-aarch64-static /usr/bin/qemu-aarch64; do
    if [[ -x "$q" ]]; then
      QEMU_STATIC="$q"
      cp "$q" "$ROOTMNT/usr/bin/qemu-aarch64-static"
      chmod 755 "$ROOTMNT/usr/bin/qemu-aarch64-static"
      break
    fi
  done
  [[ -n "$QEMU_STATIC" ]] || die "install qemu-user-static for cross chroot"
fi

mount --bind /dev "$ROOTMNT/dev"
mount --bind /dev/pts "$ROOTMNT/dev/pts"
mount -t proc proc "$ROOTMNT/proc"
mount -t sysfs sys "$ROOTMNT/sys"
cp /etc/resolv.conf "$ROOTMNT/etc/resolv.conf.bak" 2>/dev/null || true
cp /etc/resolv.conf "$ROOTMNT/etc/resolv.conf"

chroot "$ROOTMNT" /bin/bash -s <<REMOTE
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
if command -v apt-get >/dev/null; then
  apt-get update -y
  apt-get install -y ${PACKAGES} openssh-server ca-certificates
fi
if command -v chpasswd >/dev/null; then
  echo 'root:pertisk' | chpasswd
fi
if id -u orangepi >/dev/null 2>&1; then
  echo 'orangepi:orangepi' | chpasswd || true
fi
if id -u pi >/dev/null 2>&1; then
  echo 'pi:raspberry' | chpasswd || true
fi
mkdir -p /etc/ssh/sshd_config.d
printf 'PermitRootLogin yes\nPasswordAuthentication yes\n' >/etc/ssh/sshd_config.d/pertisk.conf
systemctl enable pertisk-firstboot.service pertiskd.service || true
systemctl enable ssh.service 2>/dev/null || systemctl enable sshd.service 2>/dev/null || true
systemctl disable systemd-networkd.service systemd-networkd-wait-online.service 2>/dev/null || true
systemctl disable orangepi-firstlogin.service armbian-firstrun.service 2>/dev/null || true
systemctl enable serial-getty@${SERIAL}.service 2>/dev/null || true
REMOTE

if [[ -n "$QEMU_STATIC" ]]; then
  rm -f "$ROOTMNT/usr/bin/qemu-aarch64-static"
fi
if [[ -f "$ROOTMNT/etc/resolv.conf.bak" ]]; then
  mv "$ROOTMNT/etc/resolv.conf.bak" "$ROOTMNT/etc/resolv.conf"
fi

sync
umount "$ROOTMNT/dev/pts" "$ROOTMNT/dev" "$ROOTMNT/proc" "$ROOTMNT/sys" || true
umount -R "$ROOTMNT" || true
ROOTMNT=""
losetup -d "$LOOP"
LOOP=""

OUT_IMG="$RELEASE_DIR/pertisk-node-${VERSION}-${BOARD}.img"
OUT_XZ="${OUT_IMG}.xz"
cp --sparse=always "$IMG" "$OUT_IMG"
echo "compressing $OUT_XZ"
xz -T0 -6 -f "$OUT_IMG"
ls -lh "$OUT_XZ"
echo
echo "Flash: xzcat $OUT_XZ | sudo dd of=/dev/sdX bs=4M status=progress conv=fsync"
echo "Boot the SD, then http://<host>:7480/  (admin / see /etc/pertisk/admin)"
echo "Move OS to NVMe: pertisk-install --disk /dev/nvme0n1 --yes"
