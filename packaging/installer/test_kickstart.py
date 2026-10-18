#!/usr/bin/env python3
"""Checks for the kickstart written by the Pertisk installer."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import server

TEMPLATE = (Path(__file__).resolve().parent.parent / "kickstart" / "pertisk-node.ks").read_text(
    encoding="utf-8"
)
DISK = {"id": "nvme0n1", "kind": "disk", "size": 200 * 1024 ** 3}


def answers(**extra):
    base = {
        "lang": "en_US.UTF-8",
        "keyboard": "us",
        "timezone": "Asia/Bangkok",
        "nicId": "eno1",
        "mode": "dhcp",
        "hostname": "pertisk-node",
        "adminPassword": "s3cret-pass",
        "adminConfirm": "s3cret-pass",
        "separateRoot": False,
        "peer": "",
        "eraseConfirmed": True,
    }
    base.update(extra)
    return base


def fail(message):
    print(f"FAIL {message}", file=sys.stderr)
    raise SystemExit(1)


def main():
    ks = server.build_kickstart(TEMPLATE, answers(), DISK, "uefi")
    if not ks.startswith("cmdline\n"):
        fail("interactive kickstart must be cmdline so Anaconda stays off the screen")
    if "\ngraphical\n" in f"\n{ks}":
        fail("interactive kickstart must not select the Anaconda GUI")
    for needle in (
        "rootpw --iscrypted",
        "user --name=admin",
        "ignoredisk --only-use=nvme0n1",
        "clearpart --all --initlabel --disklabel=gpt --drives=nvme0n1",
        "part /var/lib/pertisk --fstype=xfs --size=10240 --grow",
        "part / --fstype=xfs --size=10240 --maxsize=71680",
        "--hostname=pertisk-node",
        "timezone Asia/Bangkok --utc",
        "eula --agreed",
        "pertisk-vms",
        "file:///run/pertisk-repo",
        "/run/pertisk/admin.pass",
        "/run/pertisk/join.url",
        "7443:tcp,7480:tcp",
    ):
        if needle not in ks:
            fail(f"missing {needle}")
    if "s3cret-pass" in ks:
        fail("plaintext password leaked into kickstart")
    if "part biosboot" in ks:
        fail("UEFI layout must not add biosboot")
    if any(line.strip() == "reboot" for line in ks.splitlines()):
        fail("installer must reboot only when the operator asks")

    bios = server.build_kickstart(TEMPLATE, answers(), DISK, "bios")
    if "part biosboot --fstype=biosboot --size=1" not in bios:
        fail("BIOS layout must include biosboot")

    static = server.build_kickstart(
        TEMPLATE,
        answers(mode="static", address="10.1.1.42", prefix="24", gateway="10.1.1.1", dns="1.1.1.1"),
        DISK,
        "uefi",
    )
    if "--netmask=255.255.255.0" not in static or "--ip=10.1.1.42" not in static:
        fail("static network line")

    separate = server.build_kickstart(
        TEMPLATE,
        answers(
            separateRoot=True,
            rootPassword="root-pass-ok",
            rootConfirm="root-pass-ok",
        ),
        DISK,
        "uefi",
    )
    import re

    hashes = re.findall(r"\$6\$[A-Za-z0-9./]+\$[A-Za-z0-9./]+", separate)
    if len(set(hashes)) < 2:
        fail("separate root password must hash differently from admin")

    try:
        server.build_kickstart(TEMPLATE, answers(hostname="bad_host"), DISK, "uefi")
    except ValueError:
        pass
    else:
        fail("bad hostname must be rejected")

    try:
        server.build_kickstart(TEMPLATE, answers(), {"id": "nvme0n1;rm", "kind": "disk", "size": DISK["size"]}, "uefi")
    except ValueError:
        pass
    else:
        fail("disk id must be a single device name")

    small = dict(DISK)
    small["size"] = 8 * 1024 ** 3
    try:
        server.build_kickstart(TEMPLATE, answers(), small, "uefi")
    except ValueError:
        pass
    else:
        fail("small disk must be rejected")

    payload = server.extract_payload(TEMPLATE)
    if "%pre" in payload.split("%packages")[0]:
        fail("interactive payload must not keep the autodisk %pre")
    if "pertisk-vms" not in payload or "%post" not in payload:
        fail("payload must keep packages and %post")

    parsed = server.parse_lsblk(
        {
            "blockdevices": [
                {"name": "nvme0n1", "size": "1000000000000", "type": "disk", "rm": False, "tran": "nvme", "model": "Samsung"},
                {"name": "sda", "size": "30000000000", "type": "disk", "rm": True, "tran": "usb", "model": "Ultra Fit"},
                {"name": "sr0", "size": "1000", "type": "rom", "rm": True, "tran": "usb", "model": "CD"},
            ]
        }
    )
    kinds = {item["id"]: item["kind"] for item in parsed}
    if kinds.get("nvme0n1") != "disk" or kinds.get("sda") != "installer" or "sr0" in kinds:
        fail(f"lsblk classification {parsed}")

    early = "url --url=https://example\n%post\nset -euo pipefail\n%end\n"
    if server.detect_stage("", early, "", early) != 0:
        fail("kickstart text %post must not count as configuring the node")
    if server.detect_stage("Installing pertisk-vms", "", "", "") != 2:
        fail("package install should be stage 2")
    configuring = "INFO Running post-installation scripts\n"
    if server.detect_stage("pertisk-vms", configuring, "", configuring) != 3:
        fail("post-installation scripts should be stage 3")
    shim = server.SYSTEMCTL_SHIM
    if "reboot|poweroff|halt|kexec" not in shim:
        fail("systemctl shim must ignore Anaconda's automatic reboot")

    print("ok")


if __name__ == "__main__":
    main()
