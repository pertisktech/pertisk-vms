#!/usr/bin/env bash
# Build a bootable, flashable raw disk image (mkosi) on Linux.
# Usage: ./scripts/build-iso.sh [amd64|arm64] [VERSION]
#        make release-amd VERSION=0.1.0
#        make release-arm VERSION=0.1.0
# Output: release/pertisk-node-<version>-<arch>.raw
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"
OUT="$ROOT/out"
RELEASE_DIR="$ROOT/release"
FORMAT="${PERTISK_IMAGE_FORMAT:-disk}"
ARCH="${PERTISK_ARCH:-}"
VERSION="${PERTISK_VERSION:-}"

die() { echo "build-iso: $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage: ./scripts/build-iso.sh [amd64|arm64] [VERSION]

Build a flashable raw disk image with mkosi (Linux only).

  make release-amd VERSION=0.1.0
  make release-arm VERSION=0.1.0

Output: release/pertisk-node-<version>-<arch>.raw
Flash:  sudo ./scripts/flash.sh --image release/pertisk-node-<version>-amd64.raw --disk /dev/sdX --yes
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --arch) ARCH="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    amd64|x86_64|x86-64|arm64|aarch64)
      ARCH="$1"
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

normalize_arch() {
  case "$1" in
    amd64|x86_64|x86-64) echo amd64 ;;
    arm64|aarch64) echo arm64 ;;
    *) die "unsupported arch $1 (need amd64 or arm64)" ;;
  esac
}

host_arch() {
  case "$(uname -m)" in
    x86_64) echo amd64 ;;
    aarch64|arm64) echo arm64 ;;
    *) die "unsupported host $(uname -m)" ;;
  esac
}

[[ "$(uname -s)" == "Linux" ]] || die "build the image on Linux (this host is $(uname -s))"
export PATH="${HOME}/.local/bin:${PATH}"
command -v cargo >/dev/null || die "cargo not in PATH"
command -v mkosi >/dev/null || die "install mkosi (https://github.com/systemd/mkosi). Debian: apt install mkosi  |  CI: ./scripts/ci-install-deps.sh"
command -v curl >/dev/null || die "curl not in PATH"
command -v npm >/dev/null || die "npm not in PATH (install Node.js to build the embedded web UI)"
[[ "$FORMAT" == "disk" ]] || die "this script builds a raw disk image only"

ARCH="$(normalize_arch "${ARCH:-$(host_arch)}")"
VERSION="${VERSION:-$(git_version)}"
VERSION="${VERSION:-$(cargo_version)}"
[[ -n "$VERSION" ]] || die "set VERSION (e.g. make release-amd VERSION=0.1.0)"
VERSION="${VERSION#v}"

case "$ARCH" in
  amd64)
    MKOSI_ARCH="x86-64"
    RUST_TARGET="x86_64-unknown-linux-gnu"
    UNAME_ARCH="x86_64"
    CROSS_CC="x86_64-linux-gnu-gcc"
    ;;
  arm64)
    MKOSI_ARCH="arm64"
    RUST_TARGET="aarch64-unknown-linux-gnu"
    UNAME_ARCH="aarch64"
    CROSS_CC="aarch64-linux-gnu-gcc"
    ;;
esac

OUTPUT_STEM="pertisk-node-${VERSION}-${ARCH}"
OUTPUT_RAW="${OUTPUT_STEM}.raw"

# shellcheck source=lib.sh
CACHE="${PERTISK_HOME:-$HOME/.pertisk}/images"
mkdir -p "$CACHE" "$OUT" "$RELEASE_DIR" "$OVERLAY/usr/bin" "$OVERLAY/usr/lib/cloud-hypervisor" "$OVERLAY/etc/pertisk"
# shellcheck source=lib.sh
source "$ROOT/scripts/lib.sh"

echo "building web ui"
(cd "$ROOT/web/ui" && npm ci --no-audit --no-fund && npm run build)

