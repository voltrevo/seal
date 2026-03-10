use std::path::Path;

/// How DNS resolution for .seal is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsMethod {
    /// macOS /etc/resolver — points to our embedded DNS server
    MacosResolver,
    /// dnsmasq handles resolution natively (no embedded DNS needed)
    Dnsmasq,
    /// NetworkManager dnsmasq handles resolution natively
    NmDnsmasq,
    /// systemd-resolved forwards to our embedded DNS server
    SystemdResolved,
}

impl DnsMethod {
    /// Whether this method requires the daemon to run an embedded DNS server.
    pub fn needs_embedded_dns(self) -> bool {
        matches!(self, DnsMethod::MacosResolver | DnsMethod::SystemdResolved)
    }
}

/// Configure system DNS to resolve *.seal to 127.0.0.1.
/// Returns the method used, or None if already configured.
pub fn configure() -> anyhow::Result<(DnsMethod, bool)> {
    if cfg!(target_os = "macos") {
        configure_macos()
    } else {
        configure_linux()
    }
}

/// Detect which DNS method is active (for an already-configured system).
pub fn detect_method() -> DnsMethod {
    if cfg!(target_os = "macos") {
        return DnsMethod::MacosResolver;
    }

    if Path::new("/etc/dnsmasq.d/seal-tld.conf").exists() {
        return DnsMethod::Dnsmasq;
    }
    if Path::new("/etc/NetworkManager/dnsmasq.d/seal-tld.conf").exists() {
        return DnsMethod::NmDnsmasq;
    }

    // Default: systemd-resolved (needs embedded DNS)
    DnsMethod::SystemdResolved
}

/// macOS: write /etc/resolver/seal
fn configure_macos() -> anyhow::Result<(DnsMethod, bool)> {
    let resolver_dir = Path::new("/etc/resolver");
    let resolver_file = resolver_dir.join("seal");

    if resolver_file.exists() {
        let content = std::fs::read_to_string(&resolver_file)?;
        if content.contains("nameserver 127.0.0.1") {
            eprintln!("macOS resolver already configured");
            return Ok((DnsMethod::MacosResolver, false));
        }
    }

    eprintln!("configuring macOS resolver at /etc/resolver/seal");
    std::fs::create_dir_all(resolver_dir)?;
    std::fs::write(
        &resolver_file,
        "# Seal TLD - local daemon\nnameserver 127.0.0.1\nport 53\n",
    )?;

    Ok((DnsMethod::MacosResolver, true))
}

/// Linux: prefer dnsmasq, fall back to systemd-resolved (which needs embedded DNS).
fn configure_linux() -> anyhow::Result<(DnsMethod, bool)> {
    // Prefer dnsmasq — it resolves natively without an embedded DNS server
    if Path::new("/etc/dnsmasq.d").exists() {
        let (method, changed) = configure_dnsmasq()?;
        return Ok((method, changed));
    }

    // NetworkManager dnsmasq
    if Path::new("/etc/NetworkManager/dnsmasq.d").exists() {
        let (method, changed) = configure_nm_dnsmasq()?;
        return Ok((method, changed));
    }

    // Fall back to systemd-resolved (requires embedded DNS on 127.0.0.1:53)
    if Path::new("/etc/systemd/resolved.conf.d").exists()
        || Path::new("/etc/systemd/resolved.conf").exists()
    {
        let (method, changed) = configure_systemd_resolved()?;
        return Ok((method, changed));
    }

    anyhow::bail!(
        "could not detect DNS resolver. Install dnsmasq or systemd-resolved, \
         or manually configure *.seal to resolve to 127.0.0.1"
    );
}

fn configure_dnsmasq() -> anyhow::Result<(DnsMethod, bool)> {
    let conf_file = Path::new("/etc/dnsmasq.d/seal-tld.conf");

    if conf_file.exists() {
        eprintln!("dnsmasq already configured");
        return Ok((DnsMethod::Dnsmasq, false));
    }

    eprintln!("configuring dnsmasq for .seal TLD");
    std::fs::write(conf_file, "address=/seal/127.0.0.1\n")?;

    restart_service("dnsmasq")?;
    Ok((DnsMethod::Dnsmasq, true))
}

