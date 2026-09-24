#!/usr/bin/env bash
# Build Anaconda product.img (cpio+gzip) with Pertisk branding.
# Usage: build-product-img.sh [VERSION] [OUT_PATH]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BRAND="$ROOT/packaging/anaconda-branding"
VERSION="${1:-0.1.0}"
OUT="${2:-}"

[[ -f "$BRAND/pixmaps/sidebar-bg.png" ]] || {
  echo "build-product-img: missing pixmaps; run: python3 $BRAND/gen-assets.py" >&2
  exit 1
}

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/pertisk-product-XXXXXX")"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

PROD="$WORKDIR/product"
mkdir -p "$PROD/usr/share/anaconda/pixmaps"
cp -f "$BRAND/pixmaps/"*.png "$PROD/usr/share/anaconda/pixmaps/"
cp -f "$BRAND/redhat.css" "$PROD/usr/share/anaconda/pixmaps/redhat.css"

# Anaconda hub title: "%(productName)s %(productVersion)s INSTALLATION" (uppercased).
# trim_product_version_for_ui() keeps only major.minor when Version has ≥2 dots
# (0.1.26 → 0.1). Put the full release in Product and leave Version empty so the
# banner reads "PERTISK VM 0.1.26 INSTALLATION".
{
  echo "[Main]"
  echo "Product=Pertisk VM ${VERSION}"
  echo "Version="
  sed -n '/^BugURL=/,$p' "$BRAND/buildstamp"
} >"$PROD/.buildstamp"

# Optional profile snippet: keep AlmaLinux detection, force our stylesheet path.
mkdir -p "$PROD/etc/anaconda/conf.d"
cat >"$PROD/etc/anaconda/conf.d/90-pertisk-branding.conf" <<'EOF'
# Pertisk product.img branding — stylesheet path matches AlmaLinux (rhel) profile.
[User Interface]
custom_stylesheet = /usr/share/anaconda/pixmaps/redhat.css

# AlmaLinux caps / at 70 GiB and gives the rest to /home. VM disks live under
# /var/lib/pertisk, so the grow volume must be that mount, not /home.
[Storage]
default_partitioning =
    /                 (min 10 GiB, max 70 GiB)
    /var/lib/pertisk  (min 10 GiB)
EOF

if [[ -z "$OUT" ]]; then
  OUT="$WORKDIR/../product.img"
fi
mkdir -p "$(dirname "$OUT")"
(
  cd "$PROD"
  find . | cpio -c -o 2>/dev/null | gzip -9 >"$OUT"
)

echo "$OUT"
ls -lh "$OUT" >&2
