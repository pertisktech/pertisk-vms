use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "static/"]
#[prefix = ""]
struct Assets;

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("v1/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if path.is_empty() || path == "index.html" {
        return serve("index.html");
    }
    if Assets::get(path).is_some() {
        return serve(path);
    }
    serve("index.html")
}

fn serve(path: &str) -> Response {
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .body(Body::from(content.data.to_vec()))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        None => {
            let html = r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"/><title>Pertisk VM</title>
<style>body{font-family:Inter,system-ui,sans-serif;background:#0c0d18;color:#e6e7f0;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}
.card{max-width:32rem;padding:2rem;border:1px solid #23253c;border-radius:12px;background:#131421}
code{background:#0c0d18;padding:0.2em 0.4em;border-radius:4px}
a{color:#9a7bf7}</style></head>
<body><div class="card"><h1>Pertisk VM</h1>
<p>API is up. Build the UI with <code>npm install && npm run build</code> in <code>web/ui</code>, then rebuild pertiskd.</p>
<p><a href="/v1/health">/v1/health</a></p></div></body></html>"#;
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(html))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}
