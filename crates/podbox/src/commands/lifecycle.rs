use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;

use podbox::config::Config;
use podbox::env::HostEnv;
use podbox::error::PodboxError;
use podbox::xdg::ResolvedXdgDirs;

fn snapshot_tag(tag: &str, name: &str) -> String {
    format!("localhost/podbox-{}:snapshot-{}", name, tag)
}

fn snapshots_dir() -> PathBuf {
    podbox::config::config_dir().join("snapshots")
}

/// Snapshot the current container state as a tagged image.
pub fn run_snapshot(_config: &Config, name: &str, tag: Option<&str>) -> Result<()> {
    let tag: String = match tag {
        Some(t) => t.to_string(),
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string()),
    };

    let container_name = format!("podbox-{}", name);
    let image_tag = snapshot_tag(&tag, name);

    eprintln!(
        "Snapshotting container '{}' as '{}'...",
        container_name, image_tag
    );

    let output = podbox::process::run_piped(
        "podman",
        &podbox::process::args(&["commit", &container_name, &image_tag]),
    )?;
    print!("{}", String::from_utf8_lossy(&output.stdout));

    // Store metadata
    let dir = snapshots_dir().join(name);
    std::fs::create_dir_all(&dir)?;
    let meta_path = dir.join(format!("{}.toml", tag));
    let now_rfc = date_now_rfc3339();
    let meta = format!(
        "tag = \"{}\"\ncreated = \"{}\"\nimage = \"{}\"\n",
        tag, now_rfc, image_tag
    );
    std::fs::write(&meta_path, &meta)?;

    println!("✓ Snapshot '{}' saved (tag: {})", image_tag, tag);
    Ok(())
}

