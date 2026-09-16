# AlmaLinux / RHEL packaging

Customer x86_64 UEFI install path:

1. `scripts/build-rpm.sh` — builds `pertisk-vms` RPM (binaries + Cloud Hypervisor + units).
2. `scripts/build-alma-iso.sh` — wraps AlmaLinux 10 **boot.iso** + Kickstart + RPM via `mkksiso`.

The default base is the ~1 GiB network `boot.iso` (Anaconda pulls BaseOS/AppStream from
`repo.almalinux.org`). That avoids needing ~12 GiB free to remaster the full DVD.
Install-time DHCP/network is required (`ip=dhcp` is baked into the ISO cmdline).

If the console looks stuck on `anaconda-nm-disable-autocons`, rebuild with the current
script (console order + `ip=dhcp`), or attach serial / `tmux attach` on tty2.

```bash
# On AlmaLinux/RHEL 10 x86_64 build host:
sudo dnf install -y lorax rpm-build createrepo_c
make release-alma-iso VERSION=0.1.0

# Optional: offline DVD base (needs ~12 GiB free for source + output):
# PERTISK_ALMA_ISO_URL=https://repo.almalinux.org/almalinux/10/isos/x86_64/AlmaLinux-10.2-x86_64-dvd.iso \
#   make release-alma-iso VERSION=0.1.0
```

You can delete a previously cached DVD under `~/.pertisk/images/` to reclaim space.

Artifacts land in `release/`:

- `pertisk-vms-<ver>-1.*.x86_64.rpm`
- `pertisk-node-<ver>-x86_64.iso`

Kickstart: [`kickstart/pertisk-node.ks`](kickstart/pertisk-node.ks).  
Alma-specific overlay scraps: [`rpm/alma-overlay/`](rpm/alma-overlay/).

Debian mkosi raw images (`make release-amd`) remain for the live-USB / `pertisk-install` flow and for ARM/SBC boards.
