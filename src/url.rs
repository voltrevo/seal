const SEAL_TLD: &str = "seal";
const KECCAK_SUFFIX: &str = "keccak";

/// Encode a regular hostname into a .seal hostname.
///
/// Example: "sub.example.com" -> "sub.example--com.seal"
pub fn encode_host(hostname: &str) -> Option<String> {
    let domain = psl::domain(hostname.as_bytes())?;
    let registrable = std::str::from_utf8(domain.as_bytes()).ok()?;

    let prefix = if hostname.len() > registrable.len() {
        Some(&hostname[..hostname.len() - registrable.len() - 1])
    } else {
        None
    };

    // Split the registrable domain on dots, escape each label's dashes, rejoin with "--".
    // e.g. "example.co.uk" → ["example", "co", "uk"] → "example--co--uk"
    let last_label = registrable
        .split('.')
        .map(|part| escape_dashes(part))
        .collect::<Vec<_>>()
        .join("--");

    let seal_host = match prefix {
        Some(p) => format!("{p}.{last_label}.{SEAL_TLD}"),
        None => format!("{last_label}.{SEAL_TLD}"),
    };

    Some(seal_host)
}

/// Decode a .seal hostname back to the original hostname.
///
/// Example: "sub.example--com.seal" -> "sub.example.com"
/// Returns None for local keccak apps (use `parse_local_app` instead).
pub fn decode_host(seal_hostname: &str) -> Option<String> {
    let without_tld = seal_hostname.strip_suffix(&format!(".{SEAL_TLD}"))?;

    // Check if this is a local keccak app
    if is_local_app(seal_hostname) {
        return None;
    }

    let (prefix, last_label) = match without_tld.rfind('.') {
        Some(pos) => (Some(&without_tld[..pos]), &without_tld[pos + 1..]),
        None => (None, without_tld),
    };

    let decoded = decode_dashes(last_label);

    let original = match prefix {
        Some(p) => format!("{p}.{decoded}"),
        None => decoded,
    };

    Some(original)
}

/// Check if a .seal hostname is a local keccak app (e.g. "<hash>--keccak.seal").
pub fn is_local_app(seal_hostname: &str) -> bool {
    let without_tld = match seal_hostname.strip_suffix(&format!(".{SEAL_TLD}")) {
        Some(s) => s,
        None => return false,
    };
    // Local apps have no subdomain dots — the whole thing before .seal is "<hash>--keccak"
    !without_tld.contains('.') && without_tld.ends_with(&format!("--{KECCAK_SUFFIX}"))
}

/// Parse a local app hostname, returning the hex hash if valid.
pub fn parse_local_app(seal_hostname: &str) -> Option<String> {
    let without_tld = seal_hostname.strip_suffix(&format!(".{SEAL_TLD}"))?;
    let hash = without_tld.strip_suffix(&format!("--{KECCAK_SUFFIX}"))?;
    if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hash.to_string())
    } else {
        None
    }
}

/// Build a local app hostname from a hex hash.
pub fn local_app_host(hash_hex: &str) -> String {
    format!("{hash_hex}--{KECCAK_SUFFIX}.{SEAL_TLD}")
}

/// Check if a hostname is the home.seal UI.
pub fn is_home(seal_hostname: &str) -> bool {
    seal_hostname == format!("home.{SEAL_TLD}") || seal_hostname == format!("home.{SEAL_TLD}.")
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

/// Decode runs of dashes: N=2 becomes '.', N>2 becomes N-1 dashes.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_basic() {
        assert_eq!(encode_host("example.com").unwrap(), "example--com.seal");
        assert_eq!(
            encode_host("sub.example.com").unwrap(),
            "sub.example--com.seal"
        );
        assert_eq!(
            encode_host("mail.google.com").unwrap(),
            "mail.google--com.seal"
        );
    }

    #[test]
    fn test_encode_compound_tld() {
        assert_eq!(
            encode_host("example.co.uk").unwrap(),
            "example--co--uk.seal"
        );
    }

    #[test]
    fn test_encode_dashes() {
        assert_eq!(
            encode_host("weird--com.com").unwrap(),
            "weird---com--com.seal"
        );
        assert_eq!(
            encode_host("a---b.org").unwrap(),
            "a----b--org.seal"
        );
    }

    #[test]
    fn test_decode_basic() {
        assert_eq!(decode_host("example--com.seal").unwrap(), "example.com");
        assert_eq!(
            decode_host("sub.example--com.seal").unwrap(),
            "sub.example.com"
        );
    }

    #[test]
    fn test_decode_dashes() {
        assert_eq!(
            decode_host("weird---com--com.seal").unwrap(),
            "weird--com.com"
        );
        assert_eq!(decode_host("a----b--org.seal").unwrap(), "a---b.org");
    }

    #[test]
    fn test_roundtrip() {
        let hosts = [
            "example.com",
            "sub.example.com",
            "a.b.example.org",
            "weird--com.com",
            "example.co.uk",
        ];
        for host in hosts {
            let encoded = encode_host(host).unwrap();
            let decoded = decode_host(&encoded).unwrap();
            assert_eq!(decoded, host, "roundtrip failed for {host}");
        }
    }

    #[test]
    fn test_local_app() {
        assert!(is_local_app("abc123--keccak.seal"));
        assert!(!is_local_app("example--com.seal"));
        assert!(!is_local_app("sub.abc123--keccak.seal"));
        assert_eq!(
            parse_local_app("abc123--keccak.seal").unwrap(),
            "abc123"
        );
        assert!(parse_local_app("notahash!--keccak.seal").is_none());
    }

    #[test]
    fn test_home() {
        assert!(is_home("home.seal"));
        assert!(!is_home("example--com.seal"));
    }
}
