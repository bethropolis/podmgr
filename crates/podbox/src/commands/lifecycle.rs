use std::io::Write;

use anyhow::Result;

use podbox::config::Config;
use podbox::env::HostEnv;
use podbox::error::PodboxError;
use podbox::xdg::ResolvedXdgDirs;

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
