#!/usr/bin/env python3
"""Pertisk node installer API. Serves the web UI and writes an Anaconda kickstart."""
from __future__ import annotations

import json
import os
import re
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

PORT = int(os.environ.get("PERTISK_INSTALLER_PORT", "8090"))
MIN_DISK = 24 * 1024 ** 3
HERE = Path(__file__).resolve().parent
DIST = HERE / "dist"
STATE_LOCK = threading.Lock()
STATE = {
    "status": "idle",
    "proc": None,
    "error": "",
}


def hash_password(password: str) -> str:
    import crypt

    return crypt.crypt(password, crypt.mksalt(crypt.METHOD_SHA512))


def ipv4_ok(value: str) -> bool:
    parts = value.split(".")
    if len(parts) != 4:
        return False
    try:
        return all(0 <= int(part) <= 255 and str(int(part)) == part for part in parts)
    except ValueError:
        return False


def netmask(prefix: int) -> str:
    mask = (0xFFFFFFFF << (32 - prefix)) & 0xFFFFFFFF
    return ".".join(str((mask >> shift) & 0xFF) for shift in (24, 16, 8, 0))


def read_version() -> str:
    for path in (HERE / "VERSION", Path("/usr/share/pertisk-installer/VERSION")):
        if path.is_file():
            text = path.read_text(encoding="utf-8").strip()
            if text:
                return text
    return os.environ.get("PERTISK_VERSION", "0.1.0")


def template_path() -> Path:
    env = os.environ.get("PERTISK_KS_TEMPLATE")
    if env:
        return Path(env)
    for candidate in (HERE / "pertisk-node.ks", HERE.parent / "kickstart" / "pertisk-node.ks"):
        if candidate.is_file():
            return candidate
    raise FileNotFoundError("kickstart template not found")


def _section_end(lines: list[str], start: int) -> int:
    for index in range(start + 1, len(lines)):
        if lines[index].strip() == "%end":
            return index
    raise ValueError("kickstart section missing %end")


def extract_payload(template: str) -> str:
    """Keep package, post, and source commands. Drop the interactive header."""
    lines = template.splitlines()
    kept: list[str] = []
    prefixes = (
        "url ",
        "repo ",
        "firewall ",
        "selinux ",
        "firstboot ",
        "services ",
        "bootloader ",
    )
    index = 0
    while index < len(lines):
        stripped = lines[index].strip()
        if stripped.startswith("%pre"):
            index = _section_end(lines, index) + 1
            continue
        if stripped.startswith(("%addon", "%packages", "%post")):
            end = _section_end(lines, index)
            kept.extend(lines[index : end + 1])
            kept.append("")
            index = end + 1
            continue
        if stripped.startswith(prefixes):
            kept.append(lines[index])
        index += 1
    return "\n".join(kept).strip() + "\n"


def _token(value: str, pattern: str, label: str) -> str:
    text = value.strip()
    if not re.fullmatch(pattern, text):
        raise ValueError(f"Invalid {label}.")
    return text


def _password(value: str, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label} is required.")
    if len(value) < 8:
        raise ValueError(f"{label} must be at least 8 characters.")
    if ":" in value or "\n" in value or "\r" in value or "\x00" in value:
        raise ValueError(f"{label} cannot contain a colon or a new line.")
    return value