fn date_now_rfc3339() -> String {
    // Simple RFC 3339 without chrono
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Days since epoch
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Compute year/month/day from days since epoch
    let (year, month, day) = days_to_date(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// Restore a container from a snapshot image.
pub fn run_restore(_config: &Config, name: &str, tag: &str) -> Result<()> {
    let snapshot_img = snapshot_tag(tag, name);
    let latest_img = format!("localhost/podbox-{}:latest", name);

    // Verify snapshot exists
    let exists = podbox::podman::image_exists(&snapshot_img).unwrap_or(false);
    if !exists {
        anyhow::bail!("Snapshot '{}' not found as image '{}'", tag, snapshot_img);
    }

    // Stop the container
    eprintln!("Stopping container 'podbox-{}'...", name);
    let _ = podbox::process::run_piped(
        "podman",
        &podbox::process::args(&["stop", &format!("podbox-{}", name)]),
    );

    // Re-tag snapshot as the main image
    eprintln!("Restoring from snapshot '{}'...", snapshot_img);
    let output = podbox::process::run_piped(
        "podman",
        &podbox::process::args(&["tag", &snapshot_img, &latest_img]),
    )?;
    if !output.status.success() {
        anyhow::bail!("Failed to tag snapshot image");
    }

    // Start the container
    eprintln!("Starting container...");
    let _ = podbox::process::run_piped(
        "podman",
        &podbox::process::args(&["start", &format!("podbox-{}", name)]),
    );

    println!("✓ Restored '{}' from snapshot '{}'", name, tag);
    Ok(())
}

/// Build the container image (or pull a prebuilt image).
pub fn run_build(
    config: &Config,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    dry_run: bool,
    rebuild: bool,
    no_diff: bool,
) -> Result<()> {
    podbox::build::run(config, env, xdg, dry_run, rebuild)?;
    if !dry_run && config.lifecycle.quadlet {
        println!("\nRun `podbox enable` to install Quadlet files.");
    }
    // Post-build drift check (best-effort).
    if !dry_run && !no_diff {
        let name = &config.container.name;
        if let Ok(state) = podbox::podman::query_state(name) {
            if state == podbox::podman::ContainerState::Running {
                match podbox::diff::compute(config, name, &env.username) {
                    Ok(result) if result.has_drift => {
                        println!("\n── Package drift detected ──");
                        println!("{}", podbox::diff::format_report(&result));
                        println!("Run `podbox diff --apply` to update the TOML.");
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("Warning: drift check skipped ({})", e),
                }
            }
        }
    }
    Ok(())
}

/// Install Quadlet files (enable systemd container lifecycle).
pub fn run_enable(
    config: &Config,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    dry_run: bool,
) -> Result<()> {
    podbox::quadlet_install::install(config, env, xdg, dry_run)?;
    if !dry_run {
        println!("\nRun `podbox shell` to start and enter the container.");
    }
    Ok(())
}

/// Remove Quadlet files (disable systemd container lifecycle).
pub fn run_disable(name: &str) -> Result<()> {
    podbox::quadlet_install::uninstall(name)
}

/// Start the container, auto-healing missing images and Quadlet files.
pub fn run_start(
    config: &Config,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    name: &str,
    dry_run: bool,
    timeout_secs: u64,
) -> Result<()> {
    if dry_run {
        println!("podman start {}", name);
        return Ok(());
    }

    let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
    if !podbox::podman::image_exists(&local_tag).unwrap_or(false) {
        println!("Image not found, building first...");
        podbox::build::run(config, env, xdg, false, false)?;
    }

    let qdir = dirs::config_dir()
        .unwrap_or_else(|| podbox::config::expand_tilde("~/.config"))
        .join("containers/systemd");
    let container_file = qdir.join(format!("{}.container", name));
    if !container_file.exists() {
        println!("Quadlet files not found, installing...");
        podbox::quadlet_install::install(config, env, xdg, false)?;
    }

    println!("Starting container...");
    crate::commands::ensure_running(name, false, timeout_secs)?;
    println!("Container '{}' is running!", name);
    Ok(())
}

/// Stop the container.
///
/// Uses `systemctl --user stop` when quadlet is enabled so that systemd
/// tracks the service state transition (preventing a stale "unknown" in
/// subsequent `systemctl is-active` checks).
pub fn run_stop(config: &Config, name: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        if config.lifecycle.quadlet && which::which("systemctl").is_ok() {
            println!("systemctl --user stop {}", name);
        } else {
            println!("podman stop {}", name);
        }
        return Ok(());
    }
    if config.lifecycle.quadlet && which::which("systemctl").is_ok() {
        let args = podbox::process::args(&["--user", "stop", name]);
        podbox::process::spawn_interactive("systemctl", &args).map(|_| ())
    } else {
        let args = podbox::process::args(&["stop", name]);
        podbox::process::spawn_interactive("podman", &args).map(|_| ())
    }
}

/// Update a container: pull latest image, rebuild, and restart.
pub fn run_update(
    config: &Config,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    name: &str,
    dry_run: bool,
    no_restart: bool,
) -> Result<()> {
    if dry_run {
        println!("podbox update: pull/rebuild and restart {}", name);
        println!("  build::run(config, env, xdg, dry_run: true, rebuild: true)");
        if !no_restart {
            if config.lifecycle.quadlet && which::which("systemctl").is_ok() {
                println!("  systemctl --user restart {}", name);
            } else {
                println!("  podman restart {}", name);
            }
        }
        return Ok(());
    }

    println!("Updating '{}'...", name);

    podbox::build::run(config, env, xdg, false, true)?;

    if no_restart {
        println!("Image updated. Restart skipped (--no-restart).");
        return Ok(());
    }

    println!("Restarting container...");
    if config.lifecycle.quadlet && which::which("systemctl").is_ok() {
        let args = podbox::process::args(&["--user", "restart", name]);
        podbox::process::spawn_interactive("systemctl", &args)?;
    } else {
        let args = podbox::process::args(&["restart", name]);
        podbox::process::spawn_interactive("podman", &args)?;
    }

    println!("Update complete.");
    Ok(())
}

/// Remove a container and optionally its home directory.
pub fn run_remove(
    config: &Config,
    name: &str,
    dry_run: bool,
    all: bool,
    force: bool,
) -> Result<()> {
    if dry_run {
        println!("podman stop {}", name);
        println!("podman rm {}", name);
        if all {
            println!("rm -rf {}", config.container.home.display());
        }
        return Ok(());
    }

    if !force {
        print!("Remove container '{}'? [y/N] ", name);
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let stop_args = podbox::process::args(&["stop", name]);
    let _ = podbox::process::run_piped("podman", &stop_args);

    let rm_args = podbox::process::args(&["rm", name]);
    let output = podbox::process::run_piped("podman", &rm_args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(
            PodboxError::ContainerRemoveFailed(name.to_string(), stderr.to_string()).into(),
        );
    }
    println!("Container '{}' removed.", name);

    if all {
        let home = &config.container.home;
        if home.exists() {
            if !force {
                print!("Remove home directory '{}'? [y/N] ", home.display());
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Home directory kept.");
                    return Ok(());
                }
            }
            std::fs::remove_dir_all(home)?;
            println!("Home directory '{}' removed.", home.display());
        }
    }

    Ok(())
}

/// Scan for stale/orphaned containers that should be cleaned up.
///
/// A container is considered stale if:
/// - Its Quadlet `.container` file exists but there's no matching config TOML
/// - The podman container is `Missing` (removed manually but Quadlet remains)
/// - The systemd unit is in `failed` state
fn find_stale_containers() -> Vec<String> {
    let qdir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("containers/systemd");
    let config_dir = podbox::config::config_dir();

    let mut stale = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&qdir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "container").unwrap_or(false) {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                let config_path = config_dir.join(format!("{}.toml", name));

                if !config_path.exists() {
                    stale.push(name);
                    continue;
                }

                if let Ok(state) = podbox::podman::query_state(&name) {
                    match state {
                        podbox::podman::ContainerState::Missing => stale.push(name),
                        podbox::podman::ContainerState::Stopped => {
                            if let Ok(output) = std::process::Command::new("systemctl")
                                .args(["--user", "is-failed", &format!("{}.service", name)])
                                .output()
                            {
                                if String::from_utf8_lossy(&output.stdout).trim() == "failed" {
                                    stale.push(name);
                                }
                            }
                        }
                        podbox::podman::ContainerState::Running => {}
                    }
                }
            }
        }
    }

    stale
}

