#!/usr/bin/env bash
# Build pertisk-vms RPM for AlmaLinux / RHEL x86_64.
# Usage: ./scripts/build-rpm.sh [VERSION]
# Output: release/pertisk-vms-<version>-1.*.x86_64.rpm
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OVERLAY="$ROOT/iso/overlay"
ALMA_OVERLAY="$ROOT/packaging/rpm/alma-overlay"
SPEC="$ROOT/packaging/rpm/pertisk-vms.spec"
RELEASE_DIR="$ROOT/release"
VERSION="${1:-${PERTISK_VERSION:-}}"

die() { echo "build-rpm: $*" >&2; exit 1; }

cargo_version() {
  awk '/^\[workspace.package\]/{p=1} p && /^version =/{gsub(/"/,"",$3); print $3; exit}' "$ROOT/Cargo.toml"
}

git_version() {
  git -C "$ROOT" describe --tags --always --dirty 2>/dev/null | sed 's/^v//' || true
}

[[ "$(uname -s)" == "Linux" ]] || die "build the RPM on Linux (this host is $(uname -s))"
[[ "$(uname -m)" == "x86_64" ]] || die "x86_64 host required for this RPM (got $(uname -m))"
command -v cargo >/dev/null || die "cargo not in PATH"
command -v rpmbuild >/dev/null || die "install rpm-build (dnf install rpm-build)"
command -v npm >/dev/null || die "npm not in PATH"
command -v curl >/dev/null || die "curl not in PATH"

VERSION="${VERSION:-$(git_version)}"
VERSION="${VERSION:-$(cargo_version)}"
[[ -n "$VERSION" ]] || die "set VERSION"
VERSION="${VERSION#v}"
# RPM versions cannot contain some git dirty markers cleanly; sanitize lightly.
RPM_VERSION="$(echo "$VERSION" | tr -c 'A-Za-z0-9._+' '_')"

TOP="$ROOT/packaging/rpm/rpmbuild"
PAYLOAD="$TOP/SOURCES/payload"
rm -rf "$TOP"
mkdir -p "$TOP"/{BUILD,RPMS,SOURCES,SPECS,SRPMS} "$PAYLOAD" "$RELEASE_DIR"

CACHE="${PERTISK_HOME:-$HOME/.pertisk}/images"
mkdir -p "$CACHE"
# shellcheck source=lib.sh
source "$ROOT/scripts/lib.sh"

echo "building web ui"
(cd "$ROOT/web/ui" && npm ci --no-audit --no-fund && npm run build)

echo "building release binaries (x86_64 v$VERSION)"
pertisk_vms_VERSION="$VERSION" cargo build --release --locked \
  -p pertisk-daemon -p pertisk-cli -p pertisk-tui
BINDIR="$ROOT/target/release"

ensure_cloud_hypervisor x86_64
ensure_firmware x86_64

stage() {
  local src="$1" dest="$2" mode="${3:-}"
  install -d "$(dirname "$PAYLOAD$dest")"
  if [[ -n "$mode" ]]; then
    install -m "$mode" "$src" "$PAYLOAD$dest"
  else
    cp -a "$src" "$PAYLOAD$dest"
  fi
}

echo "staging payload"
install -d \
  "$PAYLOAD/usr/bin" \
  "$PAYLOAD/usr/sbin" \
  "$PAYLOAD/usr/lib/cloud-hypervisor" \
  "$PAYLOAD/usr/lib/systemd/system" \
  "$PAYLOAD/usr/lib/systemd/system-preset" \
  "$PAYLOAD/etc/pertisk" \
  "$PAYLOAD/etc/modules-load.d" \
  "$PAYLOAD/etc/ssh/sshd_config.d" \
  "$PAYLOAD/etc/NetworkManager/conf.d" \
  "$PAYLOAD/etc/systemd/system/getty@tty1.service.d" \
  "$PAYLOAD/etc/systemd/system/serial-getty@ttyS0.service.d" \
  "$PAYLOAD/etc/systemd/journald.conf.d"

