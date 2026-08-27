use std::path::Path;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;

use crate::{Result, VmmError};

pub async fn put_json(socket: &Path, path: &str, body: Option<&[u8]>) -> Result<(u16, Vec<u8>)> {
    request(socket, Method::PUT, path, body).await
}

pub async fn get(socket: &Path, path: &str) -> Result<(u16, Vec<u8>)> {
    request(socket, Method::GET, path, None).await
}

async fn request(
    socket: &Path,
    method: Method,
    path: &str,
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>)> {
    let stream = UnixStream::connect(socket).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = http1::handshake(io)
        .await
        .map_err(|err| VmmError::Http(err.to_string()))?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let payload = body.unwrap_or(b"");
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::copy_from_slice(payload)))
        .map_err(|err| VmmError::Http(err.to_string()))?;

    let response = sender
        .send_request(req)
        .await
        .map_err(|err| VmmError::Http(err.to_string()))?;
    let status = response.status().as_u16();
    let collected = response
        .into_body()
        .collect()
        .await
        .map_err(|err| VmmError::Http(err.to_string()))?;
    Ok((status, collected.to_bytes().to_vec()))
}

pub async fn wait_ready(socket: &Path, timeout: Duration) -> Result<()> {
    let start = tokio::time::Instant::now();
    loop {
        if socket.exists()
            && let Ok((status, _)) = get(socket, "/api/v1/vmm.ping").await
            && (200..300).contains(&status)
        {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(VmmError::Message(format!(
                "timed out waiting for cloud-hypervisor socket {}",
                socket.display()
            )));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub fn expect_ok(status: u16, body: &[u8]) -> Result<()> {
    if (200..300).contains(&status) || status == StatusCode::NO_CONTENT.as_u16() {
        Ok(())
    } else {
        Err(VmmError::Api {
            status,
            body: String::from_utf8_lossy(body).into_owned(),
        })
    }
}
