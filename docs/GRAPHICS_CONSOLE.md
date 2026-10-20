# Graphics (VGA) Console Guide

## Overview
Pertisk now supports **graphics consoles (VGA)** for a Proxmox-like experience. Boot ISO installers and watch the graphical output directly in your browser.

## What's Supported
- **VGA graphics forwarding** via UNIX socket
- **WebSocket relay** to browser
- **noVNC integration** for VNC client support
- **Serial console** still available as fallback
- **QEMU** on Linux (VGA or virtio-gpu, VNC, USB tablet). Cloud Hypervisor stays serial-only

## Creating a VM with Graphics Console

### Using CLI
```bash
pertisk vm create \
  --id 100 \
  --name "debian-installer" \
  --cpus 2 \
  --memory 2048 \
  --iso debian-12-amd64-netinst \
  --disk-size 30G \
  --graphics \
  --start
```

The `--graphics` flag enables VGA console instead of serial.

### Using REST API
```json
POST /v1/vms
{
  "id": 100,
  "spec": {
    "name": "debian-installer",
    "vcpus": 2,
    "memory_mib": 2048,
    "console_type": "graphics",
    "firmware": "/path/to/hypervisor-fw",
    "disks": [
      {
        "path": "/var/lib/pertisk/volumes/debian.qcow2",
        "cdrom": false
      },
      {
        "path": "/var/lib/pertisk/iso/debian-12-amd64-netinst.iso",
        "cdrom": true
      }
    ]
  }
}
```

## Connecting to Graphics Console

### Web Dashboard
1. Navigate to **Guests** → Select VM → **Console** tab
2. If VGA is enabled, the console displays graphics (requires noVNC support)
3. Keyboard and mouse input are forwarded to the VM

### VNC Client (Native)
```bash
# Graphics console is available at:
ws://localhost:7480/v1/vms/<vm-id>/graphics/ws?token=<jwt-token>

# Use any VNC client that supports WebSocket:
vncviewer localhost:7480
```

### Troubleshooting
- **"No graphics console"** error: Ensure VM was created with `--graphics` flag
- **Canvas blank**: VM may not have booted yet; check serial console for messages
- **Keyboard not responding**: Click the canvas to focus it
- **Connection refused**: Ensure daemon is running and accessible

## Default: Serial Console
If `--graphics` is not specified, VMs use **serial console** (ttyS0):
```bash
pertisk vm create --id 101 --name "server" --iso alpine  # Serial by default
```

Serial console is suitable for headless servers and provides reliable logging.

## Technical Details

### Architecture
```
VM (QEMU)
  ├─ serial console → /run/pertisk/101.serial.sock → HTTP → WebSocket
  └─ graphics (VGA) → /run/pertisk/101.graphics.sock → HTTP → WebSocket
                                                              ↓
                                                         Browser (noVNC)
```

### API Endpoints
- `GET /v1/vms/<id>/console` - Console metadata (type, paths, WebSocket URL)
- `WS /v1/vms/<id>/console/ws` - Serial console WebSocket
- `WS /v1/vms/<id>/graphics/ws` - Graphics console WebSocket (VNC binary protocol)

### Storage
Graphics socket paths: `/run/pertisk/<vm-id>.graphics.sock`  
Automatically cleaned up on VM destroy.

## Limitations
- Graphics console is the QEMU driver on Linux. Cloud Hypervisor has no VGA
- Mock driver (macOS) does not open a VNC socket
- No GPU acceleration or PCIe passthrough yet
- Serial and Display are separate tabs on the same guest

## Future Enhancements
- [ ] USB device passthrough for better installer support
- [x] Multi-console (Serial and Display tabs on the same guest)
- [x] Remote console recording (Display saves a WebM; Serial downloads the log)
- [x] Clipboard paste types into the Display tab (works before a guest agent is installed)
