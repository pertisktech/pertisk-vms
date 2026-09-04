#!/usr/bin/env bash
# Install mkosi (>=25) and image-build deps on a self-hosted Linux runner.
# Debian/Ubuntu (apt) and RHEL/AlmaLinux/Rocky (dnf/yum).
set -euo pipefail

export PATH="${HOME}/.local/bin:${PATH}"

MIN_MKOSI=25
MKOSI_TAG="${MKOSI_TAG:-v25.3}"
VENV="${HOME}/.local/share/pertisk-mkosi"

mkosi_major() {
  command -v mkosi >/dev/null 2>&1 || return 1
  mkosi --version 2>/dev/null | grep -oE '[0-9]+' | head -n1
}

mkosi_ok() {
  local major
  major="$(mkosi_major || true)"
  [[ -n "${major:-}" && "$major" -ge "$MIN_MKOSI" ]]
}

sudo_cmd() {
  if [[ "$(id -u)" -eq 0 ]]; then
    echo ""
  elif sudo -n true 2>/dev/null; then
    echo "sudo -n"
  else
    echo ""
  fi
}

install_host_packages() {
  local SUDO
  SUDO="$(sudo_cmd)"
  if [[ -z "$SUDO" && "$(id -u)" -ne 0 ]]; then
    echo "::warning::No passwordless sudo; installing mkosi via pip only."
    return 0
  fi

  if command -v apt-get >/dev/null 2>&1; then
    $SUDO apt-get update
    $SUDO apt-get install -y --no-install-recommends \
      python3 python3-venv python3-pip \
      bubblewrap uidmap systemd-container \
      qemu-user-static qemu-system-x86 qemu-system-arm \
      gcc-aarch64-linux-gnu gcc-x86-64-linux-gnu \
      xz-utils debian-archive-keyring \
      mtools dosfstools e2fsprogs squashfs-tools \
      ca-certificates curl
  elif command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y \
      python3 python3-pip python3-virtualenv \
      bubblewrap systemd-container \
      qemu-user-static xz \
      gcc gcc-c++ \
      e2fsprogs dosfstools mtools \
      ca-certificates curl
    $SUDO dnf install -y gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu \
      gcc-x86_64-linux-gnu binutils-x86_64-linux-gnu \
      qemu-system-x86 qemu-system-aarch64 dpkg debian-keyring \
      2>/dev/null || true
  elif command -v yum >/dev/null 2>&1; then
    $SUDO yum install -y python3 python3-pip xz gcc gcc-c++ curl ca-certificates
  else
    echo "::warning::No apt-get/dnf/yum; skipping host packages."
  fi
}

install_mkosi_pip() {
  # mkosi is not published on PyPI; install a tagged GitHub source tarball.
  mkdir -p "${HOME}/.local/bin"
  local tmp src
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  echo "Downloading systemd/mkosi ${MKOSI_TAG}"
  curl -fsSL "https://github.com/systemd/mkosi/archive/refs/tags/${MKOSI_TAG}.tar.gz" -o "$tmp/mkosi.tgz"
  tar -xzf "$tmp/mkosi.tgz" -C "$tmp"
  src="$(find "$tmp" -mindepth 1 -maxdepth 1 -type d -name 'mkosi-*' | head -n1)"
  [[ -n "$src" ]] || {
    echo "::error::Failed to unpack mkosi ${MKOSI_TAG}" >&2
    return 1
  }
  if python3 -m venv "$VENV" 2>/dev/null; then
    "$VENV/bin/pip" install setuptools wheel
    "$VENV/bin/pip" install "$src"
    ln -sfn "$VENV/bin/mkosi" "${HOME}/.local/bin/mkosi"
  else
    python3 -m pip install --user setuptools wheel
    python3 -m pip install --user "$src"
  fi
}

if mkosi_ok; then
  echo "mkosi already present: $(mkosi --version | head -n1)"
else
  install_host_packages
  if ! mkosi_ok; then
    echo "Installing mkosi ${MKOSI_TAG} from GitHub"
    install_mkosi_pip
  fi
fi

if ! mkosi_ok; then
  echo "::error::mkosi ${MIN_MKOSI}+ is required (iso/mkosi.conf MinimumVersion=${MIN_MKOSI})." >&2
  echo "Install: ./scripts/ci-install-deps.sh  (or pip install git+https://github.com/systemd/mkosi.git@${MKOSI_TAG})" >&2
  exit 1
fi

echo "mkosi=$(command -v mkosi) $(mkosi --version | head -n1)"
if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${HOME}/.local/bin" >> "$GITHUB_PATH"
fi
if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "PATH=${HOME}/.local/bin:${PATH}" >> "$GITHUB_ENV"
fi
