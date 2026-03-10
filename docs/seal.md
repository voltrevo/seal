# Seal

Seal protects users from unverifiable web frontends. Open-source web apps can publish their frontend as a `.zip.br` bundle; users run a local daemon that serves a cached, verified copy under the `.seal` TLD.

## Components

1. **Manifest** — a JSON file at `.seal/manifest.json` linking the web location to the on-chain registry
2. **Browser extension** — detects `<meta name="seal-tld-manifest">` tags, shows a badge, and redirects to the local daemon
3. **Local daemon** — fetches, verifies, caches, and serves bundles over HTTPS on `*.seal`
4. **On-chain registry** — Ethereum mainnet contract tracking versions, publication timestamps, and security advisories

## URL Transform

Regular URLs map to `.seal` URLs by encoding the TLD boundary with `--`. Subdomain dots are preserved for same-origin compatibility. See [url-transform.md](url-transform.md) for the full encoding spec and Rust implementation.

```
https://example.com/app       → https://example--com.seal/app
https://sub.example.com/      → https://sub.example--com.seal/
https://user.github.io/proj/  → https://user.github--io.seal/proj/
```

## Manifest

Hosted at `<web-location>/.seal/manifest.json`. The page that advertises Seal support includes a meta tag pointing here — the meta tag can be on any page, but the manifest must be at the web location corresponding to the `seal_url`.

```json
{
  "seal_url": "https://example--com.seal/app",
  "chain_id": 1,
  "registry": "0xSealRegistryContractAddress",
  "owner": "0xOwnerAddress",
  "bundleSources": [
    "https://cdn.example.com/seal-bundles/",
    "https://example.com/app/.seal/bundles/"
  ]
}
```

- **`seal_url`**: full `.seal` URL with `https://` prefix. Daemon verifies it matches the web location the manifest was fetched from.
- **`chain_id`** / **`registry`**: identify the governing contract. Daemon rejects unrecognized registries.
- **`owner`**: Ethereum address. Daemon cross-references with on-chain record.
- **`bundleSources`**: array of base URLs serving `<contentHash>.zip.br`. Content hash is the keccak256 truncated to 192 bits, base36 encoded (~38 lowercase alphanumeric chars) — the same encoding used for `<hash>--keccak.seal` local app URLs. Can be empty — the on-chain `VersionRecord` also has this field. Daemon tries all sources (manifest + on-chain).

Version, bundle hash, and integrity are on-chain only — the manifest does not duplicate them.

## On-Chain Registry

### Identifiers

- **sealUrl**: human-readable `.seal` URL (e.g. `https://example--com.seal/app`)
- **sealId**: `keccak256(sealUrl)` — on-chain map key

### Data Model

```
(owner, sealId) → AppRecord { sealUrl, name, keepAlive, versionKey }
```

Multiple owners can claim the same sealId. The daemon only trusts the one matching the manifest at the web location — imposters are ignored.

```
versionKey (bytes32) → VersionRecord
```

Version key = `keccak256(abi.encode(owner, bundleHash))`.

```solidity
struct VersionRecord {
    address owner;
    string version;             // semver
    bytes32 bundleHash;         // keccak256 of the bundle
    uint256 bundleFormat;       // 1 = zip.br
    string[] bundleSources;     // base URLs serving <contentHash>.zip.br
    uint256 publishedAt;        // block.timestamp — used for update delay
    string insecureMessage;     // empty = safe
    bytes32 previousVersionKey; // linked list for version history
}
```

Versions belong to the publishing owner and are not transferable.

### Functions

- **`publish(sealUrl, name, version, bundleHash, bundleFormat, bundleSources, previousVersionKey)`** — creates app on first call. `previousVersionKey` must match current (prevents races). `bundleFormat` is `uint256` (1 = zip.br).
- **`keepAlive(sealUrl)`** — refresh annually
- **`markInsecure(versionKey, message)`** / **`clearInsecure(versionKey)`** — owner only
- **`updateBundleSources(versionKey, bundleSources)`** — owner only
- **`allowNewOwner(sealUrl, newOwner)`** — tells daemon not to warn on ownership change

### KeepAlive & Ownership

If keepAlive is active and the daemon detects a different owner in the manifest than on-chain:
- If `allowNewOwner` was called → accept silently
- Otherwise → hard warning, user must acknowledge

If keepAlive has expired → daemon silently accepts the new owner.

## Content Hash

Bundle filenames and local app URLs use the same content hash encoding:

1. Compute keccak256 of the zip file (32 bytes)
2. Truncate to 192 bits (24 bytes)
3. Base36 encode (0-9, a-z) → ~38 lowercase alphanumeric chars

This encoding is DNS-safe (case-insensitive, no special characters) and fits within the 63-char DNS label limit (38 + `--keccak` = 48 chars).

Bundle sources serve files named `<contentHash>.zip.br`. The daemon derives the content hash from the on-chain `bundleHash` (full 256-bit keccak256) by truncating and base36-encoding.

## Daemon

### DNS

Configures the system resolver to forward `*.seal` to `127.0.0.1` (dnsmasq on Linux, `/etc/resolver/` on macOS).

### HTTPS