HOST="$(host_arch)"
CARGO_ARGS=(build --release --locked -p pertisk-daemon -p pertisk-cli -p pertisk-tui)
BINDIR="$ROOT/target/release"
if [[ "$HOST" != "$ARCH" ]]; then
  echo "cross-compiling $ARCH on $HOST host ($RUST_TARGET)"
  if command -v rustup >/dev/null; then
    rustup target add "$RUST_TARGET"
  fi
  if command -v "$CROSS_CC" >/dev/null; then
    case "$ARCH" in
      arm64) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$CROSS_CC" ;;
      amd64) export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$CROSS_CC" ;;
    esac
  else
    echo "build-iso: ${CROSS_CC} not found; cargo --target may fail" >&2
  fi
  CARGO_ARGS+=(--target "$RUST_TARGET")
  BINDIR="$ROOT/target/${RUST_TARGET}/release"
fi

echo "building release binaries ($ARCH v$VERSION)"
pertisk_vms_VERSION="$VERSION" cargo "${CARGO_ARGS[@]}"
install -m 755 "$BINDIR/pertiskd" "$OVERLAY/usr/bin/pertiskd"
install -m 755 "$BINDIR/pertisk" "$OVERLAY/usr/bin/pertisk"
install -m 755 "$BINDIR/pertisk-tui" "$OVERLAY/usr/bin/pertisk-tui"

ensure_cloud_hypervisor "$UNAME_ARCH"
ensure_firmware "$UNAME_ARCH"
install -m 755 "$CLOUD_HYPERVISOR" "$OVERLAY/usr/bin/cloud-hypervisor"
install -m 644 "$FIRMWARE" "$OVERLAY/usr/lib/cloud-hypervisor/hypervisor-fw"
chmod 755 "$OVERLAY/usr/sbin/pertisk-kvm-check" "$OVERLAY/usr/sbin/pertisk-firstboot" "$OVERLAY/usr/sbin/pertisk-install"
printf '%s\n' "$VERSION" >"$OVERLAY/etc/pertisk/version"

echo "mkosi format=$FORMAT architecture=$MKOSI_ARCH version=$VERSION (needs root for the image)"
MKOSI_BIN="$(command -v mkosi)"
mkosi_cmd=("$MKOSI_BIN")
if [[ "$(id -u)" -ne 0 ]] && command -v sudo >/dev/null && sudo -n true 2>/dev/null; then
  mkosi_cmd=(sudo -n "$MKOSI_BIN")
fi
if ! grep -qiE 'debian|ubuntu' /etc/os-release 2>/dev/null; then
  echo "host is not Debian/Ubuntu; using mkosi tools tree"
  mkosi_cmd+=(--tools-tree default)
fi
(
  cd "$ROOT/iso"
  "${mkosi_cmd[@]}" --force --format "$FORMAT" \
    --architecture "$MKOSI_ARCH" \
    --image-version "$VERSION" \
    --output "$OUTPUT_STEM"
)

SRC=""
for candidate in "$OUT/$OUTPUT_RAW" "$OUT/${OUTPUT_STEM}" "$ROOT/iso/$OUTPUT_RAW"; do
  if [[ -f "$candidate" ]]; then
    SRC="$candidate"
    break
  fi
done
[[ -n "$SRC" ]] || die "mkosi finished but $OUTPUT_RAW was not found in out/"

cp -f "$SRC" "$RELEASE_DIR/$OUTPUT_RAW"
ln -sfn "$OUTPUT_RAW" "$OUT/pertisk-node-${ARCH}.raw"
if [[ "$ARCH" == "$(host_arch)" ]]; then
  ln -sfn "$OUTPUT_RAW" "$OUT/pertisk-node.raw"
fi

echo
echo "=== image ==="
ls -lh "$RELEASE_DIR/$OUTPUT_RAW"
echo "Flash: sudo ./scripts/flash.sh --image $RELEASE_DIR/$OUTPUT_RAW --disk /dev/sdX --yes"
echo "Then boot USB. To install to NVMe: pertisk-install --list && pertisk-install --disk /dev/nvme0n1 --yes"
