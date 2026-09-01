cd /root/pertisk-vms

# UI (if you changed web/ui)
cd web/ui && npm ci && npm run build && cd ../..

# Binaries
cargo build --release -p pertisk-daemon -p pertisk-cli -p pertisk-tui

# Copy into appliance VM disk
qm stop 901
mount -o offset=$((1050624*512)) /dev/zvol/rpool/data/vm-901-disk-0 /mnt/pertisk901
install -m 755 target/release/pertiskd /mnt/pertisk901/usr/bin/pertiskd
install -m 755 target/release/pertisk     /mnt/pertisk901/usr/bin/pertisk
install -m 755 target/release/pertisk-tui   /mnt/pertisk901/usr/bin/pertisk-tui
umount /mnt/pertisk901
qm start 901