#!/usr/bin/env bash
# Build AlmaLinux 10 Kickstart installer ISO with embedded pertisk-vms RPM.
# Usage: ./scripts/build-alma-iso.sh [VERSION]
#        make release-alma-iso VERSION=0.1.0
#
# Requires: Linux x86_64, lorax (mkksiso), rpm-build, cargo, npm.
# Downloads AlmaLinux 10.2 boot ISO on first run (override with PERTISK_ALMA_ISO).
#
# Default base is the ~1 GiB network boot.iso (needs net during Anaconda).
# For offline installs with enough disk (~12 GiB free), use the DVD:
#   PERTISK_ALMA_ISO_URL=https://repo.almalinux.org/almalinux/10/isos/x86_64/AlmaLinux-10.2-x86_64-dvd.iso \
#     make release-alma-iso VERSION=0.1.0
# and switch kickstart url/repo → cdrom.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
RELEASE_DIR="$ROOT/release"
CACHE="${PERTISK_HOME:-$HOME/.pertisk}/images"
KS="$ROOT/packaging/kickstart/pertisk-node.ks"
VERSION="${1:-${PERTISK_VERSION:-}}"

# AlmaLinux 10 boot (x86_64) — small; packages pulled over the network at install.
# Mirror listing: https://repo.almalinux.org/almalinux/10/isos/x86_64/
ALMA_ISO_URL="${PERTISK_ALMA_ISO_URL:-https://repo.almalinux.org/almalinux/10/isos/x86_64/AlmaLinux-10.2-x86_64-boot.iso}"
ALMA_ISO="${PERTISK_ALMA_ISO:-}"

die() { echo "build-alma-iso: $*" >&2; exit 1; }

cargo_version() {
  awk '/^\[workspace.package\]/{p=1} p && /^version =/{gsub(/"/,"",$3); print $3; exit}' "$ROOT/Cargo.toml"
}

git_version() {
  git -C "$ROOT" describe --tags --always --dirty 2>/dev/null | sed 's/^v//' || true
}

# Require enough free bytes on the filesystem hosting $1 for an output ≈ $2 bytes.
need_free_space() {
  local dir="$1" need="$2"
  local avail
  avail="$(df -PB1 "$dir" | awk 'NR==2{print $4}')"
  if [[ "$avail" -lt "$need" ]]; then
    local need_h avail_h
    need_h="$(numfmt --to=iec --suffix=B "$need" 2>/dev/null || echo "${need}B")"
    avail_h="$(numfmt --to=iec --suffix=B "$avail" 2>/dev/null || echo "${avail}B")"
    die "not enough free space under $dir (need ~$need_h, have $avail_h). Free disk, set RELEASE_DIR elsewhere, or use boot.iso (default) instead of the DVD."
  fi
}

[[ "$(uname -s)" == "Linux" ]] || die "build on Linux"
[[ "$(uname -m)" == "x86_64" ]] || die "x86_64 required"
command -v mkksiso >/dev/null || die "install lorax (provides mkksiso): dnf install lorax"
command -v curl >/dev/null || die "curl not in PATH"
[[ -f "$KS" ]] || die "missing $KS"

VERSION="${VERSION:-$(git_version)}"
VERSION="${VERSION:-$(cargo_version)}"
[[ -n "$VERSION" ]] || die "set VERSION"
VERSION="${VERSION#v}"

mkdir -p "$CACHE" "$RELEASE_DIR"

if [[ -z "$ALMA_ISO" ]]; then
  ALMA_ISO="$CACHE/$(basename "$ALMA_ISO_URL")"
fi
if [[ ! -f "$ALMA_ISO" ]]; then
  echo "downloading AlmaLinux ISO → $ALMA_ISO"
  curl -fL --retry 3 -o "$ALMA_ISO.partial" "$ALMA_ISO_URL"
  mv "$ALMA_ISO.partial" "$ALMA_ISO"
fi
[[ -f "$ALMA_ISO" ]] || die "AlmaLinux ISO not found at $ALMA_ISO"

echo "=== building RPM ==="
chmod +x "$ROOT/scripts/build-rpm.sh"
"$ROOT/scripts/build-rpm.sh" "$VERSION"

RPM="$(ls -1t "$RELEASE_DIR"/pertisk-vms-*.rpm 2>/dev/null | head -n1 || true)"
[[ -n "$RPM" && -f "$RPM" ]] || die "no RPM in $RELEASE_DIR"

ADD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/pertisk-iso-XXXXXX")"
cleanup() { rm -rf "$ADD_DIR"; }
trap cleanup EXIT

mkdir -p "$ADD_DIR/pertisk"
cp -f "$RPM" "$ADD_DIR/pertisk/"

OUT_ISO="$RELEASE_DIR/pertisk-node-${VERSION}-x86_64.iso"
rm -f "$OUT_ISO"

# xorriso writes a full new image ≈ input size + payload; need that free on $RELEASE_DIR.
ISO_BYTES="$(stat -c%s "$ALMA_ISO" 2>/dev/null || stat -f%z "$ALMA_ISO")"
PAYLOAD_BYTES="$(du -sb "$ADD_DIR" | awk '{print $1}')"
# +64 MiB slack for EFI/boot rebuild overhead
need_free_space "$RELEASE_DIR" "$((ISO_BYTES + PAYLOAD_BYTES + 64*1024*1024))"

CMDLINE="inst.ks=cdrom:/pertisk-node.ks console=tty0 console=ttyS0 inst.text"

echo "=== mkksiso (base=$(basename "$ALMA_ISO")) ==="
# --ks injects the Kickstart onto the ISO; --add places RPMs at /pertisk/.
mkksiso \
  --ks "$KS" \
  --add "$ADD_DIR/pertisk" \
  --cmdline "$CMDLINE" \
  "$ALMA_ISO" \
  "$OUT_ISO"

echo
echo "=== iso ==="
ls -lh "$OUT_ISO"
echo "Flash (USB): sudo dd if=$OUT_ISO of=/dev/sdX bs=4M status=progress conv=fsync"
echo "Or:          sudo ./scripts/flash.sh --image $OUT_ISO --disk /dev/sdX --yes"
echo "Boot UEFI with Secure Boot disabled. Anaconda installs AlmaLinux + pertiskd (network required for boot.iso)."
