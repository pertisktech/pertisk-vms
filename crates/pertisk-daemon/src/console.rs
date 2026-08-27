//! Per-VM serial multiplexer: file tail (mock) or unix-socket proxy (Cloud Hypervisor).

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pertisk_types::VmId;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::warn;

#[derive(Clone, Default)]
pub struct ConsoleHub {
    inner: Arc<Mutex<HashMap<VmId, Session>>>,
}

struct Session {
    out: broadcast::Sender<Vec<u8>>,
    input: mpsc::UnboundedSender<Vec<u8>>,
    has_socket: bool,
}

impl ConsoleHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn ensure(&self, id: VmId, serial_log: PathBuf, socket: Option<PathBuf>) {
        let mut map = self.inner.lock().await;
        if let Some(existing) = map.get(&id)
            && existing.has_socket == socket.is_some()
        {
            return;
        }
        map.remove(&id);
        let (out, _) = broadcast::channel(256);
        let (input, in_rx) = mpsc::unbounded_channel();
        map.insert(
            id,
            Session {
                out: out.clone(),
                input,
                has_socket: socket.is_some(),
            },
        );
        drop(map);
        if let Some(socket) = socket {
            tokio::spawn(unix_or_file(id, socket, serial_log, out, in_rx));
        } else {
            tokio::spawn(file_pump(serial_log, out, in_rx));
        }
    }

    pub async fn subscribe(
        &self,
        id: VmId,
    ) -> Option<(broadcast::Receiver<Vec<u8>>, mpsc::UnboundedSender<Vec<u8>>)> {
        let map = self.inner.lock().await;
        map.get(&id)
            .map(|session| (session.out.subscribe(), session.input.clone()))
    }

    pub async fn write(&self, id: VmId, data: Vec<u8>) -> bool {
        let map = self.inner.lock().await;
        match map.get(&id) {
            Some(session) => session.input.send(data).is_ok(),
            None => false,
        }
    }

    pub async fn drop_vm(&self, id: VmId) {
        self.inner.lock().await.remove(&id);
    }
}

async fn unix_or_file(
    _id: VmId,
    socket: PathBuf,
    serial_log: PathBuf,
    out: broadcast::Sender<Vec<u8>>,
    input: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    #[cfg(unix)]
    {
        match wait_unix(&socket).await {
            Ok(stream) => {
                unix_pump(stream, serial_log, out, input).await;
                return;
            }
            Err(err) => warn!(path = %socket.display(), %err, "console socket; using serial file"),
        }
    }
    let _ = socket;
    file_pump(serial_log, out, input).await;
}

#[cfg(unix)]
async fn wait_unix(path: &std::path::Path) -> std::io::Result<tokio::net::UnixStream> {
    for _ in 0..50 {
        if path.exists() {
            match tokio::net::UnixStream::connect(path).await {
                Ok(stream) => return Ok(stream),
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("console socket {} not ready", path.display()),
    ))
}

#[cfg(unix)]
async fn unix_pump(
    stream: tokio::net::UnixStream,
    serial_log: PathBuf,
    out: broadcast::Sender<Vec<u8>>,
    mut input: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut buf = vec![0u8; 4096];
    loop {
        tokio::select! {
            read = reader.read(&mut buf) => {
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        append_log(&serial_log, &chunk).await;
                        let _ = out.send(chunk);
                    }
                }
            }
            msg = input.recv() => {
                match msg {
                    Some(bytes) => {
                        if writer.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

async fn file_pump(
    serial_log: PathBuf,
    out: broadcast::Sender<Vec<u8>>,
    mut input: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let mut pos = file_len(&serial_log).await;
    loop {
        tokio::select! {
            msg = input.recv() => {
                match msg {
                    Some(bytes) => append_log(&serial_log, &bytes).await,
                    None => break,
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(150)) => {
                if let Some(chunk) = read_from(&serial_log, pos).await {
                    pos += chunk.len() as u64;
                    if !chunk.is_empty() {
                        let _ = out.send(chunk);
                    }
                }
            }
        }
    }
}

async fn file_len(path: &std::path::Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0)
}

async fn read_from(path: &std::path::Path, from: u64) -> Option<Vec<u8>> {
    let mut file = tokio::fs::File::open(path).await.ok()?;
    file.seek(SeekFrom::Start(from)).await.ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await.ok()?;
    Some(buf)
}

async fn append_log(path: &std::path::Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(mut file) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    {
        let _ = file.write_all(bytes).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pertisk_types::VmId;

    #[tokio::test]
    async fn file_console_echoes_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serial");
        std::fs::write(&path, "boot\n").unwrap();
        let hub = ConsoleHub::new();
        let id = VmId::new();
        hub.ensure(id, path.clone(), None).await;
        let (mut rx, _) = hub.subscribe(id).await.expect("session");
        assert!(hub.write(id, b"hi\n".to_vec()).await);
        let chunk = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let bytes = rx.recv().await.expect("broadcast");
                if String::from_utf8_lossy(&bytes).contains("hi") {
                    return bytes;
                }
            }
        })
        .await
        .expect("timed out waiting for console input");
        assert!(String::from_utf8_lossy(&chunk).contains("hi"));
    }
}
