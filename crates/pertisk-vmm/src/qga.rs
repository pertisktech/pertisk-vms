//! Best-effort QEMU guest agent (QGA) queries over the virtio-serial unix socket.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

/// Return IPv4 addresses keyed by interface MAC from `guest-network-get-interfaces`.
pub fn ipv4_by_mac(qga_sock: &Path) -> Vec<(String, String)> {
    let Ok(raw) = qga_command(qga_sock, "guest-network-get-interfaces", None) else {
        return Vec::new();
    };
    let Some(list) = raw.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
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
            if kind != "ipv4" {
                continue;
            }
            let Some(ip) = addr.get("ip-address").and_then(|v| v.as_str()) else {
                continue;
            };
            if ip.starts_with("127.") || ip.starts_with("169.254.") {
                continue;
            }
            out.push((mac.clone(), ip.to_string()));
        }
    }
    out
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
        body
            .as_object_mut()
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
