#!/usr/bin/env bash
# Register qemu-aarch64 binfmt so dpkg can run arm64 maintainer scripts on amd64.
# libc6:arm64 preinst is an aarch64 binary; without binfmt it fails with Exec format error.
set -euo pipefail

binfmt_aarch64_enabled() {
  local f
  for f in /proc/sys/fs/binfmt_misc/qemu-aarch64 /proc/sys/fs/binfmt_misc/qemu-arm64; do
    [[ -f "$f" ]] || continue
    grep -q '^enabled' "$f" && return 0
  done
  return 1
}

dump_binfmt() {
  echo "binfmt_misc:"
  ls -la /proc/sys/fs/binfmt_misc/ 2>/dev/null || echo "(missing)"
}

if [[ "$(uname -m)" == "aarch64" || "$(uname -m)" == "arm64" ]]; then
  echo "host is aarch64; qemu-aarch64 binfmt not required"
  return 0 2>/dev/null || exit 0
fi

if binfmt_aarch64_enabled; then
  echo "qemu-aarch64 binfmt already enabled"
  return 0 2>/dev/null || exit 0
fi

echo "qemu-aarch64 binfmt is not registered (needed for cross-arch mkosi/dpkg)"

if command -v docker >/dev/null && docker info >/dev/null 2>&1; then
  echo "Registering binfmt via docker run --privileged tonistiigi/binfmt --install arm64"
  docker run --rm --privileged tonistiigi/binfmt --install arm64 || true
fi

if binfmt_aarch64_enabled; then
  echo "qemu-aarch64 binfmt enabled via docker"
  dump_binfmt
  return 0 2>/dev/null || exit 0
fi

if command -v sudo >/dev/null && sudo -n true 2>/dev/null; then
  if command -v dnf >/dev/null; then
    sudo -n dnf install -y qemu-user-static qemu-user-binfmt || sudo -n dnf install -y qemu-user-static || true
  elif command -v apt-get >/dev/null; then
    sudo -n apt-get install -y qemu-user-static binfmt-support || true
    sudo -n update-binfmts --enable qemu-aarch64 || true
  fi
  sudo -n systemctl restart systemd-binfmt 2>/dev/null || true
fi

if binfmt_aarch64_enabled; then
  echo "qemu-aarch64 binfmt enabled via packages"
  dump_binfmt
  return 0 2>/dev/null || exit 0
fi

echo "::error::arm64 mkosi cannot run dpkg maintainer scripts on this amd64 host (Exec format error on libc6:arm64)."
echo "On the runner (as root), register qemu-aarch64 binfmt:"
echo "  docker run --rm --privileged tonistiigi/binfmt --install arm64"
dump_binfmt
exit 1