def build_kickstart(template: str, answers: dict, disk: dict, firmware: str) -> str:
    if disk.get("kind") != "disk":
        raise ValueError("Select an internal disk.")
    if int(disk.get("size") or 0) < MIN_DISK:
        raise ValueError("Choose a disk of at least 24 GiB.")
    disk_id = _token(str(disk.get("id") or ""), r"[A-Za-z0-9]+", "disk")
    nic = _token(str(answers.get("nicId") or ""), r"[A-Za-z0-9._-]+", "network interface")
    hostname = _token(
        str(answers.get("hostname") or ""),
        r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?",
        "hostname",
    )
    lang = _token(str(answers.get("lang") or "en_US.UTF-8"), r"[A-Za-z0-9_.@-]+", "language")
    keyboard = _token(str(answers.get("keyboard") or "us"), r"[A-Za-z0-9_-]+", "keyboard")
    timezone = _token(str(answers.get("timezone") or "Etc/UTC"), r"[A-Za-z0-9_+/-]+", "time zone")
    admin = _password(str(answers.get("adminPassword") or ""), "Password")
    if answers.get("adminConfirm") not in (None, admin):
        raise ValueError("Passwords do not match.")
    separate = bool(answers.get("separateRoot"))
    if separate:
        root = _password(str(answers.get("rootPassword") or ""), "Root password")
        if answers.get("rootConfirm") not in (None, root):
            raise ValueError("Root passwords do not match.")
    else:
        root = admin
    mode = answers.get("mode") or "dhcp"
    if mode not in ("dhcp", "static"):
        raise ValueError("Unknown network mode.")
    network = f"network --device={nic} --bootproto=dhcp --activate --onboot=on --hostname={hostname}"
    if mode == "static":
        address = str(answers.get("address") or "").strip()
        gateway = str(answers.get("gateway") or "").strip()
        dns = str(answers.get("dns") or "").strip()
        if not ipv4_ok(address):
            raise ValueError("Enter an IPv4 address.")
        if not ipv4_ok(gateway):
            raise ValueError("Enter a gateway.")
        if not ipv4_ok(dns):
            raise ValueError("Enter a DNS server.")
        try:
            prefix = int(str(answers.get("prefix")).strip())
        except (TypeError, ValueError):
            prefix = 0
        if prefix < 1 or prefix > 32:
            raise ValueError("Prefix must be 1–32.")
        network = (
            f"network --device={nic} --bootproto=static --ip={address} "
            f"--netmask={netmask(prefix)} --gateway={gateway} --nameserver={dns} "
            f"--activate --onboot=on --hostname={hostname}"
        )
    peer = str(answers.get("peer") or "").strip()
    if peer and not re.fullmatch(r"https?://\S+", peer):
        raise ValueError("Use an http:// or https:// URL.")
    admin_hash = hash_password(admin)
    root_hash = hash_password(root)
    parts = [
        "cmdline",
        f"lang {lang}",
        f"keyboard --vckeymap={keyboard}",
        f"timezone {timezone} --utc",
        "eula --agreed",
        network,
        f"rootpw --iscrypted '{root_hash}' --allow-ssh",
        f"user --name=admin --groups=wheel --homedir=/home/admin --iscrypted --password='{admin_hash}'",
        f"ignoredisk --only-use={disk_id}",
        "zerombr",
        f"clearpart --all --initlabel --disklabel=gpt --drives={disk_id}",
    ]
    if firmware != "uefi":
        parts.append("part biosboot --fstype=biosboot --size=1")
    parts.extend(
        [
            "part /boot/efi --fstype=efi --size=512",
            "part /boot --fstype=xfs --size=1024",
            "part swap --fstype=swap --size=2048",
            "part / --fstype=xfs --size=10240 --maxsize=71680",
            "part /var/lib/pertisk --fstype=xfs --size=10240 --grow",
            "",
            extract_payload(template).rstrip(),
            "",
            RPM_PRE.rstrip(),
            "",
            NOCHROOT.rstrip(),
            "",
        ]
    )
    text = "\n".join(parts) + "\n"
    if re.search(r"(?m)^reboot$", text):
        raise ValueError("generated kickstart must wait for the installer reboot button")
    return text


RPM_PRE = """%pre --erroronfail --log=/tmp/pertisk-kickstart-pre.log
#!/bin/bash
set -euo pipefail
mkdir -p /run/pertisk-repo
RPM="$(find /run/install /run/media /media -name 'pertisk-vms-*.rpm' 2>/dev/null | head -n1 || true)"
if [[ -z "${RPM}" ]]; then
  echo "pertisk kickstart: ERROR - pertisk-vms RPM not on boot media" >&2
  exit 1
fi
echo "pertisk kickstart: found ${RPM}"
cp -a "$(dirname "${RPM}")/." /run/pertisk-repo/
%end
"""

