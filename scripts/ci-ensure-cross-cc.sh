#!/usr/bin/env bash
# User-local GNU cross-gcc (Bootlin), no root. Zig cc is not used: cc-rs treats
# zig as clang, and zig's driver then invokes ld.lld for `-c` and never writes .o
# (ring, libsqlite3-sys, aws-lc-sys).
set -euo pipefail

BOOTLIN_REL="${BOOTLIN_REL:-2024.05-1}"

case "$(uname -m)" in
  x86_64|amd64)
    HOST_TRIPLE=x86_64-linux-gnu
    CROSS_PREFIX=aarch64-linux-gnu
    BOOTLIN_ARCH=aarch64
    CROSS_LINKER_ENV=CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
    ;;
  aarch64|arm64)
    HOST_TRIPLE=aarch64-linux-gnu
    CROSS_PREFIX=x86_64-linux-gnu
    BOOTLIN_ARCH=x86-64
    CROSS_LINKER_ENV=CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER
    ;;
  *)
    echo "unsupported arch for cross-gcc: $(uname -m)" >&2
    exit 1
    ;;
esac

TARBALL="${BOOTLIN_ARCH}--glibc--stable-${BOOTLIN_REL}.tar.xz"
URL="https://toolchains.bootlin.com/downloads/releases/toolchains/${BOOTLIN_ARCH}/tarballs/${TARBALL}"
PREFIX="${HOME}/.local/bootlin"
EXTRACT="${PREFIX}/${BOOTLIN_ARCH}--glibc--stable-${BOOTLIN_REL}"
mkdir -p "$PREFIX" "${HOME}/.local/bin"

rm -f "${HOME}/.local/bin/${HOST_TRIPLE}-gcc" "${HOME}/.local/bin/${HOST_TRIPLE}-g++" \
  "${HOME}/.local/bin/${HOST_TRIPLE}-ar"

if [[ ! -d "$EXTRACT/bin" ]]; then
  tmp="$(mktemp)"
  echo "Downloading Bootlin ${TARBALL}"
  curl -fsSL --retry 5 --retry-delay 2 -o "$tmp" "$URL"
  tar -xJf "$tmp" -C "$PREFIX"
  rm -f "$tmp"
fi

find_tool() {
  local suffix="$1"
  local found
  found="$(find "$EXTRACT/bin" -maxdepth 1 \( -type f -o -type l \) -name "*-${suffix}" 2>/dev/null | sort | head -n1 || true)"
  [[ -n "$found" && -x "$found" ]] || {
    echo "::error::no *-${suffix} in ${EXTRACT}/bin" >&2
    ls -la "$EXTRACT/bin" >&2 || true
    exit 1
  }
  echo "$found"
}

REAL_GCC="$(find_tool gcc)"
REAL_GPP="$(find_tool g++)"
REAL_AR="$(find_tool ar)"

echo "cross-gcc ${REAL_GCC}"
"$REAL_GCC" --version | head -n1

# cc-rs may still pass clang-style --target= when the wrapper looks unusual.
# Real gcc does not accept that flag.
write_gnu_cc() {
  local name="$1"
  local real="$2"
  cat >"${HOME}/.local/bin/${name}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
args=()
skip_next=0
for arg in "\$@"; do
  if [[ "\$skip_next" -eq 1 ]]; then
    skip_next=0
    continue
  fi
  case "\$arg" in
    --target=*|-target=*|--triple=*|-triple=*)
      continue
      ;;
    --target|-target|--triple|-triple)
      skip_next=1
      continue
      ;;
  esac
  args+=("\$arg")
done
exec "${real}" "\${args[@]}"
EOF
  chmod +x "${HOME}/.local/bin/${name}"
}

write_gnu_cc "${CROSS_PREFIX}-gcc" "$REAL_GCC"
write_gnu_cc "${CROSS_PREFIX}-g++" "$REAL_GPP"
ln -sfn "$REAL_AR" "${HOME}/.local/bin/${CROSS_PREFIX}-ar"

export PATH="${EXTRACT}/bin:${HOME}/.local/bin:${PATH}"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${EXTRACT}/bin" >> "$GITHUB_PATH"
  echo "${HOME}/.local/bin" >> "$GITHUB_PATH"
fi
if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "PATH=${EXTRACT}/bin:${HOME}/.local/bin:${PATH}" >> "$GITHUB_ENV"
  echo "${CROSS_LINKER_ENV}=${HOME}/.local/bin/${CROSS_PREFIX}-gcc" >> "$GITHUB_ENV"
fi

probe_dir="$(mktemp -d)"
printf 'void foo(void) {}\n' > "$probe_dir/foo.c"
if ! "${HOME}/.local/bin/${CROSS_PREFIX}-gcc" --target=aarch64-unknown-linux-gnu \
  -c "$probe_dir/foo.c" -o "$probe_dir/foo.o"; then
  rm -rf "$probe_dir"
  echo "::error::cross gcc failed to compile a probe (after stripping --target=)" >&2
  exit 1
fi
rm -rf "$probe_dir"
echo "cross-cc ${CROSS_PREFIX}-gcc -> ${REAL_GCC} (GNU, not zig)"
echo "native gcc left alone (${HOST_TRIPLE})"
