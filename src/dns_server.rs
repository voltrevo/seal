use std::net::Ipv4Addr;
use tokio::net::UdpSocket;

/// Run a minimal DNS server on 127.0.0.1:53 that responds to *.seal with 127.0.0.1.
/// All other queries get NXDOMAIN.
pub async fn run() -> anyhow::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:53").await?;
    tracing::info!("DNS server listening on 127.0.0.1:53");

    let mut buf = [0u8; 512];
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("DNS recv error: {e}");
                continue;
            }
        };

        let query = &buf[..len];
        if let Some(response) = handle_query(query) {
            if let Err(e) = socket.send_to(&response, src).await {
                tracing::debug!("DNS send error: {e}");
            }
        }
    }
}

fn handle_query(query: &[u8]) -> Option<Vec<u8>> {
    // Minimum DNS header is 12 bytes
    if query.len() < 12 {
        return None;
    }

    let id = &query[0..2];
    let qname = parse_qname(query, 12)?;

    // Check if it ends with .seal
    let name_lower = qname.to_ascii_lowercase();
    let is_seal = name_lower == "seal" || name_lower.ends_with(".seal");

    if is_seal {
        Some(build_response(id, query, &qname, Ipv4Addr::LOCALHOST))
    } else {
        Some(build_nxdomain(id, query))
    }
}

/// Parse a DNS name from wire format starting at `offset`.
/// Returns the dotted string (without trailing dot).
fn parse_qname(buf: &[u8], mut offset: usize) -> Option<String> {
    let mut labels = Vec::new();
    loop {
        if offset >= buf.len() {
            return None;
        }
        let label_len = buf[offset] as usize;
        if label_len == 0 {
            break;
        }
        // We don't handle compression pointers (0xC0) — queries shouldn't have them
        if label_len > 63 || offset + 1 + label_len > buf.len() {
            return None;
        }
        let label = std::str::from_utf8(&buf[offset + 1..offset + 1 + label_len]).ok()?;
        labels.push(label.to_string());
        offset += 1 + label_len;
    }
    if labels.is_empty() {
        return None;
    }
    Some(labels.join("."))
}

/// Build an A record response: question echoed back + answer with the given IP.
fn build_response(id: &[u8], query: &[u8], _qname: &str, ip: Ipv4Addr) -> Vec<u8> {
    let mut resp = Vec::with_capacity(query.len() + 16);

    // Header
    resp.extend_from_slice(id); // ID
    resp.extend_from_slice(&[
        0x81, 0x80, // Flags: QR=1, AA=1, RCODE=0 (no error)
        0x00, 0x01, // QDCOUNT = 1
        0x00, 0x01, // ANCOUNT = 1
        0x00, 0x00, // NSCOUNT = 0
        0x00, 0x00, // ARCOUNT = 0
    ]);

    // Copy the question section from the query
    let question_section = &query[12..];
    // Find the end of the question (QNAME null terminator + 4 bytes for QTYPE/QCLASS)
    let qname_end = question_section.iter().position(|&b| b == 0).unwrap_or(0) + 1;
    let question_len = qname_end + 4; // QTYPE (2) + QCLASS (2)
    if question_len > question_section.len() {
        return build_nxdomain(id, query);
    }
    resp.extend_from_slice(&question_section[..question_len]);

    // Answer section: pointer to name in question + A record
    resp.extend_from_slice(&[
        0xC0, 0x0C, // Name pointer to offset 12 (the QNAME)
        0x00, 0x01, // TYPE = A
        0x00, 0x01, // CLASS = IN
        0x00, 0x00, 0x00, 0x3C, // TTL = 60 seconds
        0x00, 0x04, // RDLENGTH = 4
    ]);
    resp.extend_from_slice(&ip.octets());

    resp
}

/// Build an NXDOMAIN response.
fn build_nxdomain(id: &[u8], query: &[u8]) -> Vec<u8> {
    let mut resp = Vec::with_capacity(query.len());

    resp.extend_from_slice(id);
    resp.extend_from_slice(&[
        0x81, 0x83, // Flags: QR=1, AA=1, RCODE=3 (NXDOMAIN)
        0x00, 0x01, // QDCOUNT = 1
        0x00, 0x00, // ANCOUNT = 0
        0x00, 0x00, // NSCOUNT = 0
        0x00, 0x00, // ARCOUNT = 0
    ]);

    // Copy question section
    let question_section = &query[12..];
    let qname_end = question_section.iter().position(|&b| b == 0).unwrap_or(0) + 1;
    let question_len = qname_end + 4;
    if question_len <= question_section.len() {
        resp.extend_from_slice(&question_section[..question_len]);
    }

    resp
}
