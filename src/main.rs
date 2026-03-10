mod home;
mod local;
mod log;
mod registry;
mod serve;
mod server;
mod service;
mod url;

use seal::dns;
use seal::dns_server;
use seal::state;
use seal::state::AppState;
use seal::tls;
use seal::tls::CertStore;

const USAGE: &str = "\
seal — secure frontends

USAGE:
    seal <COMMAND>

COMMANDS:
    install  Generate CA certificates, install trust store, and configure DNS.
             This needs root privileges on most systems:
               sudo seal install

    start    Start the daemon and enable it to start on boot.
             Requires root/sudo because it binds to port 443:
               sudo seal start

    run      Run the daemon in the foreground.
               sudo seal run

    stop     Stop the running daemon.
               sudo seal stop

    restart  Stop and start the daemon.
               sudo seal restart

    status   Check if the daemon is running.
               seal status

    reinstall  Stop daemon, reconfigure DNS, restart if it was previously
               running via `start`. Certificates are preserved.
               Use --full to also regenerate certs and trust store.
                 sudo seal reinstall

    uninstall  Stop daemon, remove DNS, trust store, service, and app data.
               CA certificates on disk are kept so `seal install` reuses
               them (seal-ptr clients stay valid). Use --full to also
               delete the CA certificates.
                 sudo seal uninstall

    show-cert  Print the root CA certificate (PEM) to stdout.
                 seal show-cert > seal-root.pem

Run `seal install` once after installing, then `seal start` to begin.
Visit https://home.seal/ to manage your apps.
";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("install") => cmd_install().await,
        Some("start") => {
            if args.get(2).map(|s| s.as_str()) == Some("--foreground") {
                cmd_run(true).await
            } else {
                cmd_start()
            }
        }
        Some("run") => cmd_run(false).await,
        Some("stop") => cmd_stop(),
        Some("restart") => cmd_restart(),
        Some("status") => cmd_status(),
        Some("reinstall") => {
            let full = args.iter().any(|a| a == "--full");
            cmd_reinstall(full).await
        }
        Some("uninstall") => {
            let full = args.iter().any(|a| a == "--full");
            cmd_uninstall(full)
        }
        Some("show-cert") => cmd_show_cert(),
        _ => {
            eprint!("{USAGE}");
            Ok(())
        }
    }
}

/// `seal install` — generate certs, install trust store, configure DNS.
async fn cmd_install() -> anyhow::Result<()> {
    let data_dir = state::data_dir();
    eprintln!("data directory: {}", data_dir.display());

    // Ensure directories exist
    std::fs::create_dir_all(data_dir.join("ca"))?;
    std::fs::create_dir_all(data_dir.join("bundles"))?;
    std::fs::create_dir_all(data_dir.join("sites"))?;
    std::fs::create_dir_all(data_dir.join("state"))?;

    // Generate CA chain (idempotent — skips if already exists)
    let ca_dir = data_dir.join("ca");
    if CertStore::exists(&ca_dir) {
        eprintln!("CA certificates already exist, skipping generation");
    } else {
        eprintln!("generating CA certificate chain...");
        CertStore::install(&ca_dir)?;
        eprintln!("CA certificates generated");
    }

    // Install root CA into system trust store
    eprintln!();
    tls::install_trust_store(&ca_dir)?;

    // Configure DNS
    eprintln!();
    match dns::configure() {
        Ok((method, true)) => {
            eprintln!("DNS configured for .seal TLD");
            if method.needs_embedded_dns() {
                eprintln!("note: the daemon will run an embedded DNS server on port 53 (install dnsmasq to avoid this)");
            }
        }
        Ok((_method, false)) => eprintln!("DNS already configured"),
        Err(e) => {
            eprintln!("warning: could not auto-configure DNS: {e}");
            dns::print_manual_instructions();
        }
    }

    eprintln!();
    eprintln!("install complete. Start the daemon with:");
    eprintln!("  sudo seal start");
    Ok(())
}

/// Check preconditions common to start and run: not already running, init done.
fn check_can_start() -> anyhow::Result<()> {
    // Check service manager first
    if let Ok(Some(pid)) = service::status() {
        anyhow::bail!("daemon already running (pid {pid})");
    }

    // Also check PID file (covers `seal run` case)
    let pid_path = state::pid_file();
    if let Some(pid) = state::read_pid(&pid_path)? {
        if is_process_alive(pid) {
            anyhow::bail!("daemon already running (pid {pid})");
        }
        state::remove_pid(&pid_path);
    }

    let ca_dir = state::data_dir().join("ca");
    if !CertStore::exists(&ca_dir) {
        anyhow::bail!("not installed. Run `sudo seal install` first.");
    }

    Ok(())
}

/// `seal start` — install system service and start the daemon.
fn cmd_start() -> anyhow::Result<()> {
    check_can_start()?;

    let exe = std::env::current_exe()?;

    // Install/update the system service (idempotent)
    service::install(&exe)?;

    // Start via service manager
    service::start()?;

    let log_path = state::data_dir().join("daemon.log");
    eprintln!("daemon started");
    eprintln!("logs: {}", log_path.display());
    eprintln!("visit https://home.seal/");
    Ok(())
}

