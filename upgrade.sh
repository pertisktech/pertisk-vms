#!/usr/bin/env bash
# Install or upgrade pertisk on this Linux machine (real node).
# Safe to re-run: replaces binaries, restarts service, keeps /var/lib/pertisk.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "upgrade: run as root →  sudo $0" >&2
  exit 1
fi

VER="$(awk '/^\[workspace.package\]/{p=1} p && /^version =/{gsub(/"/,"",$3); print $3; exit}' Cargo.toml 2>/dev/null || echo '?')"
echo "==> pertisk node install/upgrade (version ${VER})"
echo "    data kept: /var/lib/pertisk"
echo

exec "$ROOT/scripts/install-node.sh"