NOCHROOT = """%post --nochroot --erroronfail --log=/tmp/pertisk-pass.log
#!/bin/bash
set -euo pipefail
mkdir -p /mnt/sysimage/etc/pertisk
touch /mnt/sysimage/etc/pertisk/daemon.env
chmod 600 /mnt/sysimage/etc/pertisk/daemon.env
if [[ -f /run/pertisk/admin.pass ]]; then
  install -m 600 /run/pertisk/admin.pass /mnt/sysimage/etc/pertisk/admin
  if ! grep -q '^PERTISK_ADMIN_PASSWORD=' /mnt/sysimage/etc/pertisk/daemon.env; then
    printf 'PERTISK_ADMIN_PASSWORD=%s\\n' "$(head -n1 /run/pertisk/admin.pass)" >> /mnt/sysimage/etc/pertisk/daemon.env
  fi
fi
if [[ -f /run/pertisk/join.url ]]; then
  if ! grep -q '^PERTISK_JOIN=' /mnt/sysimage/etc/pertisk/daemon.env; then
    printf 'PERTISK_JOIN=%s\\n' "$(head -n1 /run/pertisk/join.url)" >> /mnt/sysimage/etc/pertisk/daemon.env
  fi
fi
%end
"""


def _read(path: str) -> str:
    try:
        return Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def _gib(num_bytes: int) -> str:
    gib = num_bytes / 1024 ** 3
    if gib >= 10:
        return f"{round(gib)} GiB"
    return f"{gib:.1f} GiB"


def parse_lsblk(data: dict) -> list[dict]:
    disks = []
    for dev in data.get("blockdevices") or []:
        if dev.get("type") != "disk":
            continue
        name = str(dev.get("name") or "")
        if not re.fullmatch(r"[A-Za-z0-9]+", name):
            continue
        try:
            size = int(dev.get("size") or 0)
        except (TypeError, ValueError):
            size = 0
        rm = dev.get("rm") in (True, 1, "1", "true")
        tran = str(dev.get("tran") or "").lower()
        model = str(dev.get("model") or "").strip() or name
        kind = "installer" if rm or tran in {"usb", "mmc"} else "disk"
        bus = {"nvme": "NVMe", "sata": "SATA", "sas": "SAS", "virtio": "VirtIO", "usb": "USB", "ata": "SATA"}.get(
            tran, tran.upper() or "Disk"
        )
        disks.append({"id": name, "model": model, "size": size, "bus": bus, "kind": kind})
    return disks


def probe() -> tuple[list[dict], list[dict], dict, str]:
    raw = subprocess.check_output(
        ["lsblk", "-J", "-b", "-o", "NAME,SIZE,TYPE,RM,TRAN,MODEL"],
        text=True,
    )
    disks = parse_lsblk(json.loads(raw))
    nics = _probe_nics()
    host, firmware = _probe_host()
    return disks, nics, host, firmware


def _probe_nics() -> list[dict]:
    nics = []
    root = Path("/sys/class/net")
    if not root.is_dir():
        return nics
    for name in sorted(item.name for item in root.iterdir()):
        if name == "lo" or not re.fullmatch(r"[A-Za-z0-9._-]+", name):
            continue
        base = root / name
        mac = _read(str(base / "address")).strip()
        link = _read(str(base / "operstate")).strip() == "up"
        speed_text = _read(str(base / "speed")).strip()
        try:
            speed_n = int(speed_text)
        except ValueError:
            speed_n = -1
        driver = ""
        driver_link = base / "device" / "driver"
        if driver_link.exists():
            driver = os.path.basename(os.path.realpath(driver_link))
        nics.append(
            {
                "id": name,
                "model": driver or "Ethernet",
                "mac": mac,
                "link": link,
                "speed": f"{speed_n} Mb/s" if speed_n > 0 else None,
                "dhcp": _ipv4(name) if link else None,
            }
        )
    return nics


