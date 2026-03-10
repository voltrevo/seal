# Seal

Actual repo: https://github.com/voltrevo/seal
(./copy-of-repo also provided)

## Team
- Andrew Morris (voltrevo)

## Video

https://drive.google.com/file/d/1II_BRJEgMWlXWJnarePmH_4AR0qfI1pl/view?usp=sharing

[![Recording](thumbnail.avif)](https://drive.google.com/file/d/1II_BRJEgMWlXWJnarePmH_4AR0qfI1pl/view?usp=sharing)

## Problem
Web frontends are unverifiable. When you visit a web app, you trust the server to send the same code the developer published. The server could be compromised, the CDN could inject scripts, or the operator could silently push a malicious update. Users of DeFi, wallets, and other security-critical web apps have no way to confirm they're running the code they think they are.

## Solution
Seal is a local daemon that serves verified web apps over HTTPS on the `.seal` TLD (prototype solution perhaps better solved with web3 browser or new web standards). Developers publish their frontend as a content-addressed bundle and register it on an Ethereum mainnet smart contract ([SealRegistry](https://etherscan.io/address/0x0377Ef2b30CA1E93D54de6576CFb8E133663AD9E)). The daemon fetches bundles, verifies the keccak256 hash against the on-chain record, and serves the app locally — no remote server involved in page loads.

**Components built:**
- **Rust daemon** — local HTTPS server with auto-generated CA (root key deleted after install), DNS configuration, SNI-based cert issuance for `*.seal` domains
- **On-chain registry** — Solidity contract deployed on Ethereum mainnet, tracking versions, bundle hashes, and security advisories
- **Browser extension** (MV3) — detects Seal-enabled sites via `<meta>` tags, offers redirect to the local verified copy, with 3-way redirect settings (no / tentative / always)
- **Publishing toolchain** — deterministic bundle builder (store-only zip + brotli), on-chain publish script
- **Two demo apps** — calculator and pomodoro timer, published on-chain and served via GitHub Pages as bundle sources

## Demo
- Live bundle sources: `https://voltrevo.github.io/seal/calculator/` and `https://voltrevo.github.io/seal/pomodoro/`
- On-chain registry: [0x0377Ef2b30CA1E93D54de6576CFb8E133663AD9E](https://etherscan.io/address/0x0377Ef2b30CA1E93D54de6576CFb8E133663AD9E) (verified on Etherscan + Sourcify)
- After install, visit `https://home.seal/` to manage apps, or navigate to `https://voltrevo.github--io.seal/seal/calculator/` to trigger auto-install from chain

## How to Run
```bash
# Build
cargo build --release

# Install (generates CA certs, configures DNS, installs trust store)
sudo target/release/seal install

# Start the daemon
sudo target/release/seal start

# Visit the dashboard
open https://home.seal/

# Navigate to a registered app (auto-discovers, fetches, verifies, serves)
open https://voltrevo.github--io.seal/seal/calculator/
```

Optionally load the browser extension from `extension/` in Chrome (developer mode, "Load unpacked") to get automatic detection on web pages that advertise Seal support.

## Impact
- Eliminates supply-chain attacks on web frontends — users verify what they run, every time
- Zero-trust page loads: no CDN, no server, no MITM can alter the served code
- On-chain version history provides tamper-proof audit trail with timestamps
- Security advisories let developers flag compromised versions; daemon blocks serving until user acknowledges

## What's Next

1. Sample app repo with automated publishing (private key in github secret)
2. Implement upgrade enforcement
3. Implement insecureMessage
4. Implement source code verification
5. Self-contained browser fork so you don't have to install extension and daemon and mess with trust store
