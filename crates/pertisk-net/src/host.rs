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

pub fn delete_bridge(bridge: &str) -> Result<()> {
    check_name(bridge)?;
    run_ip(&["link", "delete", "dev", bridge, "type", "bridge"], true)
}

/// Give an isolated guest bridge IPv4 egress through the host's default route.
pub fn ensure_ipv4_egress(bridge: &str, network: Ipv4Net) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (bridge, network);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        check_name(bridge)?;
        let uplink = default_uplink()?;
        let cidr = network.to_cidr_string();
        run_command("sysctl", &["-w", "net.ipv4.ip_forward=1"])?;
        ensure_iptables_rule(&["-t", "nat", "POSTROUTING", "-s", &cidr, "-o", &uplink, "-j", "MASQUERADE"])?;
        ensure_iptables_rule(&["FORWARD", "-i", bridge, "-o", &uplink, "-j", "ACCEPT"])?;
        ensure_iptables_rule(&[
            "FORWARD",
            "-i",
            &uplink,
            "-o",
            bridge,
            "-m",
            "conntrack",
            "--ctstate",
            "RELATED,ESTABLISHED",
            "-j",
            "ACCEPT",
        ])
    }
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

#[cfg(target_os = "linux")]
fn default_uplink() -> Result<String> {
    let output = Command::new("ip")
        .args(["-o", "-4", "route", "show", "to", "default"])
        .output()
        .map_err(|err| NetError::Host(format!("ip route: {err}")))?;
    if !output.status.success() {
        return Err(NetError::Host(format!(
            "ip route failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let route = String::from_utf8_lossy(&output.stdout);
    let fields: Vec<_> = route.split_whitespace().collect();
    let dev = fields
        .windows(2)
        .find_map(|pair| (pair[0] == "dev").then_some(pair[1]))
        .filter(|name| valid_ifname(name))
        .ok_or_else(|| NetError::Host("no IPv4 default-route interface found".into()))?;
    Ok(dev.into())
}

#[cfg(target_os = "linux")]
fn ensure_iptables_rule(rule: &[&str]) -> Result<()> {
    let (prefix, rule) = match rule {
        ["-t", table, rest @ ..] => (vec!["-t", *table], rest),
        _ => (Vec::new(), rule),
    };
    let mut check = prefix.clone();
    check.push("-C");
    check.extend_from_slice(rule);
    let exists = Command::new("iptables").args(&check).status();
    match exists {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => {
            let mut add = prefix;
            add.push("-A");
            add.extend_from_slice(rule);
            run_command("iptables", &add)
        }
        Err(err) => Err(NetError::Host(format!("iptables: {err}"))),
    }
}

#[cfg(target_os = "linux")]
fn run_command(command: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|err| NetError::Host(format!("{command}: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(NetError::Host(format!(
            "{command} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}