install -m 755 "$BINDIR/pertiskd" "$PAYLOAD/usr/bin/pertiskd"
install -m 755 "$BINDIR/pertisk" "$PAYLOAD/usr/bin/pertisk"
install -m 755 "$BINDIR/pertisk-tui" "$PAYLOAD/usr/bin/pertisk-tui"
install -m 755 "$CLOUD_HYPERVISOR" "$PAYLOAD/usr/bin/cloud-hypervisor"
install -m 644 "$FIRMWARE" "$PAYLOAD/usr/lib/cloud-hypervisor/hypervisor-fw"

for s in pertisk-firstboot pertisk-kvm-check pertisk-host-bridge pertisk-bootfix \
         pertisk-net pertisk-console pertisk-fix-hosts pertisk-fix-dns; do
  install -m 755 "$OVERLAY/usr/sbin/$s" "$PAYLOAD/usr/sbin/$s"
done

for u in pertiskd.service pertisk-firstboot.service pertisk-net.service pertisk-bootfix.service; do
  install -m 644 "$OVERLAY/usr/lib/systemd/system/$u" "$PAYLOAD/usr/lib/systemd/system/$u"
done

install -m 644 "$OVERLAY/etc/pertisk/config.toml" "$PAYLOAD/etc/pertisk/config.toml"
install -m 644 "$OVERLAY/etc/pertisk/daemon.env" "$PAYLOAD/etc/pertisk/daemon.env"
install -m 644 "$OVERLAY/etc/pertisk/join" "$PAYLOAD/etc/pertisk/join"
install -m 644 "$OVERLAY/etc/modules-load.d/pertisk-net.conf" "$PAYLOAD/etc/modules-load.d/pertisk-net.conf"
install -m 644 "$OVERLAY/etc/ssh/sshd_config.d/pertisk.conf" "$PAYLOAD/etc/ssh/sshd_config.d/pertisk.conf"
install -m 644 "$OVERLAY/etc/systemd/system/getty@tty1.service.d/autologin.conf" \
  "$PAYLOAD/etc/systemd/system/getty@tty1.service.d/autologin.conf"
install -m 644 "$OVERLAY/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf" \
  "$PAYLOAD/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf"
install -m 644 "$OVERLAY/etc/systemd/journald.conf.d/pertisk-no-console.conf" \
  "$PAYLOAD/etc/systemd/journald.conf.d/pertisk-no-console.conf"

# Alma-specific overlays (install policy, NM, presets).
cp -a "$ALMA_OVERLAY"/. "$PAYLOAD"/
# setup/systemd already own these; packaging them causes Anaconda DNF file conflicts.
rm -f "$PAYLOAD/etc/hostname" "$PAYLOAD/etc/hosts" "$PAYLOAD/etc/motd"
printf '%s\n' "$VERSION" >"$PAYLOAD/etc/pertisk/version"

# After=network-online is enough; NetworkManager provides it on Alma.
tmp="$(mktemp)"
sed 's/After=systemd-networkd.service/After=network-online.target NetworkManager.service/;s/Wants=systemd-networkd.service/Wants=network-online.target/' \
  "$PAYLOAD/usr/lib/systemd/system/pertisk-net.service" >"$tmp"
mv "$tmp" "$PAYLOAD/usr/lib/systemd/system/pertisk-net.service"

cp "$SPEC" "$TOP/SPECS/pertisk-vms.spec"

echo "rpmbuild v$RPM_VERSION"
rpmbuild -bb \
  --define "_topdir $TOP" \
  --define "pertisk_version $RPM_VERSION" \
  "$TOP/SPECS/pertisk-vms.spec"

RPM="$(find "$TOP/RPMS" -name 'pertisk-vms-*.rpm' | head -n1)"
[[ -n "$RPM" && -f "$RPM" ]] || die "rpmbuild produced no RPM"
cp -f "$RPM" "$RELEASE_DIR/"
echo
echo "=== rpm ==="
ls -lh "$RELEASE_DIR"/pertisk-vms-*.rpm
echo "$RELEASE_DIR/$(basename "$RPM")"
