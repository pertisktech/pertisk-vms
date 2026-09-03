//! Discover this host's IPv4 and IPv6 addresses.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};
use std::time::Duration;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostAddrs {
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
}

/// Addresses suitable to display for this node (no loopback / link-local IPv4).
pub fn probe_host_addrs() -> HostAddrs {
    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    for ip in local_host_ips() {
        match ip {
            IpAddr::V4(ip) if usable_ipv4(ip) => push_unique(&mut ipv4, ip.to_string()),
            IpAddr::V6(ip) if usable_ipv6(ip) => push_unique(&mut ipv6, ip.to_string()),
            _ => {}
        }
    }
    ipv6.sort_by_key(|s| ipv6_rank(s));
    HostAddrs { ipv4, ipv6 }
}

/// Every address found on the host, including loopback. Used for TLS SANs.
pub fn local_host_ips() -> Vec<IpAddr> {
    let mut ips = vec![
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ];
    push_ip(&mut ips, egress_ip("0.0.0.0:0", "1.1.1.1:443"));
    push_ip(
        &mut ips,
        egress_ip("[::]:0", "[2001:4860:4860::8888]:443"),
    );
    for ip in hostname_ips() {
        push_ip(&mut ips, Some(ip));
    }
    for ip in proc_inet6() {
        push_ip(&mut ips, Some(ip));
    }
    ips
}

fn push_ip(ips: &mut Vec<IpAddr>, ip: Option<IpAddr>) {
    if let Some(ip) = ip
        && !ip.is_unspecified()
        && !ips.contains(&ip)
    {
        ips.push(ip);
    }
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.contains(&value) {
        out.push(value);
    }
}

fn usable_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_link_local()
        && !ip.is_multicast()
        && !ip.is_broadcast()
}

fn usable_ipv6(ip: Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast()
}

fn ipv6_rank(s: &str) -> u8 {
    s.parse::<Ipv6Addr>()
        .map(|ip| if ip.is_unicast_link_local() { 1 } else { 0 })
        .unwrap_or(2)
}

fn egress_ip(bind: &str, target: &str) -> Option<IpAddr> {
    let sock = UdpSocket::bind(bind).ok()?;
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    sock.connect(target).ok()?;
    Some(sock.local_addr().ok()?.ip())
}

fn hostname_ips() -> Vec<IpAddr> {
    let Ok(out) = std::process::Command::new("hostname").arg("-I").output() else {
        return Vec::new();
    };
    let Ok(text) = String::from_utf8(out.stdout) else {
        return Vec::new();
    };
    parse_hostname_ips(&text)
}

fn parse_hostname_ips(text: &str) -> Vec<IpAddr> {
    text.split_whitespace()
        .filter_map(|tok| tok.parse::<IpAddr>().ok())
        .collect()
}

fn proc_inet6() -> Vec<IpAddr> {
    let Ok(text) = std::fs::read_to_string("/proc/net/if_inet6") else {
        return Vec::new();
    };
    parse_if_inet6(&text)
}

fn parse_if_inet6(text: &str) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for line in text.lines() {
        let Some(hex) = line.split_whitespace().next() else {
            continue;
        };
        if let Some(ip) = parse_inet6_hex(hex) {
            ips.push(IpAddr::V6(ip));
        }
    }
    ips
}

fn parse_inet6_hex(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let s = std::str::from_utf8(chunk).ok()?;
        bytes[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(Ipv6Addr::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_tokens_keep_ipv6() {
        let ips = parse_hostname_ips("10.0.0.5 fe80::1 2001:db8::10 127.0.0.1");
        assert!(ips.iter().any(|ip| matches!(ip, IpAddr::V4(v) if v.octets() == [10, 0, 0, 5])));
        assert!(ips.iter().any(|ip| ip.is_ipv6()));
        let addrs = {
            let mut ipv4 = Vec::new();
            let mut ipv6 = Vec::new();
            for ip in ips {
                match ip {
                    IpAddr::V4(ip) if usable_ipv4(ip) => ipv4.push(ip.to_string()),
                    IpAddr::V6(ip) if usable_ipv6(ip) => ipv6.push(ip.to_string()),
                    _ => {}
                }
            }
            ipv6.sort_by_key(|s| ipv6_rank(s));
            HostAddrs { ipv4, ipv6 }
        };
        assert_eq!(addrs.ipv4, vec!["10.0.0.5"]);
        assert_eq!(addrs.ipv6[0], "2001:db8::10");
        assert!(addrs.ipv6.contains(&"fe80::1".into()));
    }

    #[test]
    fn parse_if_inet6_sample() {
        let text = "\
20010db8000000000000000000000001 02 40 00 80     eth0
fe800000000000000000000000000001 02 40 20 80     eth0
00000000000000000000000000000001 01 80 10 80       lo
";
        let ips = parse_if_inet6(text);
        assert!(ips.contains(&"2001:db8::1".parse().unwrap()));
        assert!(ips.contains(&"fe80::1".parse().unwrap()));
        assert!(ips.contains(&Ipv6Addr::LOCALHOST.into()));
    }

    #[test]
    fn local_host_ips_includes_loopback() {
        let ips = local_host_ips();
        assert!(ips.contains(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(ips.contains(&IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }
}
