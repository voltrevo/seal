/// On-chain registry integration: discover manifests, query SealRegistry, fetch and verify bundles.

use seal::state::{AppState, RegisteredApp};
use serde::Deserialize;
use std::fmt::Write as FmtWrite;

const ACCEPTED_REGISTRIES: &[(&str, u64)] = &[
    ("0x0377Ef2b30CA1E93D54de6576CFb8E133663AD9E", 1),
];
const DEFAULT_RPC: &str = "https://ethereum-rpc.publicnode.com";

#[derive(Deserialize)]
pub struct SealManifest {
    pub seal_url: String,
    pub chain_id: u64,
    pub registry: String,
    pub owner: String,
    #[serde(default, rename = "bundleSources")]
    pub bundle_sources: Vec<String>,
}

struct AppInfo {
    name: String,
    #[allow(dead_code)]
    keep_alive: u64,
    version_key: [u8; 32],
}

struct VersionInfo {
    #[allow(dead_code)]
    owner: [u8; 20],
    version: String,
    bundle_hash: [u8; 32],
    bundle_format: u64,
    bundle_sources: Vec<String>,
    #[allow(dead_code)]
    published_at: u64,
    #[allow(dead_code)]
    insecure_message: String,
    #[allow(dead_code)]
    previous_version_key: [u8; 32],
}

/// Install a registered app: discover manifest, query chain, fetch bundle, verify, extract.
/// Returns the redirect URL on success.
pub async fn install_app(state: &AppState, url: &str) -> anyhow::Result<String> {
    let url = url.trim_end_matches('/');
    let without_scheme = url
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("URL must start with https://"))?;

    let (hostname, path) = match without_scheme.find('/') {
        Some(pos) => (&without_scheme[..pos], &without_scheme[pos..]),
        None => (without_scheme, "/"),
    };

    // Discover manifest via progressive path search
    let manifest = discover_manifest(hostname, path).await?;

    // Validate registry is accepted
    let registry_accepted = ACCEPTED_REGISTRIES
        .iter()
        .any(|(addr, chain)| addr.eq_ignore_ascii_case(&manifest.registry) && *chain == manifest.chain_id);
    if !registry_accepted {
        anyhow::bail!(
            "unrecognized registry: {} on chain {}",
            manifest.registry,
            manifest.chain_id
        );
    }

    // Parse seal_url to get hostname and base_path
    let seal_url = &manifest.seal_url;
    let seal_without_scheme = seal_url
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("seal_url must start with https://"))?;
    let (seal_hostname, base_path) = match seal_without_scheme.find('/') {
        Some(pos) => (
            seal_without_scheme[..pos].to_string(),
            seal_without_scheme[pos..].trim_end_matches('/').to_string(),
        ),
        None => (seal_without_scheme.to_string(), String::new()),
    };

    // Parse owner address
    let owner = parse_address(&manifest.owner)?;

    // Query on-chain
    let app_info = get_app(DEFAULT_RPC, &manifest.registry, &owner, seal_url).await?;

    let zero_key = [0u8; 32];
    if app_info.version_key == zero_key {
        anyhow::bail!("no version published for this app");
    }
    let version_info =
        get_version(DEFAULT_RPC, &manifest.registry, &app_info.version_key).await?;

    if version_info.bundle_format != 1 {
        anyhow::bail!("unsupported bundle format: {}", version_info.bundle_format);
    }

    // Compute content hash from on-chain bundle hash
    let content_hash = crate::local::base36_encode(&version_info.bundle_hash[..24]);

    // Collect all bundle sources (manifest + on-chain)
    let mut sources = manifest.bundle_sources.clone();
    for s in &version_info.bundle_sources {
        if !sources.contains(s) {
            sources.push(s.clone());
        }
    }
    if sources.is_empty() {
        anyhow::bail!("no bundle sources available");
    }

    // Fetch, decompress, verify, and extract bundle
    let bundle_filename = format!("{content_hash}.zip.br");
    let zip_bytes =
        fetch_and_verify_bundle(&sources, &bundle_filename, &version_info.bundle_hash).await?;

    let site_dir = state.site_dir(&content_hash);
    if !site_dir.exists() {
        extract_flat_zip(&zip_bytes, &site_dir)?;
    }

    // Register app in state
    let app = RegisteredApp {
        seal_url: seal_url.clone(),
        hostname: seal_hostname,
        base_path: base_path.clone(),
        name: app_info.name,
        owner: manifest.owner,
        bundle_hash: hex_encode(&version_info.bundle_hash),
        content_hash,
        version: version_info.version,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    state.register_app(app).await?;

    // Return redirect URL
    Ok(format!("https://{hostname}{path}"))
}

/// URL-encode a string for use in query parameters.
pub fn percent_encode(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                write!(result, "%{:02X}", b).unwrap();
            }
        }
    }
    result
}


// --- Manifest discovery ---

