#!/usr/bin/env bash
# zig cc frontend that drops cc-rs clang flags which conflict with zig -target.
# cc-rs sees `zig cc` as clang and passes --target=aarch64-unknown-linux-gnu
# (Rust/LLVM triple). Zig's target grammar is arch-os-abi, so it parses
# "unknown" as the OS and exits: UnknownOperatingSystem.
# Usage: ZIG=/path/to/zig ZIG_CC_TARGET=aarch64-linux-gnu.2.28 zig-cc.sh [args...]
set -euo pipefail

: "${ZIG:?ZIG is required}"
: "${ZIG_CC_TARGET:?ZIG_CC_TARGET is required}"

args=()
skip_next=0
for arg in "$@"; do
  if [[ "$skip_next" -eq 1 ]]; then
    skip_next=0
    continue
  fi
  case "$arg" in
    --target=*|-target=*|--triple=*|-triple=*)
      continue
      ;;
    --target|-target|--triple|-triple)
      skip_next=1
      continue
      ;;
    -m64|-m32)
      continue
      ;;
    *-unknown-linux-*|*-unknown-unknown-*)
      # Bare Rust triple, not a filesystem path.
      if [[ "$arg" != */* ]]; then
        continue
      fi
      args+=("$arg")
      ;;
  esac
  args+=("$arg")
done

exec "$ZIG" cc -target "$ZIG_CC_TARGET" "${args[@]}"