/// Remove all stale containers interactively.
///
/// When `--force` is set, skip the confirmation prompt.
pub fn run_remove_stale(dry_run: bool, force: bool) -> Result<()> {
    let stale = find_stale_containers();
    if stale.is_empty() {
        println!("No stale containers found.");
        return Ok(());
    }

    println!("Stale containers found:");
    for name in &stale {
        let config_path = podbox::config::config_dir().join(format!("{}.toml", name));
        let reason = if !config_path.exists() {
            "orphaned Quadlet (no config)"
        } else {
            "container not running or failed"
        };
        println!("  {}  ({})", name, reason);
    }

    if !force {
        print!("Remove these? [y/N] ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    for name in &stale {
        if dry_run {
            println!("Would remove: {}", name);
            continue;
        }

        if let Err(e) = podbox::quadlet_install::uninstall(name) {
            eprintln!("Warning: failed to uninstall '{}': {}", name, e);
        }

        let _ = podbox::process::run_piped("podman", &podbox::process::args(&["rm", "-f", name]));

        if which::which("systemctl").is_ok() {
            let unit_names = [
                format!("{}.service", name),
                format!("{}.socket", name),
                format!("{}-host.service", name),
                format!("{}-proxy.service", name),
            ];
            for unit in &unit_names {
                let _ = podbox::process::run_piped(
                    "systemctl",
                    &podbox::process::args(&["--user", "reset-failed", unit]),
                );
            }
        }

        let config_path = podbox::config::config_dir().join(format!("{}.toml", name));
        if config_path.exists() {
            std::fs::remove_file(&config_path)?;
        }

        println!("✓ {} removed", name);
    }

    Ok(())
}
