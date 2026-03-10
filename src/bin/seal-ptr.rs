use seal::dns;
use seal::tls;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

const USAGE: &str = "\
seal-ptr — point .seal DNS to a remote seal instance

USAGE:
    seal-ptr <COMMAND>

COMMANDS:
    install <host> <cert>   Configure DNS to resolve *.seal to <host> and
                            install <cert> as a trusted root CA.
                              sudo seal-ptr install 10.0.0.5 ./seal-root.pem

    start                   Start the embedded DNS daemon (if needed) and
                            enable it to start on boot.
                              sudo seal-ptr start

    run                     Run the embedded DNS daemon in the foreground.
                              sudo seal-ptr run

    stop                    Stop the embedded DNS daemon.
                              sudo seal-ptr stop

    uninstall               Remove DNS config, trust store entry, and service.
                              sudo seal-ptr uninstall

Get the root certificate from a seal instance:
    seal show-cert > seal-root.pem
";

const STATE_FILE: &str = "/etc/seal-ptr.conf";

const SYSTEMD_UNIT: &str = "/etc/systemd/system/seal-ptr.service";
const LAUNCHD_PLIST: &str = "/Library/LaunchDaemons/com.seal-ptr.daemon.plist";
const LAUNCHD_LABEL: &str = "com.seal-ptr.daemon";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("install") => {
            let host = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("usage: seal-ptr install <host> <cert>"))?;
            let cert = args
                .get(3)
                .ok_or_else(|| anyhow::anyhow!("usage: seal-ptr install <host> <cert>"))?;
            cmd_install(host, Path::new(cert))
        }
        Some("start") => {
            if args.get(2).map(|s| s.as_str()) == Some("--foreground") {
                cmd_run().await
            } else {
                cmd_start()
            }
        }
        Some("run") => cmd_run().await,
        Some("stop") => cmd_stop(),
        Some("uninstall") => cmd_uninstall(),
        _ => {
            eprint!("{USAGE}");
            Ok(())
        }
    }
}

/// Read the saved target IP from the state file.
fn read_target() -> anyhow::Result<Ipv4Addr> {
    let content = std::fs::read_to_string(STATE_FILE)
        .map_err(|_| anyhow::anyhow!("not installed. Run `sudo seal-ptr install <host> <cert>` first."))?;
    let ip: Ipv4Addr = content.trim().parse()
        .map_err(|_| anyhow::anyhow!("invalid IP in {STATE_FILE}"))?;
    Ok(ip)
}

fn cmd_install(host: &str, cert_path: &Path) -> anyhow::Result<()> {
    // Validate host is an IPv4 address
    let target: Ipv4Addr = host
        .parse()
        .map_err(|_| anyhow::anyhow!("host must be an IPv4 address (e.g. 10.0.0.5)"))?;

    // Validate cert file exists
    if !cert_path.exists() {
        anyhow::bail!("certificate file not found: {}", cert_path.display());
    }

    // Save target IP for the daemon
    std::fs::write(STATE_FILE, format!("{target}\n"))?;
    eprintln!("saved target: {target}");

    // Install the root CA certificate
    // Copy cert to the expected location for trust store installation
    let ca_dir = seal_ptr_ca_dir();
    std::fs::create_dir_all(&ca_dir)?;
    let root_cert_dest = ca_dir.join("root.cert.pem");
    std::fs::copy(cert_path, &root_cert_dest)?;

    eprintln!();
    tls::install_trust_store(&ca_dir)?;

    // Configure DNS
    eprintln!();
    let method = dns::detect_method();
    if method.needs_embedded_dns() {
        // For systemd-resolved/macOS: DNS config points to 127.0.0.1,
        // our embedded DNS daemon returns the target IP.
        match dns::configure() {
            Ok((method, true)) => {
                eprintln!("DNS configured for .seal TLD");
                eprintln!("note: the seal-ptr daemon will run an embedded DNS server");
                eprintln!("start it with: sudo seal-ptr start");
                if method.needs_embedded_dns() {
                    // already printed the note
                }
            }
            Ok((_, false)) => eprintln!("DNS already configured"),
            Err(e) => {
                eprintln!("warning: could not auto-configure DNS: {e}");
                dns::print_manual_instructions();
            }
        }
    } else {
        // dnsmasq can resolve directly to the target IP
        match dns::configure_for(host) {
            Ok((_, true)) => eprintln!("DNS configured for .seal TLD (via dnsmasq → {target})"),
            Ok((_, false)) => eprintln!("DNS already configured"),
            Err(e) => {
                eprintln!("warning: could not auto-configure DNS: {e}");
                dns::print_manual_instructions();
            }
        }
    }

    eprintln!();
    if method.needs_embedded_dns() {
        eprintln!("install complete. Start the DNS daemon with:");
        eprintln!("  sudo seal-ptr start");
    } else {
        eprintln!("install complete. No daemon needed (dnsmasq handles DNS).");
    }
    Ok(())
}