def _ipv4(nic: str) -> str | None:
    try:
        raw = subprocess.check_output(["ip", "-j", "-4", "addr", "show", "dev", nic], text=True)
        data = json.loads(raw)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError):
        return None
    for item in data:
        for addr in item.get("addr_info") or []:
            local = addr.get("local")
            if local and ipv4_ok(local) and not local.startswith("127."):
                return local
    return None


def _probe_host() -> tuple[dict, str]:
    cpu = "Unknown processor"
    threads = 0
    flags = ""
    try:
        for line in Path("/proc/cpuinfo").read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
            elif line.startswith("processor"):
                threads += 1
            elif line.startswith("flags"):
                flags = line
    except OSError:
        pass
    mem_bytes = 0
    try:
        for line in Path("/proc/meminfo").read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("MemTotal:"):
                mem_bytes = int(line.split()[1]) * 1024
                break
    except (OSError, ValueError, IndexError):
        pass
    efi = Path("/sys/firmware/efi").is_dir()
    if re.search(r"\bsvm\b", flags):
        virt = "SVM"
    elif re.search(r"\bvmx\b", flags):
        virt = "VT-x"
    else:
        virt = "Unavailable"
    host = {
        "cpu": cpu,
        "threads": threads or 1,
        "memory": _gib(mem_bytes) if mem_bytes else "Unknown",
        "firmware": "UEFI" if efi else "BIOS",
        "secureBoot": _secure_boot() if efi else False,
        "virt": virt,
    }
    return host, "uefi" if efi else "bios"


def _secure_boot() -> bool:
    try:
        for path in Path("/sys/firmware/efi/efivars").glob("SecureBoot-*"):
            data = path.read_bytes()
            return bool(data) and data[-1] == 1
    except OSError:
        return False
    return False


def _find_disk(disks: list[dict], disk_id: str) -> dict:
    for disk in disks:
        if disk["id"] == disk_id:
            return disk
    raise ValueError("Select an internal disk.")


def _find_nic(nics: list[dict], nic_id: str) -> None:
    if not any(nic["id"] == nic_id for nic in nics):
        raise ValueError("Select a network interface.")


def begin_install(answers: dict) -> None:
    with STATE_LOCK:
        proc = STATE["proc"]
        if proc is not None and proc.poll() is None:
            raise ValueError("Installation is already running.")
        disks, nics, _host, firmware = probe()
        disk = _find_disk(disks, str(answers.get("diskId") or ""))
        _find_nic(nics, str(answers.get("nicId") or ""))
        if not answers.get("eraseConfirmed"):
            raise ValueError("Confirm this disk can be erased.")
        ks = build_kickstart(template_path().read_text(encoding="utf-8"), answers, disk, firmware)
        run = Path("/run/pertisk")
        run.mkdir(parents=True, exist_ok=True)
        os.chmod(run, 0o700)
        (run / "ui.ks").write_text(ks, encoding="utf-8")
        os.chmod(run / "ui.ks", 0o600)
        admin = str(answers.get("adminPassword") or "")
        (run / "admin.pass").write_text(admin + "\n", encoding="utf-8")
        os.chmod(run / "admin.pass", 0o600)
        peer = str(answers.get("peer") or "").strip()
        join = run / "join.url"
        if peer:
            join.write_text(peer + "\n", encoding="utf-8")
            os.chmod(join, 0o600)
        elif join.exists():
            join.unlink()
        log = open("/tmp/pertisk-anaconda.log", "ab", buffering=0)
        # Anaconda reboots when the kickstart omits `reboot` (action None).
        # A PATH shim turns that reboot/halt into a no-op so this UI can show
        # the finished page. The reboot button calls /usr/sbin/reboot directly.
        bindir = run / "bin"
        bindir.mkdir(parents=True, exist_ok=True)
        shim = bindir / "systemctl"
        shim.write_text(SYSTEMCTL_SHIM, encoding="utf-8")
        os.chmod(shim, 0o755)
        env = os.environ.copy()
        env["PATH"] = str(bindir) + os.pathsep + env.get("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        child = subprocess.Popen(
            ["/usr/sbin/anaconda", "--kickstart", str(run / "ui.ks"), "--cmdline"],
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
            env=env,
        )
        STATE["proc"] = child
        STATE["status"] = "running"
        STATE["error"] = ""
        STATE["stage"] = 0


