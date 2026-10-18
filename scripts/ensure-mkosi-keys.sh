#!/usr/bin/env bash
# mkosi --tools-tree default builds a Fedora tools tree and verifies it with
# the keys in /usr/share/distribution-gpg-keys/fedora.
ensure_mkosi_fedora_keys() {
  local keys=/usr/share/distribution-gpg-keys/fedora
  if [[ -d "$keys" ]]; then
    return 0
  fi

  echo "installing distribution-gpg-keys (mkosi Fedora tools tree)"
  local installer=()
  if [[ "$(id -u)" -eq 0 ]]; then
    :
  elif command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
    installer=(sudo -n)
  else
    echo "Fedora GPG keys not found in /usr/share/distribution-gpg-keys" >&2
    echo "Install the distribution-gpg-keys package (passwordless sudo required)." >&2
    return 1
  fi

  if command -v dnf >/dev/null 2>&1; then
    "${installer[@]}" dnf install -y distribution-gpg-keys
  elif command -v yum >/dev/null 2>&1; then
    "${installer[@]}" yum install -y distribution-gpg-keys
  elif command -v apt-get >/dev/null 2>&1; then
    "${installer[@]}" apt-get update
    "${installer[@]}" apt-get install -y --no-install-recommends distribution-gpg-keys
  else
    echo "No dnf, yum, or apt-get available to install distribution-gpg-keys" >&2
    return 1
  fi

  if [[ ! -d "$keys" ]]; then
    echo "distribution-gpg-keys did not provide $keys" >&2
    return 1
  fi
}
