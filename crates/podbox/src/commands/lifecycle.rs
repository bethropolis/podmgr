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
    crate::commands::ensure_running(name, false)?;
    println!("Container '{}' is running!", name);
    Ok(())
}

/// Stop the container.
pub fn run_stop(name: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("podman stop {}", name);
        return Ok(());
    }
    let args = podbox::process::args(&["stop", name]);
    podbox::process::spawn_interactive("podman", &args).map(|_| ())
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
        return Err(PodboxError::ContainerRemoveFailed(name.to_string(), stderr.to_string()).into());
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
