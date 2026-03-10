use axum::body::Body;
use axum::http::{Response, StatusCode, header};
use std::path::Path;

/// Serve a static file from an extracted site directory.
pub async fn serve_file(site_dir: &Path, request_path: &str) -> Response<Body> {
    // Strip leading slash, default to empty
    let rel_path = request_path.strip_prefix('/').unwrap_or(request_path);

    // Try exact file match
    let file_path = site_dir.join(rel_path);
    if file_path.is_file() {
        return file_response(&file_path).await;
    }

    // Try directory index
    let index_path = file_path.join("index.html");
    if index_path.is_file() {
        return file_response(&index_path).await;
    }

    // 404
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(
            "<html><body><h1>404 Not Found</h1><p>This page does not exist in the sealed bundle.</p></body></html>",
        ))
        .unwrap()
}

async fn file_response(path: &Path) -> Response<Body> {
    // Security: ensure the resolved path doesn't escape the site directory via symlinks/..
    // (We canonicalize both and check containment in the server layer.)
    match tokio::fs::read(path).await {
        Ok(contents) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, &mime)
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(Body::from(contents))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to read file"))
            .unwrap(),
    }
}