fn cmd_start() -> anyhow::Result<()> {
    let target = read_target()?;
    let method = dns::detect_method();

    if !method.needs_embedded_dns() {
        eprintln!("no daemon needed — dnsmasq resolves *.seal to {target} directly");
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    install_service(&exe)?;
    start_service()?;

    eprintln!("seal-ptr DNS daemon started (*.seal → {target})");
    Ok(())
}

async fn cmd_run() -> anyhow::Result<()> {
    let target = read_target()?;

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("seal-ptr DNS daemon starting (*.seal → {target})");
    seal::dns_server::run(target).await
}

fn cmd_stop() -> anyhow::Result<()> {
    stop_service()?;
    eprintln!("seal-ptr DNS daemon stopped");
    Ok(())
}

fn cmd_uninstall() -> anyhow::Result<()> {
    // Stop and remove service
    let _ = uninstall_service();

    // Remove trust store entry
    eprintln!();
    match tls::uninstall_trust_store() {
        Ok(()) => {}
        Err(e) => eprintln!("warning: could not remove trust store entry: {e}"),
    }

    // Remove DNS config
    eprintln!();
    match dns::unconfigure() {
        Ok(true) => eprintln!("DNS configuration removed"),
        Ok(false) => eprintln!("no DNS configuration found to remove"),
        Err(e) => eprintln!("warning: could not remove DNS configuration: {e}"),
    }

    // Remove state
    let ca_dir = seal_ptr_ca_dir();
    let _ = std::fs::remove_dir_all(&ca_dir);
    let _ = std::fs::remove_file(STATE_FILE);
    eprintln!();
    eprintln!("seal-ptr has been fully uninstalled");
    Ok(())
}

fn seal_ptr_ca_dir() -> PathBuf {
    PathBuf::from("/etc/seal-ptr-ca")
}

// --- service management (simplified, seal-ptr specific) ---

fn install_service(exe_path: &Path) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        install_service_launchd(exe_path)
    } else {
        install_service_systemd(exe_path)
    }
}

fn start_service() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        run_cmd("launchctl", &["load", "-w", LAUNCHD_PLIST])
    } else {
        run_cmd("systemctl", &["start", "seal-ptr"])
    }
}

fn stop_service() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        run_cmd("launchctl", &["unload", LAUNCHD_PLIST])
    } else {
        run_cmd("systemctl", &["stop", "seal-ptr"])
    }
}

fn uninstall_service() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        let _ = run_cmd("launchctl", &["unload", "-w", LAUNCHD_PLIST]);
        let path = Path::new(LAUNCHD_PLIST);
        if path.exists() {
            std::fs::remove_file(path)?;
            eprintln!("launchd service removed");
        }
    } else {
        let _ = run_cmd("systemctl", &["stop", "seal-ptr"]);
        let _ = run_cmd("systemctl", &["disable", "seal-ptr"]);
        let path = Path::new(SYSTEMD_UNIT);
        if path.exists() {
            std::fs::remove_file(path)?;
            run_cmd("systemctl", &["daemon-reload"])?;
            eprintln!("systemd service removed");
        }
    }
    Ok(())
}

fn install_service_systemd(exe_path: &Path) -> anyhow::Result<()> {
    let unit = format!(
        "\
[Unit]
Description=seal-ptr — DNS for remote .seal instance
After=network.target

[Service]
Type=exec
ExecStart={exe} start --foreground
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
",
        exe = exe_path.display(),
    );

    std::fs::write(SYSTEMD_UNIT, unit)?;
    eprintln!("wrote {SYSTEMD_UNIT}");

    run_cmd("systemctl", &["daemon-reload"])?;
    run_cmd("systemctl", &["enable", "seal-ptr"])?;
    eprintln!("systemd service installed and enabled");
    Ok(())
}

fn install_service_launchd(exe_path: &Path) -> anyhow::Result<()> {
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>start</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        exe = exe_path.display(),
    );

    std::fs::write(LAUNCHD_PLIST, plist)?;
    eprintln!("wrote {LAUNCHD_PLIST}");
    eprintln!("launchd service installed");
    Ok(())
}

fn run_cmd(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = std::process::Command::new(cmd).args(args).status()?;
    if !status.success() {
        anyhow::bail!("{} {} failed (exit {})", cmd, args.join(" "), status);
    }
    Ok(())
}
