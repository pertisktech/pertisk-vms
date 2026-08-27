# sourced by linux-guest.sh / linux-iso-guest.sh
# expects: CACHE, die

ensure_cloud_hypervisor() {
  if command -v cloud-hypervisor >/dev/null 2>&1; then
    return 0
  fi
  mkdir -p "$CACHE"
  local bin="$CACHE/cloud-hypervisor"
  local asset="cloud-hypervisor-static"
  case "$(uname -m)" in
    aarch64|arm64) asset="cloud-hypervisor-static-aarch64" ;;
  esac
  if [[ ! -x "$bin" ]]; then
    echo "fetching cloud-hypervisor ($asset)"
    curl -fsSL -o "$bin" \
      "https://github.com/cloud-hypervisor/cloud-hypervisor/releases/latest/download/${asset}"
    chmod +x "$bin"
  fi
  export PATH="$CACHE:$PATH"
  command -v cloud-hypervisor >/dev/null || die "failed to install cloud-hypervisor into $CACHE"
}

explain_cli() {
  echo "CLI is $pertisk (not on PATH). After this script:"
  echo "  export PATH=\"$(dirname "$pertisk"):\$PATH\""
  echo "  pertisk --url $URL vm list"
}

show_capacity() {
  echo "--- cluster ---"
  "$pertisk" --url "$URL" cluster status || true
  echo "--- guests ---"
  "$pertisk" --url "$URL" vm list || true
}
