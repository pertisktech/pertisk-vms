# sourced by linux-guest.sh / linux-iso-guest.sh / build-iso.sh
# expects: CACHE, die

host_machine() {
  uname -m
}

normalize_uname_arch() {
  case "$1" in
    amd64|x86_64|x86-64) echo x86_64 ;;
    arm64|aarch64) echo aarch64 ;;
    *) echo "$1" ;;
  esac
}

ensure_cloud_hypervisor() {
  local arch
  arch="$(normalize_uname_arch "${1:-$(host_machine)}")"
  mkdir -p "$CACHE"
  local asset dest
  case "$arch" in
    aarch64)
      asset="cloud-hypervisor-static-aarch64"
      dest="$CACHE/cloud-hypervisor-aarch64"
      ;;
    *)
      asset="cloud-hypervisor-static"
      dest="$CACHE/cloud-hypervisor-x86_64"
      ;;
  esac

  if [[ -z "${1:-}" ]] && command -v cloud-hypervisor >/dev/null 2>&1; then
    CLOUD_HYPERVISOR="$(command -v cloud-hypervisor)"
    return 0
  fi

  if [[ ! -x "$dest" ]]; then
    echo "fetching cloud-hypervisor ($asset)"
    curl -fsSL -o "$dest" \
      "https://github.com/cloud-hypervisor/cloud-hypervisor/releases/latest/download/${asset}"
    chmod +x "$dest"
  fi
  CLOUD_HYPERVISOR="$dest"
  if [[ -z "${1:-}" ]]; then
    ln -sfn "$dest" "$CACHE/cloud-hypervisor"
    export PATH="$CACHE:$PATH"
  fi
  [[ -x "$CLOUD_HYPERVISOR" ]] || die "failed to install cloud-hypervisor into $CACHE"
}

ensure_firmware() {
  local arch
  arch="$(normalize_uname_arch "${1:-$(host_machine)}")"
  mkdir -p "$CACHE"
  local asset dest
  case "$arch" in
    aarch64)
      asset="hypervisor-fw-aarch64"
      dest="$CACHE/hypervisor-fw-aarch64"
      ;;
    *)
      asset="hypervisor-fw"
      dest="$CACHE/hypervisor-fw-x86_64"
      ;;
  esac

  if [[ ! -s "$dest" ]]; then
    echo "fetching rust-hypervisor-firmware 0.5.0 ($asset)"
    curl -fsSL -o "$dest" \
      "https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/0.5.0/${asset}"
    chmod +x "$dest" || true
  fi
  [[ -s "$dest" ]] || die "failed to install hypervisor-fw into $CACHE"
  FIRMWARE="$dest"
}

explain_cli() {
  echo "CLI is $pertisk (not on PATH). After this script:"
  echo "  export PATH=\"$(dirname "$pertisk"):\$PATH\""
  echo "  pertisk --url $URL vm list"
  echo "UI/API bound on ${LISTEN:-0.0.0.0:7480} (local $URL ; LAN https://<this-host>:7443/)"
}

show_capacity() {
  echo "--- cluster ---"
  "$pertisk" --url "$URL" cluster status || true
  echo "--- guests ---"
  "$pertisk" --url "$URL" vm list || true
}
