use std::ffi::OsString;
use std::path::Path;

use anyhow::Result;

use podbox::config::Config;
use podbox::env::HostEnv;
use podbox::podman::{query_state, ContainerState};

/// Enter a shell inside the container.
pub fn run_shell_enter(config: &Config, name: &str, dry_run: bool) -> Result<()> {
    let env = podbox::env::resolve()?;
    let tty_flag = if nix::unistd::isatty(0).unwrap_or(false) {
        "-it"
    } else {
        "-i"
    };
    let home_in_container = format!("/home/{}", env.username);
    if dry_run {
        let exec_args = podbox::process::args(&[
            "exec",
            tty_flag,
            "-u",
            &env.username,
            "--workdir",
            &home_in_container,
            name,
            &config.container.shell,
        ]);
        println!("podman {}", args_to_string(&exec_args));
        return Ok(());
    }
    crate::commands::ensure_running(name, dry_run, crate::commands::DEFAULT_START_TIMEOUT_SECS)?;
    let exec_args = podbox::process::args(&[
        "exec",
        tty_flag,
        "-u",
        &env.username,
        "--workdir",
        &home_in_container,
        name,
        &config.container.shell,
    ]);
    let err = podbox::process::exec_replace("podman", &exec_args);
    Err(err)
}

/// Execute an arbitrary command inside the container.
pub fn run_exec(
    env: &HostEnv,
    name: &str,
    cmd_args: &[String],
    dry_run: bool,
    root: bool,
) -> Result<()> {
    let tty_flag = if nix::unistd::isatty(0).unwrap_or(false) {
        "-it"
    } else {
        "-i"
    };
    let base_args: &[&str] = if root {
        &["exec", tty_flag, name]
    } else {
        &["exec", tty_flag, "-u", &env.username, name]
    };
    if dry_run {
        let mut exec_args: Vec<OsString> = podbox::process::args(base_args);
        for a in cmd_args {
            exec_args.push(a.into());
        }
        println!("podman {}", args_to_string(&exec_args));
        return Ok(());
    }
    crate::commands::ensure_running(name, dry_run, crate::commands::DEFAULT_START_TIMEOUT_SECS)?;
    let mut exec_args: Vec<OsString> = podbox::process::args(base_args);
    for a in cmd_args {
        exec_args.push(a.into());
    }
    let err = podbox::process::exec_replace("podman", &exec_args);
    Err(err)
}

/// Run an app in the background inside the container.
pub fn run_run(
    env: &HostEnv,
    name: &str,
    app: &str,
    app_args: &[String],
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        let mut exec_args: Vec<OsString> =
            podbox::process::args(&["exec", "-d", "-u", &env.username, name, app]);
        for a in app_args {
            exec_args.push(a.into());
        }
        println!("podman {}", args_to_string(&exec_args));
        return Ok(());
    }
    crate::commands::ensure_running(name, dry_run, crate::commands::DEFAULT_START_TIMEOUT_SECS)?;
    let mut exec_args: Vec<OsString> =
        podbox::process::args(&["exec", "-d", "-u", &env.username, name, app]);
    for a in app_args {
        exec_args.push(a.into());
    }
    podbox::process::spawn_interactive("podman", &exec_args).map(|_| ())
}

fn quadlet_installed(name: &str) -> bool {
    let qdir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("containers/systemd");
    qdir.join(format!("{}.container", name)).exists()
}

/// Print the container's running state.
pub fn run_status(name: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("podman inspect --format {{{{.State.Status}}}} {}", name);
        return Ok(());
    }
    let state = query_state(name)?;
    match state {
        ContainerState::Running => println!("{} [running]", name),
        ContainerState::Stopped => println!("{} [stopped]", name),
        ContainerState::Missing => {
            if quadlet_installed(name) {
                println!("{} [not built]", name);
            } else {
                println!("{} [not installed]", name);
            }
        }
    }
    Ok(())
}

