#!/usr/bin/env bash
# Write a pertisk node image to a USB/disk. Linux only. Wipes --disk.
# Usage: sudo ./scripts/flash.sh --image out/pertisk-node.iso --disk /dev/sdX --yes
set -euo pipefail

IMAGE=""
DISK=""
YES=0

die() { echo "flash: $*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="$2"; shift 2 ;;
    --disk) DISK="$2"; shift 2 ;;
    --yes) YES=1; shift ;;
    -h|--help)
      echo "Usage: sudo $0 --image out/pertisk-node.raw.xz --disk /dev/sdX --yes"
      exit 0
      ;;
    *) die "unknown arg $1" ;;
  esac
done

[[ "$(uname -s)" == "Linux" ]] || die "Linux only"
[[ "$(id -u)" -eq 0 ]] || die "run as root"
[[ -n "$IMAGE" && -f "$IMAGE" ]] || die "pass --image path/to/pertisk-node.raw (or .img / .xz)"
[[ -n "$DISK" ]] || die "pass --disk /dev/sdX"
[[ -b "$DISK" ]] || die "$DISK is not a block device"
[[ "$YES" -eq 1 ]] || die "refusing to wipe $DISK without --yes"

root_src="$(findmnt -n -o SOURCE / || true)"
if [[ -n "$root_src" && "$root_src" == "$DISK"* ]]; then
  die "$DISK looks like the current root ($root_src)"
fi

echo "Writing $IMAGE -> $DISK"
case "$IMAGE" in
  *.xz)
    xzcat "$IMAGE" | dd of="$DISK" bs=4M status=progress conv=fsync
    ;;
  *)
    dd if="$IMAGE" of="$DISK" bs=4M status=progress conv=fsync
    ;;
esac
echo "flash: done. Boot $DISK on the target machine."
