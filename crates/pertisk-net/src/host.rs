use std::process::Command;

use crate::{Ipv4Net, NetError, Result, valid_ifname};

pub fn ensure_bridge(bridge: &str, gateway: Option<&str>, prefix: u8) -> Result<()> {
    check_name(bridge)?;
    run_ip(&["link", "add", "name", bridge, "type", "bridge"], true)?;
    if let Some(gateway) = gateway {
        let cidr = format!("{gateway}/{prefix}");
        run_ip(&["addr", "add", &cidr, "dev", bridge], true)?;
    }
    run_ip(&["link", "set", "dev", bridge, "up"], false)
}

pub fn interface_exists(name: &str) -> bool {
    valid_ifname(name) && std::path::Path::new("/sys/class/net").join(name).exists()
}

pub fn overlaps_existing_ipv4(network: Ipv4Net, except_interface: Option<&str>) -> Result<bool> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (network, except_interface);
        Ok(false)
    }
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("ip")
            .args(["-o", "-4", "addr", "show"])
            .output()
            .map_err(|err| NetError::Host(format!("ip -o -4 addr show: {err}")))?;
        if !output.status.success() {
            return Err(NetError::Host(format!(
                "ip -o -4 addr show failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            let Some(index) = fields.iter().position(|field| *field == "inet") else {
                continue;
            };
            if fields
                .get(1)
                .is_some_and(|interface| Some(interface.trim_end_matches(':')) == except_interface)
            {
                continue;
            }
            let Some(cidr) = fields.get(index + 1) else {
                continue;
            };
            if Ipv4Net::parse(cidr).is_ok_and(|existing| network.overlaps(existing)) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

pub fn provision_nic(
    bridge: &str,
    tap: &str,
    _gateway: Option<&str>,
    _prefix: u8,
    isolate: bool,
) -> Result<()> {
    check_name(bridge)?;
    check_name(tap)?;
    // A failed prior setup can leave this deterministic TAP behind without a VM record.
    delete_tap(tap)?;
    run_ip(&["tuntap", "add", "dev", tap, "mode", "tap"], true)?;
    run_ip(&["link", "set", "dev", tap, "master", bridge], false)?;
    run_ip(&["link", "set", "dev", tap, "up"], false)?;
    if isolate {
        let _ = Command::new("bridge")
            .args(["link", "set", "dev", tap, "isolated", "on"])
            .status();
    }
    Ok(())
}

pub fn delete_tap(tap: &str) -> Result<()> {
    check_name(tap)?;
    run_ip(&["link", "delete", "dev", tap], true)
}

fn check_name(name: &str) -> Result<()> {
    if valid_ifname(name) {
        Ok(())
    } else {
        Err(NetError::Invalid(format!("unsafe interface name '{name}'")))
    }
}

fn run_ip(args: &[&str], ignore_exists: bool) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (args, ignore_exists);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("ip")
            .args(args)
            .output()
            .map_err(|err| NetError::Host(format!("ip {}: {err}", args.join(" "))))?;
        if output.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&output.stderr);
        if ignore_exists
            && (err.contains("File exists")
                || err.contains("exists")
                || err.contains("Address already assigned")
                || err.contains("Cannot find device"))
        {
            return Ok(());
        }
        Err(NetError::Host(format!(
            "ip {} failed: {}",
            args.join(" "),
            err.trim()
        )))
    }
}
