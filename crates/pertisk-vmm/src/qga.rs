//! Best-effort QEMU guest agent (QGA) queries over the virtio-serial unix socket.

use std::io::{Read, Write};
use std::net::Ipv6Addr;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GuestAddrs {
    pub ipv4: Vec<(String, String)>,
    pub ipv6: Vec<(String, String)>,
}

/// Return IPv4 addresses keyed by interface MAC from `guest-network-get-interfaces`.
pub fn ipv4_by_mac(qga_sock: &Path) -> Vec<(String, String)> {
    addrs_by_mac(qga_sock).ipv4
}

/// Return IPv4 and IPv6 addresses keyed by interface MAC.
pub fn addrs_by_mac(qga_sock: &Path) -> GuestAddrs {
    let Ok(raw) = qga_command(qga_sock, "guest-network-get-interfaces", None) else {
        return GuestAddrs::default();
    };
    parse_guest_addrs(&raw)
}

fn parse_guest_addrs(raw: &Value) -> GuestAddrs {
    let Some(list) = raw.as_array() else {
        return GuestAddrs::default();
    };
    let mut out = GuestAddrs::default();
    for iface in list {
        let mac = iface
            .get("hardware-address")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mac = match normalize_mac(mac) {
            Some(m) => m,
            None => continue,
        };
        let Some(addrs) = iface.get("ip-addresses").and_then(|v| v.as_array()) else {
            continue;
        };
        for addr in addrs {
            let kind = addr
                .get("ip-address-type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let Some(ip) = addr.get("ip-address").and_then(|v| v.as_str()) else {
                continue;
            };
            match kind {
                "ipv4" if usable_guest_ipv4(ip) => out.ipv4.push((mac.clone(), ip.to_string())),
                "ipv6" if usable_guest_ipv6(ip) => out.ipv6.push((mac.clone(), ip.to_string())),
                _ => {}
            }
        }
    }
    out
}

fn usable_guest_ipv4(ip: &str) -> bool {
    !ip.starts_with("127.") && !ip.starts_with("169.254.")
}

fn usable_guest_ipv6(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<Ipv6Addr>() else {
        return false;
    };
    !addr.is_loopback()
        && !addr.is_unspecified()
        && !addr.is_multicast()
        && !addr.is_unicast_link_local()
}

fn qga_command(path: &Path, execute: &str, arguments: Option<Value>) -> std::io::Result<Value> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_millis(400)))?;
    stream.set_write_timeout(Some(Duration::from_millis(400)))?;

    // Sync so we know the agent is alive and the stream is framed.
    let sync_id = (std::process::id() as i64)
        .wrapping_mul(1_000_000)
        .wrapping_add(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as i64)
                .unwrap_or(0),
        );
    let sync = serde_json::json!({
        "execute": "guest-sync",
        "arguments": { "id": sync_id }
    });
    write_msg(&mut stream, &sync)?;
    let _ = read_msg(&mut stream)?;

    let mut body = serde_json::json!({ "execute": execute });
    if let Some(args) = arguments {
        body.as_object_mut()
            .expect("object")
            .insert("arguments".into(), args);
    }
    write_msg(&mut stream, &body)?;
    let resp = read_msg(&mut stream)?;
    if let Some(err) = resp.get("error") {
        return Err(std::io::Error::other(err.to_string()));
    }
    Ok(resp.get("return").cloned().unwrap_or(Value::Null))
}

fn write_msg(stream: &mut UnixStream, value: &Value) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)
}

fn read_msg(stream: &mut UnixStream) -> std::io::Result<Value> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1024];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.contains(&b'\n') || looks_complete_json(&buf) {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err),
        }
        if buf.len() > 256 * 1024 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            return Ok(value);
        }
    }
    serde_json::from_str(text.trim()).map_err(|err| std::io::Error::other(err.to_string()))
}

fn looks_complete_json(buf: &[u8]) -> bool {
    let text = String::from_utf8_lossy(buf);
    let t = text.trim();
    t.starts_with('{') && t.ends_with('}') && t.matches('{').count() == t.matches('}').count()
}

fn normalize_mac(mac: &str) -> Option<String> {
    let hex: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if hex.len() != 12 {
        return None;
    }
    Some(
        hex.as_bytes()
            .chunks(2)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or("00"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_ipv4_and_ipv6_from_qga() {
        let raw = json!([
            {
                "name": "lo",
                "hardware-address": "00:00:00:00:00:00",
                "ip-addresses": [
                    { "ip-address-type": "ipv4", "ip-address": "127.0.0.1" },
                    { "ip-address-type": "ipv6", "ip-address": "::1" }
                ]
            },
            {
                "name": "eth0",
                "hardware-address": "52:54:00:12:34:56",
                "ip-addresses": [
                    { "ip-address-type": "ipv4", "ip-address": "10.88.0.12" },
                    { "ip-address-type": "ipv6", "ip-address": "fe80::1" },
                    { "ip-address-type": "ipv6", "ip-address": "fd00:3::10" }
                ]
            }
        ]);
        let addrs = parse_guest_addrs(&raw);
        assert_eq!(
            addrs.ipv4,
            vec![("52:54:00:12:34:56".into(), "10.88.0.12".into())]
        );
        assert_eq!(
            addrs.ipv6,
            vec![("52:54:00:12:34:56".into(), "fd00:3::10".into())]
        );
    }
}
