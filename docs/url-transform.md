# Seal URL Transform

## Overview

Seal maps regular web URLs to `.seal` URLs served by the local daemon. The transform preserves the browser's same-origin model by keeping subdomain dots intact and only encoding the boundary between the registrable domain and TLD.

## Transform Rule

Given `https://sub.domain.tld/path`:

1. Split the hostname into **subdomain prefix** (everything up to the last dot-separated label group) and **last label** (the registrable domain + TLD, i.e. everything after the last subdomain dot).
2. In the last label, **escape existing dash runs**: any run of N dashes (N≥2) becomes (N+1) dashes.
3. Replace the `.` separating domain from TLD with `--`.
4. Append `.seal`.
5. Path is preserved as-is.

Decoding reverses this: in the last label (before `.seal`), any run of N dashes (N≥2) becomes `.` if N=2, or (N-1) dashes otherwise.

## Examples

| Original | Last label | Encoded last label | Seal URL |
|---|---|---|---|
| `https://example.com/app` | `example.com` | `example--com` | `https://example--com.seal/app` |
| `https://sub.example.com/app` | `example.com` | `example--com` | `https://sub.example--com.seal/app` |
| `https://mail.google.com/inbox` | `google.com` | `google--com` | `https://mail.google--com.seal/inbox` |
| `https://example.co.uk/app` | `co.uk` | `co--uk` | `https://example.co--uk.seal/app` |
| `https://a.b.example.org/page` | `example.org` | `example--org` | `https://a.b.example--org.seal/page` |
| `https://weird--com.com/x` | `weird--com.com` | `weird---com--com` | `https://weird---com--com.seal/x` |
| `https://sub.weird--com.com/x` | `weird--com.com` | `weird---com--com` | `https://sub.weird---com--com.seal/x` |
| `https://a---b.org/x` | `a---b.org` | `a----b--org` | `https://a----b--org.seal/x` |

## Dash Encoding Detail

Within the last label only:

- **Encode**: first escape existing dashes (N≥2 → N+1), then replace the final `.` with `--`.
- **Decode**: scan for runs of N dashes (N≥2). If N=2, replace with `.`. If N>2, replace with (N-1) dashes.

Single dashes are always literal and pass through unchanged.

## Determining the Last Label

The "last label" is the registrable domain + TLD — essentially the eTLD+1 boundary. For common cases (`example.com`) this is obvious, but compound TLDs (`co.uk`, `com.au`, `github.io`) require knowledge of the public suffix list.

Use the `addr` or `psl` crate to identify the eTLD+1 boundary.

## Rust: Encode

```rust
use psl::List;

const SEAL_TLD: &str = "seal";

/// Encode a regular hostname into a .seal hostname.
///
/// Example: "sub.example.com" -> "sub.example--com.seal"
fn encode_host(hostname: &str) -> Option<String> {
    // Use the public suffix list to find the registrable domain boundary.
    let domain = List.domain(hostname.as_bytes())?;
    let suffix = std::str::from_utf8(domain.suffix().as_bytes()).ok()?;
    let registrable = std::str::from_utf8(domain.as_bytes()).ok()?;

    // The registrable domain is e.g. "example.com" or "weird--com.com".
    // Everything before it in the hostname is the subdomain prefix.
    let prefix = if hostname.len() > registrable.len() {
        // +1 to skip the dot between prefix and registrable domain
        Some(&hostname[..hostname.len() - registrable.len() - 1])
    } else {
        None
    };

    // Split registrable domain into name and TLD.
    // e.g. "example.com" -> name="example", tld="com"
    // e.g. "example.co.uk" -> name="example", tld="co.uk"
    let name = &registrable[..registrable.len() - suffix.len() - 1];

    // Escape dash runs in name: N dashes (N>=2) become N+1 dashes.
    let escaped_name = escape_dashes(name);

    // Escape dash runs in suffix too (unlikely but be safe).
    let escaped_suffix = escape_dashes(suffix);

    // Join with -- to encode the dot.
    let last_label = format!("{escaped_name}--{escaped_suffix}");

    let seal_host = match prefix {
        Some(p) => format!("{p}.{last_label}.{SEAL_TLD}"),
        None => format!("{last_label}.{SEAL_TLD}"),
    };

    Some(seal_host)
}

/// Escape runs of dashes: any run of N dashes (N >= 2) becomes N+1 dashes.
fn escape_dashes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '-' {
            let mut count = 1;
            while chars.peek() == Some(&'-') {
                chars.next();
                count += 1;
            }
            if count >= 2 {
                // N dashes -> N+1 dashes
                for _ in 0..count + 1 {
                    result.push('-');
                }
            } else {
                result.push('-');
            }
        } else {
            result.push(c);
        }
    }

    result
}
```

## Rust: Decode

```rust
/// Decode a .seal hostname back to the original hostname.
///
/// Example: "sub.example--com.seal" -> "sub.example.com"
fn decode_host(seal_hostname: &str) -> Option<String> {
    let without_tld = seal_hostname.strip_suffix(&format!(".{SEAL_TLD}"))?;

    // The last label is the final dot-separated component.
    let (prefix, last_label) = match without_tld.rfind('.') {
        Some(pos) => (Some(&without_tld[..pos]), &without_tld[pos + 1..]),
        None => (None, without_tld),
    };

    // Decode dash runs in the last label.
    let decoded = decode_dashes(last_label);

    let original = match prefix {
        Some(p) => format!("{p}.{decoded}"),
        None => decoded,
    };

    Some(original)
}

/// Decode runs of dashes: any run of N dashes (N >= 2) becomes a dot if
/// N == 2, or (N-1) dashes otherwise.
fn decode_dashes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '-' {
            let mut count = 1;
            while chars.peek() == Some(&'-') {
                chars.next();
                count += 1;
            }
            match count {
                1 => result.push('-'),
                2 => result.push('.'),
                n => {
                    for _ in 0..n - 1 {
                        result.push('-');
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
```

## Full URL Transform

```rust
/// Transform a regular URL into a .seal URL.
fn to_seal_url(url: &url::Url) -> Option<url::Url> {
    let host = url.host_str()?;
    let seal_host = encode_host(host)?;

    let mut seal_url = url.clone();
    seal_url.set_host(Some(&seal_host)).ok()?;
    seal_url.set_port(None).ok()?;
    Some(seal_url)
}

/// Recover the original URL from a .seal URL.
fn from_seal_url(seal_url: &url::Url) -> Option<url::Url> {
    let seal_host = seal_url.host_str()?;
    let original_host = decode_host(seal_host)?;

    let mut url = seal_url.clone();
    url.set_host(Some(&original_host)).ok()?;
    Some(url)
}
```