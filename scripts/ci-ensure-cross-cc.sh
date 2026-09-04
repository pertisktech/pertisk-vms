#!/usr/bin/env bash
# User-local aarch64/x86_64 GCC shims via zig cc (no root).
# cc-rs looks for aarch64-linux-gnu-gcc when cross-compiling ring/aws-lc.
set -euo pipefail

ZIG_VERSION="${ZIG_VERSION:-0.13.0}"

case "$(uname -m)" in
  x86_64|amd64) ZIG_ARCH=x86_64 ;;
  aarch64|arm64) ZIG_ARCH=aarch64 ;;
  *)
    echo "unsupported arch for zig: $(uname -m)" >&2
    exit 1
    ;;
esac

PREFIX="${HOME}/.local/zig"
DIR="${PREFIX}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}"
mkdir -p "$PREFIX" "${HOME}/.local/bin"

if [[ ! -x "${DIR}/zig" ]]; then
  tmp="$(mktemp)"
  url="https://ziglang.org/download/${ZIG_VERSION}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}.tar.xz"
  alt="https://github.com/ziglang/zig/releases/download/${ZIG_VERSION}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}.tar.xz"
  echo "Downloading zig ${ZIG_VERSION} (${ZIG_ARCH})..."
  if ! curl -fsSL --retry 5 --retry-delay 2 -o "$tmp" "$url"; then
    curl -fsSL --retry 5 --retry-delay 2 -o "$tmp" "$alt"
  fi
  tar -xJf "$tmp" -C "$PREFIX"
  rm -f "$tmp"
fi

export PATH="${DIR}:${HOME}/.local/bin:${PATH}"
command -v zig >/dev/null || {
  echo "::error::zig not found after install" >&2
  exit 1
}
echo "zig $(zig version)"

write_cc_wrapper() {
  local name="$1"
  local target="$2"
  local dest="${HOME}/.local/bin/${name}"
  cat >"$dest" <<EOF
#!/bin/sh
exec "${DIR}/zig" cc -target ${target} "\$@"
EOF
  chmod +x "$dest"
}

write_ar_wrapper() {
  local name="$1"
  local dest="${HOME}/.local/bin/${name}"
  cat >"$dest" <<EOF
#!/bin/sh
exec "${DIR}/zig" ar "\$@"
EOF
  chmod +x "$dest"
}

# glibc 2.28 is new enough for Debian Trixie and old enough to link on most distros.
write_cc_wrapper aarch64-linux-gnu-gcc aarch64-linux-gnu.2.28
write_ar_wrapper aarch64-linux-gnu-ar
write_cc_wrapper x86_64-linux-gnu-gcc x86_64-linux-gnu.2.28
write_ar_wrapper x86_64-linux-gnu-ar

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${DIR}" >> "$GITHUB_PATH"
  echo "${HOME}/.local/bin" >> "$GITHUB_PATH"
fi
if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "PATH=${DIR}:${HOME}/.local/bin:${PATH}" >> "$GITHUB_ENV"
  echo "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=${HOME}/.local/bin/aarch64-linux-gnu-gcc" >> "$GITHUB_ENV"
  echo "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=${HOME}/.local/bin/x86_64-linux-gnu-gcc" >> "$GITHUB_ENV"
fi

echo "cross-cc aarch64-linux-gnu-gcc -> zig cc -target aarch64-linux-gnu.2.28"
echo "cross-cc x86_64-linux-gnu-gcc  -> zig cc -target x86_64-linux-gnu.2.28"
