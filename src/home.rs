use crate::local::handle_upload;
use crate::state::AppState;
use crate::url::local_app_host;
use axum::extract::State;
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index_page))
        .route("/local", get(local_page))
        .route("/local/upload", post(handle_upload))
}

async fn index_page(State(state): State<AppState>) -> Html<String> {
    let apps = state.list_local_apps().await;

    let app_list = if apps.is_empty() {
        r#"<p class="empty">No apps installed yet.</p>"#.to_string()
    } else {
        let items: Vec<String> = apps
            .iter()
            .map(|app| {
                let host = local_app_host(&app.hash);
                format!(
                    r#"<li><a href="https://{host}/">{name}</a> <span class="hash">{short_hash}…</span></li>"#,
                    name = html_escape(&app.name),
                    short_hash = &app.hash[..12],
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
  nav a {{ color: #2563eb; text-decoration: none; padding: 0.5rem 1rem; border: 1px solid #2563eb; border-radius: 6px; }}
  nav a:hover {{ background: #2563eb; color: white; }}
  ul {{ list-style: none; }}
  li {{ padding: 0.75rem 0; border-bottom: 1px solid #eee; }}
  li a {{ color: #2563eb; text-decoration: none; font-weight: 500; }}
  li a:hover {{ text-decoration: underline; }}
  .hash {{ color: #999; font-family: monospace; font-size: 0.85rem; margin-left: 0.5rem; }}
  .empty {{ color: #999; font-style: italic; }}
</style>
</head>
<body>
  <h1><span>🦭</span> Seal</h1>
  <p class="subtitle">Secure frontends</p>
  <nav>
    <a href="/local">Install local app</a>
  </nav>
  <h2>Installed Apps</h2>
  {app_list}
</body>
</html>"#
    ))
}

async fn local_page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Install Local App — Seal</title>
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
  <h1>Install Local App</h1>
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
      status.textContent = 'Uploading and installing...';
      status.className = 'status';

      const formData = new FormData();
      formData.append('file', file);

      try {
        const resp = await fetch('/local/upload', {
          method: 'POST',
          body: formData,
          redirect: 'follow',
        });
        if (resp.redirected) {
          window.location.href = resp.url;
        } else if (!resp.ok) {
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
