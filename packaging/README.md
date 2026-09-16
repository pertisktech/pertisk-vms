# AlmaLinux / RHEL packaging

Customer x86_64 UEFI install path:

1. `scripts/build-rpm.sh` — builds `pertisk-vms` RPM (binaries + Cloud Hypervisor + units).
2. `scripts/build-alma-iso.sh` — wraps AlmaLinux 10 DVD + Kickstart + RPM via `mkksiso`.

```bash
# On AlmaLinux/RHEL 10 x86_64 build host:
sudo dnf install -y lorax rpm-build
make release-alma-iso VERSION=0.1.0
```

Artifacts land in `release/`:

- `pertisk-vms-<ver>-1.*.x86_64.rpm`
- `pertisk-node-<ver>-x86_64.iso`

Kickstart: [`kickstart/pertisk-node.ks`](kickstart/pertisk-node.ks).  
Alma-specific overlay scraps: [`rpm/alma-overlay/`](rpm/alma-overlay/).

Debian mkosi raw images (`make release-amd`) remain for the live-USB / `pertisk-install` flow and for ARM/SBC boards.
