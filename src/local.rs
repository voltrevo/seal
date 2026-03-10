use seal::state::{AppState, LocalApp};
use crate::url::local_app_host;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::io::Read;
use tiny_keccak::{Hasher, Keccak};

/// Handle zip file upload: hash it, extract it, register it, return the app URL.
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
    let hash_id = content_hash(&zip_bytes);

    // Extract zip to site directory
    let site_dir = state.site_dir(&hash_id);
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
        hash: hash_id.clone(),
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

    let host = local_app_host(&hash_id);
    let url = format!("https://{host}/");
    url.into_response()
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

/// Compute keccak256, truncate to 192 bits, return base36-encoded string (~38 chars).
/// Base36 is DNS-safe (case-insensitive, alphanumeric only).
fn content_hash(data: &[u8]) -> String {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut full = [0u8; 32];
    hasher.finalize(&mut full);
    base36_encode(&full[..24])
}

/// Encode bytes as a base36 string (0-9, a-z). DNS-safe and case-insensitive.
pub(crate) fn base36_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    if bytes.iter().all(|&b| b == 0) {
        return "0".to_string();
    }

    // Work on a mutable copy, repeatedly divide by 36
    let mut num = bytes.to_vec();
    let mut digits = Vec::new();

    while num.iter().any(|&b| b != 0) {
        let mut remainder: u16 = 0;
        for byte in num.iter_mut() {
            let val = (remainder << 8) | (*byte as u16);
            *byte = (val / 36) as u8;
            remainder = val % 36;
        }
        digits.push(ALPHABET[remainder as usize]);
    }

    digits.reverse();
    String::from_utf8(digits).unwrap()
}

/// Validate bundle format: all entries must be under `<app>/content/`.
/// Returns the app wrapper dir name (e.g. "my-app" from "my-app/content/index.html").
fn validate_bundle(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> anyhow::Result<String> {
    let mut wrapper: Option<String> = None;

    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let mut components = path.components();
        let first = components
            .next()
            .and_then(|c| c.as_os_str().to_str().map(String::from))
            .ok_or_else(|| anyhow::anyhow!("invalid bundle: entry with no path components"))?;

        // Every entry must start with the same wrapper dir
        match &wrapper {
            None => wrapper = Some(first.clone()),
            Some(w) if *w != first => {
                anyhow::bail!(
                    "invalid bundle: multiple top-level entries ({w}/ and {first}/). \
                     Expected a single wrapper directory."
                );
            }
            _ => {}
        }

        // Non-directory entries must be under <wrapper>/content/
        if !entry.is_dir() {
            let second = components.next().and_then(|c| c.as_os_str().to_str().map(String::from));
            if second.as_deref() != Some("content") {
                anyhow::bail!(
                    "invalid bundle: file \"{}\" is not under {}/content/",
                    path.display(),
                    first,
                );
            }
        }
    }

    wrapper.ok_or_else(|| anyhow::anyhow!("invalid bundle: zip is empty"))
}

/// Extract the `<wrapper>/content/` subtree from a bundle zip into `dest`.
fn extract_zip(zip_bytes: &[u8], dest: &std::path::Path) -> anyhow::Result<()> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    let wrapper = validate_bundle(&mut archive)?;
    let content_prefix = format!("{wrapper}/content/");

    std::fs::create_dir_all(dest)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(enclosed_name) = file.enclosed_name() else {
            continue;
        };
        let name_str = enclosed_name.to_string_lossy();

        // Only extract entries under <wrapper>/content/
        let Some(rel) = name_str.strip_prefix(&content_prefix) else {
            continue;
        };
        if rel.is_empty() {
            continue; // skip the content/ dir entry itself
        }

        let out_path = dest.join(rel);
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
