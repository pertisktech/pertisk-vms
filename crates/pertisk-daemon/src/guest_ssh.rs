//! Interactive SSH into a running guest (browser terminal → host ssh client).

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use axum::extract::ws::{Message, WebSocket};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;
use tracing::warn;

use crate::shell::{PtyIn, parse_resize};

pub struct GuestSshTarget {
    pub host: String,
    pub user: String,
    pub identity: Option<PathBuf>,
}

pub async fn proxy(socket: WebSocket, target: GuestSshTarget) {
    match spawn_ssh(&target) {
        Ok(session) => pump(socket, session).await,
        Err(err) => {
            let mut socket = socket;
            let _ = socket
                .send(Message::Text(format!("ssh: {err}\r\n").into()))
                .await;
        }
    }
}

struct SshSession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

fn spawn_ssh(target: &GuestSshTarget) -> Result<SshSession, String> {
    let system = NativePtySystem::default();
    let pair = system
        .openpty(PtySize {
            rows: 32,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("open pty: {err}"))?;

    // Guests are often rebuilt and reuse the same DHCP IP with a new host key.
    // Drop any stale known_hosts entry for this address before connecting.
    forget_known_host(&target.host);

    let ssh = ssh_bin().ok_or_else(|| {
        "ssh client missing (install openssh-client on the node)".to_string()
    })?;
    let mut cmd = CommandBuilder::new(ssh);
    cmd.arg("-tt");
    cmd.arg("-o");
    cmd.arg("StrictHostKeyChecking=accept-new");
    cmd.arg("-o");
    cmd.arg("UserKnownHostsFile=/var/lib/pertisk/ssh/known_hosts");
    cmd.arg("-o");
    cmd.arg("LogLevel=ERROR");
    cmd.arg("-o");
    cmd.arg("ConnectTimeout=10");
    cmd.arg("-o");
    cmd.arg("PreferredAuthentications=publickey,password,keyboard-interactive");
    if let Some(identity) = &target.identity {
        cmd.arg("-i");
        cmd.arg(identity);
    }
    cmd.arg(format!("{}@{}", target.user, target.host));
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("LANG", "C.UTF-8");
    cmd.env("LC_ALL", "C.UTF-8");

    let _ = std::fs::create_dir_all("/var/lib/pertisk/ssh");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| format!("spawn ssh: {err}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("pty reader: {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("pty writer: {err}"))?;
    Ok(SshSession {
        child,
        master: Arc::new(Mutex::new(pair.master)),
        reader,
        writer,
    })
}

/// Remove a guest address from the node known_hosts file (IP reuse after rebuild).
fn forget_known_host(host: &str) {
    let host = host.trim();
    if host.is_empty() {
        return;
    }
    let known = Path::new("/var/lib/pertisk/ssh/known_hosts");
    if !known.is_file() {
        return;
    }
    let Some(keygen) = ["/usr/bin/ssh-keygen", "/bin/ssh-keygen"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
    else {
        return;
    };
    let _ = std::process::Command::new(keygen)
        .arg("-f")
        .arg(known)
        .arg("-R")
        .arg(host)
        .output();
}

fn ssh_bin() -> Option<&'static str> {
    ["/usr/bin/ssh", "/bin/ssh"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
}

pub fn find_identity() -> Option<PathBuf> {
    ensure_identity();
    const CANDIDATES: &[&str] = &[
        "/etc/pertisk/ssh/id_ed25519",
        "/etc/pertisk/ssh/id_rsa",
        "/root/.ssh/id_ed25519",
        "/root/.ssh/id_rsa",
    ];
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

pub fn public_key() -> Option<String> {
    ensure_identity();
    let text = std::fs::read_to_string("/etc/pertisk/ssh/id_ed25519.pub").ok()?;
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("ssh-") || line.starts_with("ecdsa-"))
        .map(|line| line.to_string())
}

/// Create `/etc/pertisk/ssh/id_ed25519` when missing and append its pubkey to authorized_keys.
pub fn ensure_identity() {
    let key = Path::new("/etc/pertisk/ssh/id_ed25519");
    let pub_path = Path::new("/etc/pertisk/ssh/id_ed25519.pub");
    let auth = Path::new("/etc/pertisk/ssh/authorized_keys");
    let _ = std::fs::create_dir_all("/etc/pertisk/ssh");
    let _ = std::fs::create_dir_all("/var/lib/pertisk/ssh");
    if !key.is_file() {
        let keygen = ["/usr/bin/ssh-keygen", "/bin/ssh-keygen"]
            .into_iter()
            .find(|p| Path::new(p).is_file());
        if let Some(bin) = keygen {
            let status = std::process::Command::new(bin)
                .args([
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-C",
                    "pertisk-node",
                    "-f",
                    key.to_str().unwrap_or("/etc/pertisk/ssh/id_ed25519"),
                ])
                .status();
            if !matches!(status, Ok(s) if s.success()) {
                return;
            }
            let _ = std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600));
        }
    }
    if pub_path.is_file() {
        let Ok(pub_line) = std::fs::read_to_string(pub_path) else {
            return;
        };
        let pub_line = pub_line.trim();
        if pub_line.is_empty() {
            return;
        }
        let existing = std::fs::read_to_string(auth).unwrap_or_default();
        if !existing.lines().any(|l| l.trim() == pub_line) {
            let mut body = existing;
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(pub_line);
            body.push('\n');
            let _ = std::fs::write(auth, body);
            let _ = std::fs::set_permissions(auth, std::fs::Permissions::from_mode(0o600));
        }
    }
}

async fn pump(mut socket: WebSocket, session: SshSession) {
    let SshSession {
        mut child,
        master,
        mut reader,
        mut writer,
    } = session;
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<PtyIn>();

    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let master_for_resize = master.clone();
    thread::spawn(move || {
        while let Some(msg) = in_rx.blocking_recv() {
            match msg {
                PtyIn::Data(bytes) => {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
                PtyIn::Resize { cols, rows } => {
                    if let Ok(master) = master_for_resize.lock() {
                        let _ = master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Some((cols, rows)) = parse_resize(&text) {
                            let _ = in_tx.send(PtyIn::Resize { cols, rows });
                        } else if in_tx.send(PtyIn::Data(text.as_bytes().to_vec())).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if in_tx.send(PtyIn::Data(bytes.to_vec())).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if socket.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            chunk = out_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    if let Err(err) = child.kill() {
        warn!(error = %err, "guest ssh kill");
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::find_identity;

    #[test]
    fn find_identity_is_optional() {
        let _ = find_identity();
    }
}
