#!/usr/bin/env bash
# Remove mkosi leftovers that actions/checkout cannot delete on a self-hosted runner.
# sudo mkosi --tools-tree writes root-owned files under out/tools.
set -euo pipefail

ws="${GITHUB_WORKSPACE:-${1:-}}"
[[ -n "$ws" ]] || { echo "ci-reset-workspace: set GITHUB_WORKSPACE or pass a path" >&2; exit 1; }

run_priv() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  elif command -v sudo >/dev/null && sudo -n true 2>/dev/null; then
    sudo -n "$@"
  else
    "$@"
  fi
}

unmount_under() {
  local root="$1"
  [[ -d "$root" ]] || return 0
  command -v findmnt >/dev/null || return 0
  local m
  while IFS= read -r m; do
    [[ -n "$m" ]] || continue
    echo "Unmounting $m"
    run_priv umount -l "$m" 2>/dev/null || run_priv umount "$m" 2>/dev/null || true
  done < <(findmnt -rn -o TARGET 2>/dev/null | grep "^${root}/" | sort -r || true)
}

remove_tree() {
  local path="$1"
  [[ -e "$path" ]] || return 0
  unmount_under "$path"
  echo "Removing $path"
  if ! run_priv rm -rf "$path"; then
    echo "::error::Failed to remove ${path} (root-owned leftover from mkosi?)" >&2
    ls -la "$(dirname "$path")" 2>/dev/null || true
    return 1
  fi
}

echo "Clearing mkosi output under $ws"
remove_tree "$ws/out/tools"
remove_tree "$ws/out"
remove_tree "$ws/release"
remove_tree "$ws/dist"
if [[ -d "$ws" ]]; then
  run_priv chown "$(id -u):$(id -g)" "$ws" || true
fi