async fn discover_manifest(
    seal_hostname: &str,
    request_path: &str,
) -> anyhow::Result<SealManifest> {
    let web_host = crate::url::decode_host(seal_hostname)
        .ok_or_else(|| anyhow::anyhow!("cannot decode .seal hostname: {seal_hostname}"))?;

    // Build progressive list of paths to try
    let mut paths = Vec::new();
    let normalized = request_path.trim_end_matches('/');
    let mut current = normalized.to_string();
    loop {
        paths.push(current.clone());
        match current.rfind('/') {
            Some(0) => {
                paths.push(String::new());
                break;
            }
            Some(pos) => current = current[..pos].to_string(),
            None => break,
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    for base_path in &paths {
        let manifest_url = format!("https://{web_host}{base_path}/.seal/manifest.json");
        tracing::debug!("trying manifest at {manifest_url}");

        let resp = match client.get(&manifest_url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::debug!("manifest {manifest_url}: HTTP {}", r.status());
                continue;
            }
            Err(e) => {
                tracing::debug!("manifest {manifest_url}: {e}");
                continue;
            }
        };

        let text = resp.text().await?;
        match serde_json::from_str::<SealManifest>(&text) {
            Ok(m) => return Ok(m),
            Err(e) => {
                tracing::debug!("manifest parse failed: {e}");
                continue;
            }
        }
    }

    anyhow::bail!("no seal manifest found for {seal_hostname}{request_path}")
}

// --- Bundle fetch & verify ---

async fn fetch_and_verify_bundle(
    sources: &[String],
    filename: &str,
    expected_hash: &[u8; 32],
) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let mut last_error = None;

    for source in sources {
        let base = source.trim_end_matches('/');
        let full_url = format!("{base}/{filename}");
        tracing::info!("fetching bundle from {full_url}");

        let resp = match client.get(&full_url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_error = Some(anyhow::anyhow!("{full_url}: HTTP {}", r.status()));
                continue;
            }
            Err(e) => {
                last_error = Some(anyhow::anyhow!("{full_url}: {e}"));
                continue;
            }
        };

        let compressed = resp.bytes().await?;

        // Brotli decompress
        let mut zip_bytes = Vec::new();
        brotli::BrotliDecompress(&mut compressed.as_ref(), &mut zip_bytes)
            .map_err(|e| anyhow::anyhow!("brotli decompression failed: {e}"))?;

        // Verify keccak256
        let mut hasher = tiny_keccak::Keccak::v256();
        tiny_keccak::Hasher::update(&mut hasher, &zip_bytes);
        let mut actual_hash = [0u8; 32];
        tiny_keccak::Hasher::finalize(hasher, &mut actual_hash);

        if actual_hash != *expected_hash {
            last_error = Some(anyhow::anyhow!(
                "{full_url}: hash mismatch (expected {}, got {})",
                hex_encode(expected_hash),
                hex_encode(&actual_hash),
            ));
            continue;
        }

        tracing::info!("bundle verified ({} bytes)", zip_bytes.len());
        return Ok(zip_bytes);
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no bundle sources")))
}

fn extract_flat_zip(zip_bytes: &[u8], dest: &std::path::Path) -> anyhow::Result<()> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    std::fs::create_dir_all(dest)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(name) = file.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(name);

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

// --- ABI encoding/decoding ---

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = tiny_keccak::Keccak::v256();
    tiny_keccak::Hasher::update(&mut hasher, data);
    let mut out = [0u8; 32];
    tiny_keccak::Hasher::finalize(hasher, &mut out);
    out
}

fn function_selector(sig: &str) -> [u8; 4] {
    let hash = keccak256(sig.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("0x");
    for b in bytes {
        write!(s, "{:02x}", b).unwrap();
    }
    s
}

fn hex_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() % 2 != 0 {
        anyhow::bail!("odd hex length");
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&s[i..i + 2], 16)?);
    }
    Ok(bytes)
}

fn parse_address(s: &str) -> anyhow::Result<[u8; 20]> {
    let bytes = hex_decode(s)?;
    if bytes.len() != 20 {
        anyhow::bail!("address must be 20 bytes, got {}", bytes.len());
    }
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&bytes);
    Ok(addr)
}

fn encode_get_app(owner: &[u8; 20], seal_url: &str) -> Vec<u8> {
    let selector = function_selector("getApp(address,string)");
    let url_bytes = seal_url.as_bytes();

    let mut data = Vec::new();
    data.extend_from_slice(&selector);

    // word 0: address (left-padded to 32)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(owner);

    // word 1: offset to string = 64
    let mut offset_word = [0u8; 32];
    offset_word[31] = 64;
    data.extend_from_slice(&offset_word);

    // string: length
    let mut len_word = [0u8; 32];
    len_word[28..32].copy_from_slice(&(url_bytes.len() as u32).to_be_bytes());
    data.extend_from_slice(&len_word);

    // string: data (padded to 32-byte boundary)
    data.extend_from_slice(url_bytes);
    let padding = (32 - url_bytes.len() % 32) % 32;
    data.extend_from_slice(&vec![0u8; padding]);

    data
}

fn encode_get_version(version_key: &[u8; 32]) -> Vec<u8> {
    let selector = function_selector("getVersion(bytes32)");
    let mut data = Vec::new();
    data.extend_from_slice(&selector);
    data.extend_from_slice(version_key);
    data
}