SYSTEMCTL_SHIM = """#!/bin/bash
# Anaconda's exit path runs `systemctl reboot` even when the kickstart does not
# ask for it. Ignore power actions so the Pertisk installer can stay up.
real=/usr/bin/systemctl
[[ -x "$real" ]] || real=/usr/sbin/systemctl
for arg in "$@"; do
  case "$arg" in
    reboot|poweroff|halt|kexec)
      echo "pertisk-installer: ignoring systemctl $arg" >&2
      exit 0
      ;;
  esac
done
exec "$real" "$@"
"""


def tail_file(path: str, limit: int = 262144) -> str:
    """Last bytes of a log. Full logs are large enough to get the installer killed."""
    try:
        with open(path, "rb") as handle:
            handle.seek(0, os.SEEK_END)
            size = handle.tell()
            handle.seek(max(0, size - limit))
            data = handle.read(limit)
    except OSError:
        return ""
    return data.decode("utf-8", errors="replace")


def detect_stage(pack: str, program: str, storage: str, ana: str) -> int:
    """Map installer logs to a progress stage.

    Do not treat the kickstart text `%post` as configuration. Anaconda logs the
    file while it is still installing packages. Configuration starts when it
    logs "Running post-installation scripts".
    """
    blob = (program + "\n" + ana).lower()
    pack_l = pack.lower()
    storage_l = storage.lower()
    if "post-installation" in blob:
        return 3
    if "pertisk-vms" in pack_l:
        return 2
    if any(word in pack for word in ("Downloading", "Installing", "RPM")):
        return 1
    if any(word in storage_l for word in ("partition", "disklabel", "format")):
        return 0
    return 0


def progress() -> dict:
    with STATE_LOCK:
        proc = STATE["proc"]
        status = STATE["status"]
        error = STATE["error"]
        stage = int(STATE.get("stage") or 0)
    if status == "idle":
        return {"status": "idle", "pct": 0, "stage": 0, "lines": [], "error": ""}
    pack = tail_file("/tmp/packaging.log")
    program = tail_file("/tmp/program.log")
    storage = tail_file("/tmp/storage.log")
    ana = tail_file("/tmp/anaconda.log") + "\n" + tail_file("/tmp/pertisk-anaconda.log")
    stage = max(stage, detect_stage(pack, program, storage, ana))
    lines = _tail(pack, program, ana)
    pct = (8, 36, 68, 88, 100)[stage]
    code = proc.poll() if proc is not None else None
    if code is None:
        status = "running"
    elif code == 0:
        status = "done"
        stage = 4
        pct = 100
        lines = lines + ["Installation finished."]
    else:
        status = "error"
        error = error or f"Anaconda exited with status {code}."
    with STATE_LOCK:
        STATE["status"] = status
        STATE["error"] = error
        STATE["stage"] = stage
    return {"status": status, "pct": pct, "stage": stage, "lines": lines[-12:], "error": error}


def _tail(*blobs: str, limit: int = 12) -> list[str]:
    lines: list[str] = []
    for blob in blobs:
        lines.extend(line.strip() for line in blob.splitlines() if line.strip())
    return lines[-limit:]


