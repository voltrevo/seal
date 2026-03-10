use std::path::Path;

/// Install the system service (systemd on Linux, launchd on macOS).
/// The service runs `seal start --foreground`.
pub fn install(exe_path: &Path) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        install_launchd(exe_path)
    } else {
        install_systemd(exe_path)
    }
}

/// Remove the system service.
pub fn uninstall() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        uninstall_launchd()
    } else {
        uninstall_systemd()
    }
}

/// Start the service via the system service manager.
pub fn start() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        start_launchd()
    } else {
        start_systemd()
    }
}

/// Stop the service via the system service manager.
pub fn stop() -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        stop_launchd()
    } else {
        stop_systemd()
    }
}

/// Check if the service is running. Returns Some(pid) or None.
pub fn status() -> anyhow::Result<Option<u32>> {
    if cfg!(target_os = "macos") {
        status_launchd()
    } else {
        status_systemd()
    }
}

// --- systemd (Linux) ---

const SYSTEMD_UNIT: &str = "/etc/systemd/system/seal.service";

fn install_systemd(exe_path: &Path) -> anyhow::Result<()> {
    let unit = format!(
        "\
[Unit]
Description=Seal — secure frontends
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
    run_cmd("systemctl", &["enable", "seal"])?;
    eprintln!("systemd service installed and enabled");
    Ok(())
}

fn uninstall_systemd() -> anyhow::Result<()> {
    let path = Path::new(SYSTEMD_UNIT);
    if !path.exists() {
        return Ok(());
    }

    // Stop + disable first (ignore errors — may already be stopped)
    let _ = run_cmd("systemctl", &["stop", "seal"]);
    let _ = run_cmd("systemctl", &["disable", "seal"]);

    std::fs::remove_file(path)?;
    run_cmd("systemctl", &["daemon-reload"])?;
    eprintln!("systemd service removed");
    Ok(())
}

fn start_systemd() -> anyhow::Result<()> {
    run_cmd("systemctl", &["start", "seal"])
}

fn stop_systemd() -> anyhow::Result<()> {
    run_cmd("systemctl", &["stop", "seal"])
}

fn status_systemd() -> anyhow::Result<Option<u32>> {
    let output = std::process::Command::new("systemctl")
        .args(["show", "seal", "--property=MainPID", "--value"])
        .output()?;
    let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match pid_str.parse::<u32>() {
        Ok(pid) if pid > 0 => Ok(Some(pid)),
        _ => Ok(None),
    }
}

// --- launchd (macOS) ---

const LAUNCHD_PLIST: &str = "/Library/LaunchDaemons/com.seal.daemon.plist";
const LAUNCHD_LABEL: &str = "com.seal.daemon";

fn install_launchd(exe_path: &Path) -> anyhow::Result<()> {
    let data_dir = seal::state::data_dir();
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
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        exe = exe_path.display(),
        log = data_dir.join("daemon.log").display(),
    );

    std::fs::write(LAUNCHD_PLIST, plist)?;
    eprintln!("wrote {LAUNCHD_PLIST}");
    eprintln!("launchd service installed");
    Ok(())
}

fn uninstall_launchd() -> anyhow::Result<()> {
    let path = Path::new(LAUNCHD_PLIST);
    if !path.exists() {
        return Ok(());
    }

    let _ = run_cmd("launchctl", &["unload", "-w", LAUNCHD_PLIST]);
    std::fs::remove_file(path)?;
    eprintln!("launchd service removed");
    Ok(())
}

fn start_launchd() -> anyhow::Result<()> {
    run_cmd("launchctl", &["load", "-w", LAUNCHD_PLIST])
}

fn stop_launchd() -> anyhow::Result<()> {
    run_cmd("launchctl", &["unload", LAUNCHD_PLIST])
}

fn status_launchd() -> anyhow::Result<Option<u32>> {
    let output = std::process::Command::new("launchctl")
        .args(["list", LAUNCHD_LABEL])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    // Parse PID from first line: "PID\tStatus\tLabel" or "{pid}\t0\tcom.seal.daemon"
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if let Some(pid_str) = parts.first() {
            if let Ok(pid) = pid_str.parse::<u32>() {
                return Ok(Some(pid));
            }
        }
        break;
    }
    Ok(None)
}

// --- helpers ---

fn run_cmd(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = std::process::Command::new(cmd).args(args).status()?;
    if !status.success() {
        anyhow::bail!("{} {} failed (exit {})", cmd, args.join(" "), status);
    }
    Ok(())
}