/// `seal run` — run the server in the foreground.
/// Also used internally by `seal start --foreground` (with log_to_file=true).
async fn cmd_run(log_to_file: bool) -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    // When launched as a background child (--foreground), skip the check
    // since the parent already verified and the PID file doesn't exist yet.
    if !log_to_file {
        check_can_start()?;
    }

    let data_dir = state::data_dir();

    if log_to_file {
        let log_path = data_dir.join("daemon.log");
        let rotating_log = log::RotatingLog::new(log_path)?;
        tracing_subscriber::fmt()
            .with_writer(rotating_log)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .init();
    }

    let state = AppState::new(data_dir)?;
    let cert_store = CertStore::install(&state.ca_dir())?;

    // Write PID file
    let pid_path = state.pid_file();
    state::write_pid(&pid_path)?;

    // Clean up PID file on shutdown
    let pid_path_clone = pid_path.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        state::remove_pid(&pid_path_clone);
        std::process::exit(0);
    });

    tracing::info!("seal daemon starting (pid {})", std::process::id());

    // Start embedded DNS server if needed (systemd-resolved / macOS resolver)
    let dns_method = dns::detect_method();
    if dns_method.needs_embedded_dns() {
        tracing::info!("starting embedded DNS server (method: {:?})", dns_method);
        tokio::spawn(async {
            if let Err(e) = dns_server::run(std::net::Ipv4Addr::LOCALHOST).await {
                tracing::error!("DNS server failed: {e}");
            }
        });
    } else {
        tracing::info!("DNS handled by {:?}, no embedded DNS needed", dns_method);
    }

    let result = server::run(state, cert_store).await;

    state::remove_pid(&pid_path);
    result
}

/// `seal stop` — stop the daemon via the system service manager.
fn cmd_stop() -> anyhow::Result<()> {
    service::stop()?;
    eprintln!("daemon stopped");
    Ok(())
}

/// `seal restart` — stop and start the daemon.
fn cmd_restart() -> anyhow::Result<()> {
    cmd_stop()?;
    cmd_start()
}

/// `seal status` — check if daemon is running.
fn cmd_status() -> anyhow::Result<()> {
    match service::status()? {
        Some(pid) => eprintln!("daemon is running (pid {pid})"),
        None => eprintln!("daemon not running"),
    }
    Ok(())
}

/// `seal reinstall` — full teardown and re-install.
/// Without --full: keeps CA certificates on disk (seal-ptr clients stay valid).
/// With --full: also regenerates certs.
async fn cmd_reinstall(full: bool) -> anyhow::Result<()> {
    let was_service_running = matches!(service::status(), Ok(Some(_)));

    // Teardown — same as uninstall, but we don't print the final message
    cmd_uninstall_inner(full)?;

    // Re-install
    eprintln!();
    cmd_install().await?;

    // Restart if it was running via service
    if was_service_running {
        eprintln!();
        cmd_start()?;
    }

    Ok(())
}

/// `seal uninstall` — remove seal state from the system.
fn cmd_uninstall(full: bool) -> anyhow::Result<()> {
    cmd_uninstall_inner(full)?;

    eprintln!();
    if full {
        eprintln!("seal has been fully uninstalled");
    } else {
        eprintln!("seal has been uninstalled (CA certificates retained for seal-ptr compatibility)");
        eprintln!("to also remove certificates: sudo seal uninstall --full");
    }
    Ok(())
}

/// Shared teardown logic for uninstall and reinstall.
/// Without --full: keeps CA certificates on disk.
/// With --full: removes everything including CA.
fn cmd_uninstall_inner(full: bool) -> anyhow::Result<()> {
    let data_dir = state::data_dir();

    // 1. Stop and remove system service
    match service::uninstall() {
        Ok(()) => {}
        Err(e) => eprintln!("warning: could not remove system service: {e}"),
    }

    // 2. Remove DNS configuration
    eprintln!();
    match dns::unconfigure() {
        Ok(true) => eprintln!("DNS configuration removed"),
        Ok(false) => eprintln!("no DNS configuration found to remove"),
        Err(e) => eprintln!("warning: could not remove DNS configuration: {e}"),
    }

    // 3. Remove root CA from system trust store
    eprintln!();
    match tls::uninstall_trust_store() {
        Ok(()) => {}
        Err(e) => eprintln!("warning: could not remove trust store entry: {e}"),
    }

    if full {
        // 4. Remove entire data directory (including ca/)
        eprintln!();
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir)?;
            eprintln!("removed {}", data_dir.display());
        } else {
            eprintln!("data directory already removed");
        }
    } else {
        // 4. Remove app data but keep ca/
        eprintln!();
        for subdir in ["bundles", "sites", "state"] {
            let path = data_dir.join(subdir);
            if path.exists() {
                std::fs::remove_dir_all(&path)?;
                eprintln!("removed {}", path.display());
            }
        }
        for file in ["daemon.log", "seal.pid"] {
            let path = data_dir.join(file);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
    }

    Ok(())
}

/// `seal show-cert` — print root CA certificate PEM to stdout.
fn cmd_show_cert() -> anyhow::Result<()> {
    let cert_path = state::data_dir().join("ca").join("root.cert.pem");
    if !cert_path.exists() {
        anyhow::bail!(
            "root certificate not found at {}. Run `sudo seal install` first.",
            cert_path.display()
        );
    }
    let pem = std::fs::read_to_string(&cert_path)?;
    print!("{pem}");
    Ok(())
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}
