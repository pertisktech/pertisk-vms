#!/usr/bin/env bash
# User-local GCC shims via zig cc (no root) for **cross** compiles only.
# Never wrap the host gcc: aws-lc built with host glibc cannot link against zig's 2.28 libc.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ZIG_VERSION="${ZIG_VERSION:-0.13.0}"

case "$(uname -m)" in
  x86_64|amd64)
    ZIG_ARCH=x86_64
    HOST_TRIPLE=x86_64-linux-gnu
    CROSS_PREFIX=aarch64-linux-gnu
    CROSS_ZIG=aarch64-linux-gnu.2.28
    CROSS_LINKER_ENV=CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
    ;;
  aarch64|arm64)
    ZIG_ARCH=aarch64
    HOST_TRIPLE=aarch64-linux-gnu
    CROSS_PREFIX=x86_64-linux-gnu
    CROSS_ZIG=x86_64-linux-gnu.2.28
    CROSS_LINKER_ENV=CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER
    ;;
  *)
    echo "unsupported arch for zig: $(uname -m)" >&2
    exit 1
    ;;
esac

PREFIX="${HOME}/.local/zig"
DIR="${PREFIX}/zig-linux-${ZIG_ARCH}-${ZIG_VERSION}"
mkdir -p "$PREFIX" "${HOME}/.local/bin"

rm -f "${HOME}/.local/bin/${HOST_TRIPLE}-gcc" "${HOME}/.local/bin/${HOST_TRIPLE}-g++" \
  "${HOME}/.local/bin/${HOST_TRIPLE}-ar"

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

install -m 755 "$ROOT/scripts/zig-cc.sh" "${HOME}/.local/bin/zig-cc.sh"

write_cc() {
  local name="$1"
  cat >"${HOME}/.local/bin/${name}" <<EOF
#!/bin/sh
export ZIG="${DIR}/zig"
export ZIG_CC_TARGET="${CROSS_ZIG}"
exec "${HOME}/.local/bin/zig-cc.sh" "\$@"
EOF
  chmod +x "${HOME}/.local/bin/${name}"
}

write_cc "${CROSS_PREFIX}-gcc"
write_cc "${CROSS_PREFIX}-g++"

cat >"${HOME}/.local/bin/${CROSS_PREFIX}-ar" <<EOF
#!/bin/sh
exec "${DIR}/zig" ar "\$@"
EOF
chmod +x "${HOME}/.local/bin/${CROSS_PREFIX}-ar"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${DIR}" >> "$GITHUB_PATH"
  echo "${HOME}/.local/bin" >> "$GITHUB_PATH"
fi
if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "PATH=${DIR}:${HOME}/.local/bin:${PATH}" >> "$GITHUB_ENV"
  echo "${CROSS_LINKER_ENV}=${HOME}/.local/bin/${CROSS_PREFIX}-gcc" >> "$GITHUB_ENV"
fi

echo "cross-cc ${CROSS_PREFIX}-gcc -> zig cc -target ${CROSS_ZIG} (strips --target=)"
echo "native gcc left alone (${HOST_TRIPLE})"
