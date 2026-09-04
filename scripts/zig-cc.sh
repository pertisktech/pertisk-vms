#!/usr/bin/env bash
# zig cc frontend that drops cc-rs clang flags which conflict with -target.
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
    --target=*|-target=*)
      continue
      ;;
    --target|-target)
      skip_next=1
      continue
      ;;
    -m64|-m32)
      continue
      ;;
  esac
  args+=("$arg")
done

exec "$ZIG" cc -target "$ZIG_CC_TARGET" "${args[@]}"