def _power(action: str) -> None:
    binary = "/usr/sbin/reboot" if action == "reboot" else "/usr/sbin/poweroff"
    if not os.path.exists(binary):
        binary = action
    subprocess.Popen([binary], start_new_session=True)


class Handler(BaseHTTPRequestHandler):
    server_version = "pertisk-installer"

    def log_message(self, fmt: str, *args) -> None:
        print(f"pertisk-installer: {self.address_string()} {fmt % args}", flush=True)

    def do_GET(self) -> None:
        path = unquote(urlparse(self.path).path)
        if path == "/api/session":
            self._session()
            return
        if path == "/api/progress":
            try:
                payload = progress()
            except Exception as exc:
                stage = int(STATE.get("stage") or 0)
                payload = {
                    "status": "running",
                    "pct": (8, 36, 68, 88, 100)[min(stage, 4)],
                    "stage": stage,
                    "lines": [f"progress: {exc}"],
                    "error": "",
                }
            self._json(200, payload)
            return
        if path.startswith("/api/"):
            self._json(404, {"error": "Not found."})
            return
        self._static(path)

    def do_POST(self) -> None:
        path = unquote(urlparse(self.path).path)
        if path == "/api/install":
            self._install()
            return
        if path == "/api/reboot":
            _power("reboot")
            self._json(202, {"ok": True})
            return
        if path == "/api/poweroff":
            _power("poweroff")
            self._json(202, {"ok": True})
            return
        self._json(404, {"error": "Not found."})

    def _session(self) -> None:
        try:
            disks, nics, host, _firmware = probe()
        except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
            self._json(500, {"error": f"Could not read this machine ({exc})."})
            return
        self._json(
            200,
            {"live": True, "version": read_version(), "host": host, "disks": disks, "nics": nics},
        )

    def _install(self) -> None:
        try:
            length = int(self.headers.get("Content-Length") or "0")
        except ValueError:
            length = 0
        if length <= 0 or length > 256 * 1024:
            self._json(400, {"error": "Invalid install request."})
            return
        try:
            answers = json.loads(self.rfile.read(length).decode("utf-8"))
        except (UnicodeError, json.JSONDecodeError):
            self._json(400, {"error": "Invalid install request."})
            return
        if not isinstance(answers, dict):
            self._json(400, {"error": "Invalid install request."})
            return
        try:
            begin_install(answers)
        except ValueError as exc:
            self._json(400, {"error": str(exc)})
            return
        except FileNotFoundError as exc:
            self._json(500, {"error": str(exc)})
            return
        except OSError as exc:
            self._json(500, {"error": f"Could not start installation ({exc})."})
            return
        self._json(202, {"ok": True})

    def _json(self, code: int, payload: dict) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _static(self, path: str) -> None:
        rel = path.lstrip("/") or "index.html"
        candidate = (DIST / rel).resolve()
        try:
            candidate.relative_to(DIST.resolve())
        except ValueError:
            self.send_error(404)
            return
        if candidate.is_dir():
            candidate = candidate / "index.html"
        if not candidate.is_file():
            candidate = DIST / "index.html"
            if not candidate.is_file():
                self.send_error(404)
                return
        data = candidate.read_bytes()
        types = {
            ".html": "text/html; charset=utf-8",
            ".js": "text/javascript; charset=utf-8",
            ".css": "text/css; charset=utf-8",
            ".svg": "image/svg+xml",
            ".json": "application/json",
            ".png": "image/png",
            ".woff2": "font/woff2",
            ".ico": "image/x-icon",
        }
        self.send_response(200)
        self.send_header("Content-Type", types.get(candidate.suffix, "application/octet-stream"))
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(data)


def serve() -> None:
    DIST.mkdir(parents=True, exist_ok=True)
    httpd = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    print(f"pertisk installer listening on http://0.0.0.0:{PORT}/", flush=True)
    httpd.serve_forever()


if __name__ == "__main__":
    serve()
