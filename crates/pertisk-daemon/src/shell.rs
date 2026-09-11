//! Interactive host shell (Proxmox-style Node → Shell).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use axum::extract::ws::{Message, WebSocket};
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;
use tracing::warn;

pub async fn proxy(socket: WebSocket) {
    match spawn_shell() {
        Ok(session) => pump(socket, session).await,
        Err(err) => {
            let mut socket = socket;
            let _ = socket
                .send(Message::Text(format!("shell: {err}\r\n").into()))
                .await;
        }
    }
}

struct ShellSession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

fn spawn_shell() -> Result<ShellSession, String> {
    let system = NativePtySystem::default();
    let pair = system
        .openpty(PtySize {
            rows: 32,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| format!("open pty: {err}"))?;
    let mut cmd = CommandBuilder::new(shell_bin());
    cmd.arg("-l");
    cmd.env("TERM", "xterm-256color");
    cmd.env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    if Path::new("/root").is_dir() {
        cmd.cwd("/root");
        cmd.env("HOME", "/root");
        cmd.env("USER", "root");
        cmd.env("LOGNAME", "root");
    }
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| format!("spawn shell: {err}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("pty reader: {err}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("pty writer: {err}"))?;
    Ok(ShellSession {
        child,
        master: Arc::new(Mutex::new(pair.master)),
        reader,
        writer,
    })
}

fn shell_bin() -> &'static str {
    ["/bin/bash", "/usr/bin/bash", "/bin/sh", "/usr/bin/sh"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .unwrap_or("/bin/sh")
}

enum PtyIn {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

async fn pump(mut socket: WebSocket, session: ShellSession) {
    let ShellSession {
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
        warn!(error = %err, "host shell kill");
    }
    let _ = child.wait();
}

fn parse_resize(text: &str) -> Option<(u16, u16)> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    if value.get("type")?.as_str()? != "resize" {
        return None;
    }
    let cols = value.get("cols")?.as_u64()? as u16;
    let rows = value.get("rows")?.as_u64()? as u16;
    if cols == 0 || rows == 0 {
        return None;
    }
    Some((cols, rows))
}

#[cfg(test)]
mod tests {
    use super::parse_resize;

    #[test]
    fn parse_resize_json() {
        assert_eq!(
            parse_resize(r#"{"type":"resize","cols":100,"rows":40}"#),
            Some((100, 40))
        );
        assert_eq!(parse_resize("echo hi"), None);
    }
}
