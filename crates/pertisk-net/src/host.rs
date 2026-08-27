use std::process::Command;

use crate::{NetError, Result, valid_ifname};

pub fn ensure_bridge(bridge: &str, gateway: Option<&str>, prefix: u8) -> Result<()> {
    check_name(bridge)?;
    run_ip(&["link", "add", "name", bridge, "type", "bridge"], true)?;
    if let Some(gateway) = gateway {
        let cidr = format!("{gateway}/{prefix}");
        run_ip(&["addr", "add", &cidr, "dev", bridge], true)?;
    }
    run_ip(&["link", "set", "dev", bridge, "up"], false)
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
        let output = Command::new("ip").args(args).output().map_err(|err| {
            NetError::Host(format!("ip {}: {err}", args.join(" ")))
        })?;
        if output.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&output.stderr);
        if ignore_exists
            && (err.contains("File exists") || err.contains("exists"))
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
