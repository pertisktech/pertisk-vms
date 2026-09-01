#!/usr/bin/env bash
# Build UI + pertisk binaries and copy into appliance VM 901 on this Proxmox host.
# Safe wrapper around scripts/deploy-appliance.sh (stop VM, mount offline, sync, start).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

echo "==> UI (baked into pertiskd via rust-embed)"
(cd web/ui && npm ci && npm run build)

echo "==> Deploy binaries to VM 901"
exec "$ROOT/scripts/deploy-appliance.sh" 901
