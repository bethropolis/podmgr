use std::ffi::OsString;
use std::path::{Path, PathBuf};

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
    crate::commands::ensure_running(name, dry_run)?;
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
) -> Result<()> {
    let tty_flag = if nix::unistd::isatty(0).unwrap_or(false) {
        "-it"
    } else {
        "-i"
    };
    if dry_run {
        let mut exec_args: Vec<OsString> =
            podbox::process::args(&["exec", tty_flag, "-u", &env.username, name]);
        for a in cmd_args {
            exec_args.push(a.into());
        }
        println!("podman {}", args_to_string(&exec_args));
        return Ok(());
    }
    crate::commands::ensure_running(name, dry_run)?;
    let mut exec_args: Vec<OsString> =
        podbox::process::args(&["exec", tty_flag, "-u", &env.username, name]);
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
    crate::commands::ensure_running(name, dry_run)?;
    let mut exec_args: Vec<OsString> =
        podbox::process::args(&["exec", "-d", "-u", &env.username, name, app]);
    for a in app_args {
        exec_args.push(a.into());
    }
    podbox::process::spawn_interactive("podman", &exec_args).map(|_| ())
}

/// Print the container's running state.
pub fn run_status(name: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("podman inspect --format {{{{.State.Status}}}} {}", name);
        return Ok(());
    }
    let state = query_state(name)?;
    let state_str = match state {
        ContainerState::Running => "running",
        ContainerState::Stopped => "stopped",
        ContainerState::Missing => "missing",
    };
    println!("{} [{}]", name, state_str);
    Ok(())
}

/// Tail or dump container logs.
pub fn run_logs(name: &str, follow: bool, tail: Option<u32>, dry_run: bool) -> Result<()> {
    let mut args: Vec<OsString> = vec!["logs".into()];
    if follow {
        args.push("-f".into());
    }
    if let Some(t) = tail {
        args.push("--tail".into());
        args.push(t.to_string().into());
    }
    args.push(name.into());
    if dry_run {
        println!("podman {}", args_to_string(&args));
        return Ok(());
    }
    podbox::process::spawn_interactive("podman", &args).map(|_| ())
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
    let guest_paths = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("podbox-guest"))),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("podmgr-guest"))),
        which::which("podbox-guest").ok(),
        which::which("podmgr-guest").ok(),
        std::env::var("PODBOX_GUEST_BIN").map(PathBuf::from).ok(),
        std::env::var("PODMGR_GUEST_BIN").map(PathBuf::from).ok(),
    ];
    let found = guest_paths
        .iter()
        .any(|p| p.as_ref().map(|p| p.exists()).unwrap_or(false));
    if found {
        println!("[PASS] podbox-guest binary found");
        passes += 1;
    } else {
        println!("[FAIL] podbox-guest binary not found");
        println!("       Prebuilt images bundle it: podbox pull <name>");
        println!(
            "       Build: cargo build -p podbox-guest --release --target x86_64-unknown-linux-musl"
        );
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
    let runtime_dir = socket.parent().ok_or_else(|| {
        anyhow::anyhow!("Cannot determine runtime directory from socket path")
    })?;

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
