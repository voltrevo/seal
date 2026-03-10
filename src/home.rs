use crate::local::handle_upload;
use crate::url::local_app_host;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use seal::state::AppState;
use std::sync::OnceLock;

const BANNER: &[u8] = include_bytes!("../assets/banner.avif");
const SAMPLE_APP_BR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sample-app.zip.br"));

fn sample_app_zip() -> &'static [u8] {
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut out = Vec::new();
        brotli::BrotliDecompress(&mut &SAMPLE_APP_BR[..], &mut out)
            .expect("embedded brotli data is valid");
        out
    })
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index_page))
        .route("/banner.avif", get(serve_banner))
        .route("/sample-app.zip", get(serve_sample_app))
        .route("/local", get(local_page))
        .route("/local/upload", post(handle_upload))
        .route("/local/forget", post(handle_forget))
        .route("/install", get(install_page))
        .route("/install/do", post(handle_install))
}

async fn serve_banner() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "image/avif")],
        BANNER,
    )
}

async fn serve_sample_app() -> impl axum::response::IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/zip"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"seal-game-of-life.zip\"",
            ),
        ],
        sample_app_zip(),
    )
}

async fn index_page(State(state): State<AppState>) -> Html<String> {
    let local_apps = state.list_local_apps().await;
    let registered_apps = state.list_registered_apps().await;

    let registered_list = if registered_apps.is_empty() {
        r#"<p class="empty">No registered apps installed.</p>"#.to_string()
    } else {
        let items: Vec<String> = registered_apps
            .iter()
            .map(|app| {
                format!(
                    r#"<li><a href="{seal_url}/">{name}</a> <span class="hash">v{version}</span></li>"#,
                    seal_url = html_escape(&app.seal_url),
                    name = html_escape(&app.name),
                    version = html_escape(&app.version),
                )
            })
            .collect();
        format!("<ul>{}</ul>", items.join("\n"))
    };

    let app_list = if local_apps.is_empty() {
        r#"<p class="empty">No local apps.</p>"#.to_string()
    } else {
        let items: Vec<String> = local_apps
            .iter()
            .map(|app| {
                let host = local_app_host(&app.hash);
                let hash = html_escape(&app.hash);
                format!(
                    r#"<li><a href="https://{host}/">{name}</a> <span class="hash">{short_hash}…</span> <button class="forget" title="Forget app" onclick="forgetApp('{hash}')">🗑</button></li>"#,
                    name = html_escape(&app.name),
                    short_hash = &app.hash[..app.hash.len().min(12)],
                )
            })
            .collect();
        format!("<ul>{}</ul>", items.join("\n"))
    };

    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Seal</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: system-ui, -apple-system, sans-serif; max-width: 640px; margin: 0 auto; padding: 2rem 1rem; color: #1a1a2e; }}
  h1 {{ font-size: 1.5rem; margin-bottom: 0.5rem; }}
  h1 span {{ font-size: 1.8rem; }}
  .subtitle {{ color: #666; margin-bottom: 2rem; }}
  nav {{ margin-bottom: 2rem; display: flex; gap: 1rem; }}
  nav a, nav button {{ color: #2563eb; text-decoration: none; padding: 0.5rem 1rem; border: 1px solid #2563eb; border-radius: 6px; background: none; font: inherit; cursor: pointer; }}
  nav a:hover, nav button:hover {{ background: #2563eb; color: white; }}
  ul {{ list-style: none; }}
  li {{ padding: 0.75rem 0; border-bottom: 1px solid #eee; }}
  li a {{ color: #2563eb; text-decoration: none; font-weight: 500; }}
  li a:hover {{ text-decoration: underline; }}
  .hash {{ color: #999; font-family: monospace; font-size: 0.85rem; margin-left: 0.5rem; }}
  .empty {{ color: #999; font-style: italic; }}
  .forget {{ background: none; border: none; cursor: pointer; font-size: 1rem; opacity: 0.4; padding: 0 0.25rem; vertical-align: middle; }}
  .forget:hover {{ opacity: 1; }}
  .drop-overlay {{ display: none; position: fixed; inset: 0; background: rgba(37,99,235,0.12); border: 3px dashed #2563eb; z-index: 100; align-items: center; justify-content: center; font-size: 1.3rem; color: #2563eb; font-weight: 600; }}
  .drop-overlay.visible {{ display: flex; }}
  .status {{ margin-top: 1rem; color: #666; }}
  .status.error {{ color: #dc2626; }}
</style>
</head>
<body>
  <img src="/banner.avif" alt="Seal" style="width:100%;border-radius:12px;margin-bottom:1.5rem;">
  <h1><span>🦭</span> Seal</h1>
  <p class="subtitle">Secure frontends</p>
  <nav>
    <a href="/local">Add zipped app</a>
    <a href="/sample-app.zip" download>Download sample app</a>
  </nav>
  <h2>Registered Apps</h2>
  {registered_list}
  <h2 style="margin-top:1.5rem;">Local Apps</h2>
  {app_list}
  <div class="status" id="status"></div>
  <div class="drop-overlay" id="drop-overlay">Drop .zip to add</div>
  <script>
    async function forgetApp(hash) {{
      if (!confirm('Forget this app?')) return;
      const r = await fetch('/local/forget', {{ method: 'POST', headers: {{'Content-Type': 'application/x-www-form-urlencoded'}}, body: 'hash=' + encodeURIComponent(hash) }});
      if (r.ok) location.reload();
      else alert('Error: ' + await r.text());
    }}
    const overlay = document.getElementById('drop-overlay');
    const status = document.getElementById('status');
    let dragCount = 0;
    document.addEventListener('dragenter', e => {{ e.preventDefault(); if (++dragCount === 1) overlay.classList.add('visible'); }});
    document.addEventListener('dragleave', e => {{ e.preventDefault(); if (--dragCount === 0) overlay.classList.remove('visible'); }});
    document.addEventListener('dragover', e => e.preventDefault());
    document.addEventListener('drop', async e => {{
      e.preventDefault(); dragCount = 0; overlay.classList.remove('visible');
      const file = e.dataTransfer.files[0];
      if (!file || !file.name.endsWith('.zip')) {{ status.textContent = 'Please drop a .zip file'; status.className = 'status error'; return; }}
      status.textContent = 'Adding ' + file.name + '…'; status.className = 'status';
      const fd = new FormData(); fd.append('file', file);
      try {{
        const r = await fetch('/local/upload', {{ method: 'POST', body: fd }});
        if (r.ok) location.reload();
        else {{ status.textContent = 'Error: ' + await r.text(); status.className = 'status error'; }}
      }} catch (err) {{ status.textContent = 'Error: ' + err.message; status.className = 'status error'; }}
    }});
  </script>
</body>
</html>"#
    ))
}

async fn handle_forget(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(hash) = form.get("hash") else {
        return (StatusCode::BAD_REQUEST, "missing hash").into_response();
    };
    match state.forget_local_app(hash).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "app not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {e}")).into_response(),
    }
}

async fn install_page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Installing App — Seal</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: system-ui, -apple-system, sans-serif; max-width: 640px; margin: 0 auto; padding: 2rem 1rem; color: #1a1a2e; }
  h1 { font-size: 1.5rem; margin-bottom: 1rem; }
  .status { color: #666; margin-bottom: 1rem; }
  .status.error { color: #dc2626; }
  .spinner { display: inline-block; width: 1.2em; height: 1.2em; border: 2px solid #ccc; border-top-color: #2563eb; border-radius: 50%; animation: spin 0.8s linear infinite; vertical-align: middle; margin-right: 0.5rem; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .back { color: #2563eb; text-decoration: none; display: inline-block; margin-top: 1rem; }
</style>
</head>
<body>
  <h1>Installing App</h1>
  <p class="status" id="status"><span class="spinner"></span> Discovering and installing...</p>
  <a href="/" class="back" id="back" style="display:none">Back to home</a>
  <script>
    const params = new URLSearchParams(location.search);
    const url = params.get('url');
    const status = document.getElementById('status');
    const back = document.getElementById('back');

    if (!url) {
      status.textContent = 'Error: missing url parameter';
      status.className = 'status error';
      back.style.display = 'inline-block';
    } else {
      fetch('/install/do', {
        method: 'POST',
        headers: {'Content-Type': 'application/x-www-form-urlencoded'},
        body: 'url=' + encodeURIComponent(url)
      })
      .then(r => r.ok ? r.text() : r.text().then(t => { throw new Error(t); }))
      .then(redirect => { window.location.href = redirect; })
      .catch(err => {
        status.textContent = 'Error: ' + err.message;
        status.className = 'status error';
        back.style.display = 'inline-block';
      });
    }
  </script>
</body>
</html>"#,
    )
}

async fn handle_install(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(url) = form.get("url") else {
        return (StatusCode::BAD_REQUEST, "missing url").into_response();
    };

    match crate::registry::install_app(&state, url).await {
        Ok(redirect_url) => redirect_url.into_response(),
        Err(e) => {
            tracing::error!("install failed for {url}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response()
        }
    }
}

async fn local_page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Add Zipped App — Seal</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: system-ui, -apple-system, sans-serif; max-width: 640px; margin: 0 auto; padding: 2rem 1rem; color: #1a1a2e; }
  h1 { font-size: 1.5rem; margin-bottom: 0.5rem; }
  .back { color: #2563eb; text-decoration: none; display: inline-block; margin-bottom: 1.5rem; }
  .back:hover { text-decoration: underline; }
  .drop-zone {
    border: 2px dashed #ccc; border-radius: 12px; padding: 3rem 2rem;
    text-align: center; cursor: pointer; transition: all 0.2s;
    margin-bottom: 1rem;
  }
  .drop-zone.over { border-color: #2563eb; background: #f0f7ff; }
  .drop-zone p { color: #666; margin-bottom: 1rem; }
  .drop-zone .icon { font-size: 2.5rem; margin-bottom: 0.5rem; }
  input[type="file"] { display: none; }
  .status { margin-top: 1rem; color: #666; }
  .status.error { color: #dc2626; }
</style>
</head>
<body>
  <a href="/" class="back">← Back</a>
  <h1>Add Zipped App</h1>
  <p style="color: #666; margin-bottom: 1.5rem;">
    Drop a .zip file to serve it locally under a content-addressed .seal URL.
  </p>

  <form id="upload-form" action="/local/upload" method="post" enctype="multipart/form-data">
    <div class="drop-zone" id="drop-zone">
      <div class="icon">📦</div>
      <p>Drag & drop a .zip file here</p>
      <p style="font-size: 0.85rem;">or click to browse</p>
      <input type="file" name="file" id="file-input" accept=".zip">
    </div>
    <div class="status" id="status"></div>
  </form>

  <script>
    const dropZone = document.getElementById('drop-zone');
    const fileInput = document.getElementById('file-input');
    const form = document.getElementById('upload-form');
    const status = document.getElementById('status');

    dropZone.addEventListener('click', () => fileInput.click());

    dropZone.addEventListener('dragover', (e) => {
      e.preventDefault();
      dropZone.classList.add('over');
    });

    dropZone.addEventListener('dragleave', () => {
      dropZone.classList.remove('over');
    });

    dropZone.addEventListener('drop', (e) => {
      e.preventDefault();
      dropZone.classList.remove('over');
      if (e.dataTransfer.files.length > 0) {
        fileInput.files = e.dataTransfer.files;
        submitFile(e.dataTransfer.files[0]);
      }
    });

    fileInput.addEventListener('change', () => {
      if (fileInput.files.length > 0) {
        submitFile(fileInput.files[0]);
      }
    });

    async function submitFile(file) {
      status.textContent = 'Uploading…';
      status.className = 'status';

      const formData = new FormData();
      formData.append('file', file);

      try {
        const resp = await fetch('/local/upload', {
          method: 'POST',
          body: formData,
        });
        if (resp.ok) {
          window.location.href = await resp.text();
        } else {
          const text = await resp.text();
          status.textContent = 'Error: ' + text;
          status.className = 'status error';
        }
      } catch (err) {
        status.textContent = 'Error: ' + err.message;
        status.className = 'status error';
      }
    }
  </script>
</body>
</html>"#,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