On first run:
1. Generate a root CA keypair and certificate
2. Use the root CA to sign an intermediate CA constrained to `*.seal` only (Name Constraints extension)
3. Add the root CA certificate to the system trust store
4. **Delete the root CA private key** — it is never retained
5. Store only the intermediate CA (keypair + cert) at `~/.local/share/seal-tld/ca/`

The intermediate CA issues leaf certs for `*.seal` sites. Since the root key is deleted, a compromise of the daemon's key material cannot produce certificates for non-`.seal` domains.

### Storage

```
~/.local/share/seal-tld/
├── bundles/          # raw bundle files (named by content hash)
├── sites/            # extracted content
└── state/            # per-site metadata JSON
```

### home.seal

The daemon serves its own UI at `https://home.seal/`:
- **`/`** — shows known apps with an "Add zipped app" link
- **`/install`** — install flow for registered apps (by sealUrl or manifest URL)
- **`/local`** — drop a `.zip` file to add a local/unregistered app (see below)
- **`/advisory`** — security advisory interstitial

For per-app daemon UI (e.g. during registration), the daemon redirects to `https://home.seal/install?seal_url=...&return=...` rather than using reserved paths on the app's domain.

### Local apps

For prototyping or running unregistered apps, users can drop a `.zip` file onto `https://home.seal/local`. The daemon:
1. Computes a 192-bit keccak256 hash of the zip file (base36-encoded)
2. Extracts and serves it at `https://<hash>--keccak.seal/`

These apps have no on-chain record, no update mechanism, no ownership, and no security advisories. They are purely local — identified only by their content hash. The `--keccak` suffix in the hostname distinguishes them from regular seal apps (which encode a domain TLD).

This is useful for:
- Testing a local build before publishing
- Running a one-off tool without registering it
- Sharing a bundle by hash (recipient drops the same zip, gets the same URL)

### Serving

- Requests to `*.seal` (other than `home.seal`) are served from extracted bundles
- Path mapping uses the sealUrl's path component as base path
- File lookup: exact match → directory index → 404
- Content-Type from file extension, `X-Content-Type-Options: nosniff`

### Installation

**Via extension**: extension detects `<meta name="seal-tld-manifest">`, sends manifest URL to daemon. Daemon knows the exact sealUrl from the manifest.

**Via direct navigation**: user types a `.seal` URL. If no known app matches, the daemon tries progressively shorter paths to find a manifest. E.g. for `https://example--com.seal/app/dashboard`:
1. Try `https://example.com/app/dashboard/.seal/manifest.json`
2. Try `https://example.com/app/.seal/manifest.json` ← found, sealUrl is `https://example--com.seal/app`
3. Would try `https://example.com/.seal/manifest.json` if above failed

Once the manifest is found:
1. Redirect to `https://home.seal/install?seal_url=...&return=...`
2. Verify `seal_url` matches web location
3. Verify `chain_id` + `registry` are in daemon's accepted list
4. Query contract for `(owner, sealId)`, get latest `VersionRecord`
5. Fetch bundle from `bundleSources` (manifest + on-chain), verify keccak256 matches `bundleHash`
6. Check ownership consistency (keepAlive / allowNewOwner)
7. Extract bundle, write state, redirect to app

### Updates

- Daemon polls contract for new versions (active polling)
- Manifest changes (e.g. ownership, bundleSources) are only re-checked when the user visits the original web source again — the daemon does not actively poll the manifest
- `publishedAt` provides trustless, globally consistent timestamps
- Default 7-day delay from `publishedAt` before activation (user-configurable)
- Previous version kept for one rollback

### Security Advisories

When the daemon detects a non-empty `insecureMessage` on the installed version (via contract polling), it redirects to `https://home.seal/advisory?seal_url=...&return=...` instead of serving the app. The advisory page shows the message and offers:
- **Upgrade now** — bypass delay, install latest safe version
- **Remind me later** — dismiss for session, redirect to app
- **Proceed anyway** — dismiss permanently for this version, redirect to app

Seal never blocks access — but it won't serve flagged content without the user seeing the advisory first.

### Daemon Config

```json
{
  "accepted_registries": [
    { "chain_id": 1, "address": "0x...", "rpc": "https://eth.public-rpc.com" }
  ]
}
```

Apps specifying an unrecognized registry are rejected.

## Browser Extension

- Manifest V3, minimal vanilla JS (must be auditable)
- Scans DOM for `<meta name="seal-tld-manifest">` — no network requests unless found
- Shows badge when Seal alternative is available
- User clicks badge to redirect (or auto-redirect if configured)
- Sends manifest URL to daemon's local API

## Future Extensions

- **NFT domains**: bare `myapp.seal` names governed by ERC-721
- **Independent attestors**: third-party security/reputation attestations users can opt into
- **Private RPC**: privacy-preserving chain access
- **Reproducible build verification**: daemon builds from source or checks attestations
- **CSP / API policy enforcement**: daemon-injected Content Security Policy, static analysis for disallowed APIs
- **SPA mode**: manifest opt-in for single-page app routing (unmatched paths fall back to `index.html`)
- **App explorer**: browsable directory of registered apps at `home.seal`
- **Additional bundle formats**: new `bundleFormat` values beyond 1 (zip.br)