use crate::state::{AppState, LocalApp};
use crate::url::local_app_host;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use std::io::Read;
use tiny_keccak::{Hasher, Keccak};

/// Handle zip file upload: hash it, extract it, register it, redirect to the app.
pub async fn handle_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Response {
    // Read the uploaded zip file
    let (zip_bytes, filename) = match read_upload(&mut multipart).await {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("Upload error: {e}")).into_response();
        }
    };

    // Compute keccak256 hash
    let hash_hex = keccak256_hex(&zip_bytes);

    // Extract zip to site directory
    let site_dir = state.site_dir(&hash_hex);
    if !site_dir.exists() {
        if let Err(e) = extract_zip(&zip_bytes, &site_dir) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to extract zip: {e}"),
            )
                .into_response();
        }
    }

    // Derive a name from the filename
    let name = filename
        .strip_suffix(".zip")
        .unwrap_or(&filename)
        .to_string();

    // Register in state
    let app = LocalApp {
        hash: hash_hex.clone(),
        name,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    if let Err(e) = state.register_local_app(app).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save app state: {e}"),
        )
            .into_response();
    }

    let host = local_app_host(&hash_hex);
    Redirect::to(&format!("https://{host}/")).into_response()
}

async fn read_upload(multipart: &mut Multipart) -> anyhow::Result<(Vec<u8>, String)> {
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let filename = field
                .file_name()
                .unwrap_or("app.zip")
                .to_string();
            let data = field.bytes().await?;
            return Ok((data.to_vec(), filename));
        }
    }
    anyhow::bail!("no file field in upload")
}

fn keccak256_hex(data: &[u8]) -> String {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut output = [0u8; 32];
    hasher.finalize(&mut output);
    hex::encode(output)
}

fn extract_zip(zip_bytes: &[u8], dest: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;

    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(enclosed_name) = file.enclosed_name() else {
            continue; // skip entries with unsafe paths
        };
        let out_path = dest.join(enclosed_name);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut file, &mut out_file)?;
        }
    }

    Ok(())
}