/// Check whether a container is managed by systemd (Quadlet).
fn is_systemd_managed(name: &str) -> bool {
    if which::which("systemctl").is_err() {
        return false;
    }
    std::process::Command::new("systemctl")
        .args([
            "--user",
            "--quiet",
            "is-enabled",
            &format!("{}.service", name),
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Show container logs, routing through journalctl for systemd-managed
/// containers and falling back to `podman logs` for standalone ones.
pub fn run_logs(
    name: &str,
    follow: bool,
    tail: Option<u32>,
    since: Option<String>,
    dry_run: bool,
) -> Result<()> {
    let lines = tail.unwrap_or(50);

    if is_systemd_managed(name) {
        let mut args: Vec<OsString> = vec![
            "--user".into(),
            "-u".into(),
            format!("{}.service", name).into(),
        ];
        if follow {
            args.push("-f".into());
        }
        args.push("-n".into());
        args.push(lines.to_string().into());
        if let Some(s) = &since {
            args.push("--since".into());
            args.push(s.into());
        }
        if dry_run {
            println!("journalctl {}", args_to_string(&args));
            return Ok(());
        }
        println!("Showing logs for: {}.service", name);
        podbox::process::spawn_interactive("journalctl", &args).map(|_| ())
    } else {
        let mut args: Vec<OsString> = vec!["logs".into()];
        if follow {
            args.push("-f".into());
        }
        args.push("--tail".into());
        args.push(lines.to_string().into());
        if let Some(s) = &since {
            args.push("--since".into());
            args.push(s.into());
        }
        args.push(name.into());
        if dry_run {
            println!("podman {}", args_to_string(&args));
            return Ok(());
        }
        podbox::process::spawn_interactive("podman", &args).map(|_| ())
    }
}

/// Run diagnostics on the container and host environment.
pub fn run_doctor(config: &Config, env: &HostEnv, fix: bool) -> Result<()> {
    let mut checks = 0;
    let mut passes = 0;
    let mut failures = 0;

    checks += 1;
    match podbox::podman::podman_version() {
        Ok(ver) if ver.at_least(5, 6) => {
            println!(
                "[PASS] podman {}.{}.{} (>= 5.6)",
                ver.major, ver.minor, ver.patch
            );
            passes += 1;
        }
        Ok(ver) if ver.at_least(5, 5) => {
            println!(
                "[WARN] podman {}.{}.{} (< 5.6) — upgrade to 5.6+ for Environment passthrough and native Quadlet management",
                ver.major, ver.minor, ver.patch
            );
            passes += 1;
        }
        Ok(ver) => {
            println!(
                "[FAIL] podman {}.{}.{} (< 5.5) — minimum supported version is 5.5",
                ver.major, ver.minor, ver.patch
            );
            failures += 1;
        }
        Err(_) => {
            println!("[FAIL] podman not found in PATH");
            failures += 1;
        }
    }

    if config.integration.wayland {
        checks += 1;
        if let Some(ref socket) = env.wayland_socket {
            println!("[PASS] Wayland socket found");
            passes += 1;

            checks += 1;
            match socket.metadata() {
                Ok(meta) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        let owner = meta.uid();
                        if owner != env.uid {
                            println!(
                                "[WARN] Wayland socket owner {} != host UID {}",
                                owner, env.uid
                            );
                            if fix {
                                match fix_wayland_socket_ownership(socket) {
                                    Ok(()) => {
                                        println!("       -> Ownership fixed");
                                        passes += 1;
                                    }
                                    Err(e) => {
                                        println!("       -> Fix failed: {e}");
                                        failures += 1;
                                    }
                                }
                            } else {
                                println!(
                                    "       Run with --fix to repair, or: podman unshare chown 0:0 {}",
                                    socket.display()
                                );
                            }
                        } else {
                            println!("[PASS] Wayland socket owner correct");
                            passes += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("[WARN] Could not stat Wayland socket: {e}");
                }
            }
        } else {
            println!("[WARN] Wayland socket not found (WAYLAND_DISPLAY may not be set)");
        }
    }

    checks += 1;
    match which::which("xdg-user-dir") {
        Ok(_) => {
            println!("[PASS] xdg-user-dir found");
            passes += 1;
        }
        Err(_) => println!("[WARN] xdg-user-dir not found -- install xdg-user-dirs"),
    }

    checks += 1;
    match std::fs::read_to_string("/etc/subuid") {
        Ok(content) => {
            let username = &env.username;
            if content.lines().any(|l| l.starts_with(username)) {
                println!(
                    "[PASS] user '{}' has sub-UID allocations in /etc/subuid",
                    username
                );
                passes += 1;
            } else {
                println!(
                    "[FAIL] user '{}' missing from /etc/subuid. Rootless Podman may fail.",
                    username
                );
                println!(
                    "       Fix: sudo usermod --add-subuids 100000-165535 {}",
                    username
                );
                failures += 1;
            }
        }
        Err(_) => {
            println!("[WARN] could not read /etc/subuid — check manually if rootless builds fail")
        }
    }

    checks += 1;
    match std::fs::read_to_string("/etc/subgid") {
        Ok(content) => {
            let username = &env.username;
            if content.lines().any(|l| l.starts_with(username)) {
                println!(
                    "[PASS] user '{}' has sub-GID allocations in /etc/subgid",
                    username
                );
                passes += 1;
            } else {
                println!(
                    "[FAIL] user '{}' missing from /etc/subgid. Rootless Podman may fail.",
                    username
                );
                println!(
                    "       Fix: sudo usermod --add-subgids 100000-165535 {}",
                    username
                );
                failures += 1;
            }
        }
        Err(_) => {
            println!("[WARN] could not read /etc/subgid — check manually if rootless builds fail")
        }
    }

    checks += 1;
    let has_embedded = !podbox::guest::PODBOX_GUEST_BINARY.is_empty();
    if has_embedded {
        println!(
            "[PASS] podbox-guest binary embedded ({} bytes)",
            podbox::guest::PODBOX_GUEST_BINARY.len()
        );
        passes += 1;
    } else {
        println!("[FAIL] podbox-guest binary embedded, but is empty");
        println!("       Rebuild podbox: cargo build --release -p podbox");
        failures += 1;
    }

    if config.lifecycle.autostart {
        checks += 1;
        if which::which("loginctl").is_ok() {
            let username = std::env::var("USER").unwrap_or_default();
            if !username.is_empty() {
                if let Ok(output) = podbox::process::run_piped(
                    "loginctl",
                    &[
                        "show-user".into(),
                        username.into(),
                        "--property=Linger".into(),
                    ],
                ) {
                    let out = String::from_utf8_lossy(&output.stdout);
                    if out.contains("yes") {
                        println!("[PASS] loginctl linger enabled");
                        passes += 1;
                    } else {
                        println!(
                            "[WARN] loginctl linger not enabled -- run: loginctl enable-linger $USER"
                        );
                    }
                }
            }
        }
    }

    if config.lifecycle.quadlet {
        checks += 1;
        let qdir = dirs::config_dir()
            .unwrap_or_else(|| podbox::config::expand_tilde("~/.config"))
            .join("containers/systemd");
        let container_file = qdir.join(format!("{}.container", config.container.name));
        if container_file.exists() {
            println!("[PASS] Quadlet files installed");
            passes += 1;
        } else {
            println!("[WARN] Quadlet files not found -- run: podbox enable");
        }
    }

    println!("\n{} / {} checks passed", passes, checks);
    if failures > 0 {
        Err(anyhow::anyhow!("{} check(s) failed", failures))
    } else {
        Ok(())
    }
}

fn fix_wayland_socket_ownership(socket: &Path) -> Result<()> {
    let runtime_dir = socket
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine runtime directory from socket path"))?;

    let output = std::process::Command::new("podman")
        .args(["unshare", "chown", "0:0"])
        .arg(socket)
        .arg(runtime_dir)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run podman unshare: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman unshare chown failed: {stderr}");
    }
    Ok(())
}

fn args_to_string(args: &[OsString]) -> String {
    args.iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}
