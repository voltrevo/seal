<picture>
  <img src="assets/banner.avif" alt="Seal" width="100%">
</picture>

# Seal

Seal protects users from unverifiable web frontends. It runs a local daemon that serves verified web apps over HTTPS on the `.seal` TLD.

Open-source web apps publish their frontend as a content-addressed bundle. Users run the Seal daemon, which fetches, verifies, caches, and serves these bundles at `https://<app>.seal/` — fully local, fully HTTPS, no remote server involved in page loads.

## Status

EXPERIMENTAL

## Quick Start

```bash
# Build
cargo build --release

# Install (generates CA certs, configures DNS and trust store)
sudo target/release/seal install

# Start the daemon (enables on boot via systemd/launchd)
sudo target/release/seal start

# Visit the dashboard
open https://home.seal/
```

## How It Works

### TLS

On first install, Seal generates a local CA chain:

1. **Root CA** — added to your system trust store, then the private key is **permanently deleted**
2. **Intermediate CA** — constrained to `*.seal` only via X.509 Name Constraints
3. **Leaf certs** — issued on demand per hostname, signed by the intermediate CA

Since the root key is destroyed, a compromise of the daemon cannot produce certificates for non-`.seal` domains.

### DNS

Seal configures your system to resolve `*.seal` to `127.0.0.1`:

- **Linux with dnsmasq** (preferred): writes `/etc/dnsmasq.d/seal-tld.conf`
- **Linux with systemd-resolved**: writes a drop-in config and runs an embedded DNS server on port 53
- **macOS**: writes `/etc/resolver/seal`

### Local Apps

Drop a `.zip` file onto `https://home.seal/local` to serve it locally. The daemon computes the keccak256 hash and serves it at `https://<hash>--keccak.seal/`. No registration needed — apps are identified purely by content hash.

### URL Transform

Regular URLs map to `.seal` URLs by encoding the TLD boundary with `--`:

```
https://example.com/app       → https://example--com.seal/app
https://sub.example.com/      → https://sub.example--com.seal/
https://example.co.uk/app     → https://example--co--uk.seal/app
```

Subdomain dots are preserved for same-origin compatibility. See [docs/url-transform.md](docs/url-transform.md) for the full spec.

## Commands

```
seal install      Generate CA certs, configure DNS and trust store (requires sudo)
seal start        Start daemon and enable on boot (requires sudo)
seal run          Run daemon in the foreground
seal stop         Stop the daemon (requires sudo)
seal status       Check if the daemon is running
seal reinstall    Regenerate certs/DNS/trust store, restart if running (requires sudo)
seal uninstall    Remove all seal state from the system (requires sudo)
```

## Data Directory

```
~/.local/share/seal-tld/
├── ca/               # intermediate CA key + cert, root cert (no root key)
├── bundles/          # raw bundle files (named by keccak256 hash)
├── sites/            # extracted content served to the browser
├── state/            # per-app metadata JSON
└── daemon.log        # rotating log (10MB per file, 3 old files kept)
```

## Architecture

See [docs/seal.md](docs/seal.md) for the full spec including the on-chain registry, browser extension, manifest format, and security model.