async fn eth_call(rpc_url: &str, to: &str, calldata: &[u8]) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let hex_data = hex_encode(calldata);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": to, "data": hex_data}, "latest"],
        "id": 1
    });

    let resp: serde_json::Value = client.post(rpc_url).json(&body).send().await?.json().await?;

    if let Some(error) = resp.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("RPC error: {msg}");
    }

    let result = resp
        .get("result")
        .and_then(|r| r.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing result in RPC response"))?;

    hex_decode(result)
}

async fn get_app(
    rpc_url: &str,
    registry: &str,
    owner: &[u8; 20],
    seal_url: &str,
) -> anyhow::Result<AppInfo> {
    let calldata = encode_get_app(owner, seal_url);
    let result = eth_call(rpc_url, registry, &calldata).await?;
    if result.is_empty() {
        anyhow::bail!("app not found on chain");
    }
    decode_get_app(&result)
}

async fn get_version(
    rpc_url: &str,
    registry: &str,
    version_key: &[u8; 32],
) -> anyhow::Result<VersionInfo> {
    let calldata = encode_get_version(version_key);
    let result = eth_call(rpc_url, registry, &calldata).await?;
    if result.is_empty() {
        anyhow::bail!("version not found on chain");
    }
    decode_get_version(&result)
}

// --- ABI decoding helpers ---

fn read_word(data: &[u8], index: usize) -> anyhow::Result<[u8; 32]> {
    let start = index * 32;
    if start + 32 > data.len() {
        anyhow::bail!(
            "ABI data too short (need word {index}, have {} bytes)",
            data.len()
        );
    }
    let mut word = [0u8; 32];
    word.copy_from_slice(&data[start..start + 32]);
    Ok(word)
}

fn word_as_u64(word: &[u8; 32]) -> u64 {
    u64::from_be_bytes(word[24..32].try_into().unwrap())
}

fn read_string_at(data: &[u8], head_index: usize) -> anyhow::Result<String> {
    let offset_word = read_word(data, head_index)?;
    let offset = word_as_u64(&offset_word) as usize;
    read_string_at_offset(data, offset)
}

fn read_string_at_offset(data: &[u8], byte_offset: usize) -> anyhow::Result<String> {
    if byte_offset + 32 > data.len() {
        anyhow::bail!("string offset {byte_offset} out of bounds (data len {})", data.len());
    }
    let len =
        u64::from_be_bytes(data[byte_offset + 24..byte_offset + 32].try_into().unwrap()) as usize;
    let start = byte_offset + 32;
    if start + len > data.len() {
        anyhow::bail!("string data out of bounds");
    }
    Ok(String::from_utf8_lossy(&data[start..start + len]).to_string())
}

fn read_string_array_at(data: &[u8], head_index: usize) -> anyhow::Result<Vec<String>> {
    let offset_word = read_word(data, head_index)?;
    let offset = word_as_u64(&offset_word) as usize;

    if offset + 32 > data.len() {
        anyhow::bail!("string[] offset out of bounds");
    }
    let count =
        u64::from_be_bytes(data[offset + 24..offset + 32].try_into().unwrap()) as usize;

    let array_start = offset + 32;
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let elem_offset_pos = array_start + i * 32;
        if elem_offset_pos + 32 > data.len() {
            anyhow::bail!("string[] element offset out of bounds");
        }
        let elem_offset = u64::from_be_bytes(
            data[elem_offset_pos + 24..elem_offset_pos + 32]
                .try_into()
                .unwrap(),
        ) as usize;

        // String data is at array_start + elem_offset
        let str_offset = array_start + elem_offset;
        result.push(read_string_at_offset(data, str_offset)?);
    }

    Ok(result)
}

fn decode_get_app(data: &[u8]) -> anyhow::Result<AppInfo> {
    // Returns: (string name, uint256 keepAlive, bytes32 versionKey)
    let name = read_string_at(data, 0)?;
    let keep_alive_word = read_word(data, 1)?;
    let version_key = read_word(data, 2)?;

    Ok(AppInfo {
        name,
        keep_alive: word_as_u64(&keep_alive_word),
        version_key,
    })
}

fn decode_get_version(data: &[u8]) -> anyhow::Result<VersionInfo> {
    // Returns: (address, string, bytes32, uint256, string[], uint256, string, bytes32)
    // Head: 8 words — static values inline, dynamic types as offsets
    let owner_word = read_word(data, 0)?;
    let mut owner = [0u8; 20];
    owner.copy_from_slice(&owner_word[12..32]);

    let version = read_string_at(data, 1)?;
    let bundle_hash = read_word(data, 2)?;
    let bundle_format_word = read_word(data, 3)?;
    let bundle_sources = read_string_array_at(data, 4)?;
    let published_at_word = read_word(data, 5)?;
    let insecure_message = read_string_at(data, 6)?;
    let previous_version_key = read_word(data, 7)?;

    Ok(VersionInfo {
        owner,
        version,
        bundle_hash,
        bundle_format: word_as_u64(&bundle_format_word),
        bundle_sources,
        published_at: word_as_u64(&published_at_word),
        insecure_message,
        previous_version_key,
    })
}