fn configure_nm_dnsmasq() -> anyhow::Result<(DnsMethod, bool)> {
    let conf_file = Path::new("/etc/NetworkManager/dnsmasq.d/seal-tld.conf");

    if conf_file.exists() {
        eprintln!("NetworkManager dnsmasq already configured");
        return Ok((DnsMethod::NmDnsmasq, false));
    }

    eprintln!("configuring NetworkManager dnsmasq for .seal TLD");
    std::fs::write(conf_file, "address=/seal/127.0.0.1\n")?;

    restart_service("NetworkManager")?;
    Ok((DnsMethod::NmDnsmasq, true))
}

fn configure_systemd_resolved() -> anyhow::Result<(DnsMethod, bool)> {
    let conf_dir = Path::new("/etc/systemd/resolved.conf.d");
    let conf_file = conf_dir.join("seal-tld.conf");

    if conf_file.exists() {
        eprintln!("systemd-resolved already configured");
        return Ok((DnsMethod::SystemdResolved, false));
    }

    eprintln!("configuring systemd-resolved for .seal TLD");
    std::fs::create_dir_all(conf_dir)?;
    std::fs::write(
        &conf_file,
        "[Resolve]\nDNS=127.0.0.1\nDomains=~seal\n",
    )?;

    restart_service("systemd-resolved")?;
    Ok((DnsMethod::SystemdResolved, true))
}

fn restart_service(name: &str) -> anyhow::Result<()> {
    eprintln!("restarting {name}...");
    let status = std::process::Command::new("systemctl")
        .args(["restart", name])
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl restart {name} failed (exit {status})");
    }
    Ok(())
}

/// Remove DNS configuration created by `configure()`.
pub fn unconfigure() -> anyhow::Result<bool> {
    if cfg!(target_os = "macos") {
        unconfigure_macos()
    } else {
        unconfigure_linux()
    }
}

fn unconfigure_macos() -> anyhow::Result<bool> {
    let resolver_file = Path::new("/etc/resolver/seal");
    if resolver_file.exists() {
        std::fs::remove_file(resolver_file)?;
        eprintln!("removed /etc/resolver/seal");
        Ok(true)
    } else {
        Ok(false)
    }
}

fn unconfigure_linux() -> anyhow::Result<bool> {
    let mut removed = false;

    // systemd-resolved
    let resolved_conf = Path::new("/etc/systemd/resolved.conf.d/seal-tld.conf");
    if resolved_conf.exists() {
        std::fs::remove_file(resolved_conf)?;
        eprintln!("removed {}", resolved_conf.display());
        restart_service("systemd-resolved")?;
        removed = true;
    }

    // dnsmasq
    let dnsmasq_conf = Path::new("/etc/dnsmasq.d/seal-tld.conf");
    if dnsmasq_conf.exists() {
        std::fs::remove_file(dnsmasq_conf)?;
        eprintln!("removed {}", dnsmasq_conf.display());
        restart_service("dnsmasq")?;
        removed = true;
    }

    // NetworkManager dnsmasq
    let nm_conf = Path::new("/etc/NetworkManager/dnsmasq.d/seal-tld.conf");
    if nm_conf.exists() {
        std::fs::remove_file(nm_conf)?;
        eprintln!("removed {}", nm_conf.display());
        restart_service("NetworkManager")?;
        removed = true;
    }

    Ok(removed)
}

/// Print instructions for manual DNS setup.
pub fn print_manual_instructions() {
    eprintln!("=== DNS Setup Required ===");
    eprintln!("Configure your system to resolve *.seal to 127.0.0.1");
    eprintln!();
    if cfg!(target_os = "macos") {
        eprintln!("  sudo mkdir -p /etc/resolver");
        eprintln!("  echo 'nameserver 127.0.0.1' | sudo tee /etc/resolver/seal");
    } else {
        eprintln!("Option 1 (recommended — dnsmasq):");
        eprintln!("  sudo apt install dnsmasq");
        eprintln!("  echo 'address=/seal/127.0.0.1' | sudo tee /etc/dnsmasq.d/seal-tld.conf");
        eprintln!("  sudo systemctl restart dnsmasq");
        eprintln!();
        eprintln!("Option 2 (systemd-resolved — seal daemon will run an embedded DNS server):");
        eprintln!("  sudo mkdir -p /etc/systemd/resolved.conf.d");
        eprintln!(
            "  printf '[Resolve]\\nDNS=127.0.0.1\\nDomains=~seal\\n' | sudo tee /etc/systemd/resolved.conf.d/seal-tld.conf"
        );
        eprintln!("  sudo systemctl restart systemd-resolved");
    }
    eprintln!("==========================");
}
