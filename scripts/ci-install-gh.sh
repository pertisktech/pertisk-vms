#!/usr/bin/env bash
# Ensure GitHub CLI is available (self-hosted runners may not have gh on PATH).
ensure_gh() {
  local dir="${HOME}/.local/bin"
  if [ -x "${dir}/gh" ]; then
    export PATH="${dir}:${PATH}"
    return 0
  fi
  if command -v gh >/dev/null 2>&1; then
    return 0
  fi

  local version="${GH_VERSION:-2.86.0}"
  mkdir -p "$dir"
  local arch
  case "$(uname -m)" in
    x86_64|amd64) arch=amd64 ;;
    aarch64|arm64) arch=arm64 ;;
    *)
      echo "::error::unsupported architecture for gh: $(uname -m)" >&2
      return 1
      ;;
  esac

  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  curl -fsSL "https://github.com/cli/cli/releases/download/v${version}/gh_${version}_linux_${arch}.tar.gz" \
    -o "${tmp}/gh.tgz"
  tar -xzf "${tmp}/gh.tgz" -C "$tmp"
  install -m 755 "${tmp}/gh_${version}_linux_${arch}/bin/gh" "${dir}/gh"
  export PATH="${dir}:${PATH}"
  "${dir}/gh" --version
}
