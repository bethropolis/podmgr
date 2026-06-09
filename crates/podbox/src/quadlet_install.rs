use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::codegen::quadlet;
use crate::config::{self, Config};
use crate::env::HostEnv;
use crate::podman::{podman_version, PodmanVersion};
use crate::xdg::ResolvedXdgDirs;

/// Directory for user Quadlet source files.
fn quadlet_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| config::expand_tilde("~/.config"))
        .join("containers/systemd")
}

/// Directory for user systemd unit files.
fn systemd_user_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| config::expand_tilde("~/.config"))
        .join("systemd/user")
}

/// Install systemd service and socket files for a container.
pub fn install(config: &Config, env: &HostEnv, xdg: &ResolvedXdgDirs, dry_run: bool) -> Result<()> {
    let name = &config.container.name;
    let ver = podman_version().unwrap_or(&PodmanVersion {
        major: 5,
        minor: 5,
        patch: 0,
    });
    let qdir = quadlet_dir();
    let sdir = systemd_user_dir();
    let context_dir = crate::build::build_context_dir(name);
    let containerfile_path = context_dir.join("Containerfile");

    let socket_content = quadlet::generate_socket(config);
    let container_content = quadlet::generate_container(config, env, xdg);
    let host_service_content = quadlet::generate_host_service(name);
    let dbus_proxy_content = quadlet::generate_dbus_proxy_service(name, config);

    let build_content = if !config.image.source().is_prebuilt() {
        Some(quadlet::generate_build(config, &containerfile_path))
    } else {
        None
    };

    if dry_run {
        if let Some(ref bc) = build_content {
            println!("=== {}.build ===", name);
            println!("{}", bc);
            println!();
        }
        println!("=== {}.socket ===", name);
        println!("{}", socket_content);
        println!();
        println!("=== {}.container ===", name);
        println!("{}", container_content);
        println!();
        println!("=== {}-host.service ===", name);
        println!("{}", host_service_content);
        if let Some(ref proxy) = dbus_proxy_content {
            println!();
            println!("=== {}-proxy.service ===", name);
            println!("{}", proxy);
        }
        return Ok(());
    }

    // Ensure home and runtime dirs exist
    std::fs::create_dir_all(&config.container.home).with_context(|| {
        format!(
            "failed to create home dir '{}'",
            config.container.home.display()
        )
    })?;

    if ver.at_least(5, 6) {
        // 5.6+: podman quadlet install handles .container + .build placement and daemon-reload
        let tmp = std::env::temp_dir().join(format!("podbox-install-{}", name));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp)?;
        if let Some(ref bc) = build_content {
            std::fs::write(tmp.join(format!("{}.build", name)), bc)?;
        }
        std::fs::write(tmp.join(format!("{}.container", name)), container_content)?;

        let args: Vec<std::ffi::OsString> = vec![
            "quadlet".into(),
            "install".into(),
            "--replace".into(),
            tmp.into(),
        ];
        let output = crate::process::run_piped("podman", &args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("podman quadlet install failed: {}", stderr);
        }
        println!("Quadlet files installed via podman quadlet install.");

        // Socket and host-service are custom systemd units (not Quadlet), install manually.
        std::fs::create_dir_all(&sdir)?;
        std::fs::write(sdir.join(format!("{}.socket", name)), socket_content)?;
        std::fs::write(
            sdir.join(format!("{}-host.service", name)),
            host_service_content,
        )?;
        if let Some(ref proxy) = dbus_proxy_content {
            std::fs::write(sdir.join(format!("{}-proxy.service", name)), proxy)?;
        }
        println!("Systemd units installed to {}", sdir.display());

        // daemon-reload so the socket/host-service units are picked up
        if which::which("systemctl").is_ok() {
            let reload_args: Vec<std::ffi::OsString> =
                vec!["--user".into(), "daemon-reload".into()];
            let _ = crate::process::run_piped("systemctl", &reload_args);
        }
        // Clean up stale states
        let reset_args: Vec<std::ffi::OsString> = vec![
            "--user".into(),
            "reset-failed".into(),
            format!("{}.service", name).into(),
            format!("{}.socket", name).into(),
            format!("{}-host.service", name).into(),
            format!("{}-proxy.service", name).into(),
        ];
        let _ = crate::process::run_piped("systemctl", &reset_args);

        // Enable and start the socket so it's active immediately and on boot.
        // The socket has Service=<name>-host.service, so this also triggers
        // the host socket server.
        let socket_unit = format!("{}.socket", name);
        let host_unit = format!("{}-host.service", name);
        // Stop socket and host service first so re-enable doesn't hit stale state (Issue #2).
        let _ = crate::process::run_piped(
            "systemctl",
            &["--user".into(), "stop".into(), socket_unit.clone().into()],
        );
        let _ = crate::process::run_piped(
            "systemctl",
            &["--user".into(), "stop".into(), host_unit.clone().into()],
        );
        let enable_args: Vec<std::ffi::OsString> = vec![
            "--user".into(),
            "enable".into(),
            "--now".into(),
            socket_unit.into(),
        ];
        let _ = crate::process::run_piped("systemctl", &enable_args);
        // Start host service so config changes take effect immediately (Issue #3).
        let _ = crate::process::run_piped(
            "systemctl",
            &["--user".into(), "start".into(), host_unit.into()],
        );
    } else {
        // 5.5 fallback: copy files manually
        std::fs::create_dir_all(&qdir)?;
        if let Some(ref bc) = build_content {
            std::fs::write(qdir.join(format!("{}.build", name)), bc)?;
        }
        std::fs::write(qdir.join(format!("{}.container", name)), container_content)?;

        std::fs::create_dir_all(&sdir)?;
        std::fs::write(sdir.join(format!("{}.socket", name)), socket_content)?;
        std::fs::write(
            sdir.join(format!("{}-host.service", name)),
            host_service_content,
        )?;
        if let Some(ref proxy) = dbus_proxy_content {
            std::fs::write(sdir.join(format!("{}-proxy.service", name)), proxy)?;
        }

        println!("Quadlet files installed to {}", qdir.display());
        println!("Systemd units installed to {}", sdir.display());
    }

    // Auto-export apps and bins
    for app in &config.integration.export.apps {
        if let Err(e) = crate::export::export_app(name, app) {
            eprintln!("Warning: auto-export app '{}' failed: {}", app, e);
        }
    }
    for bin in &config.integration.export.bins {
        if let Err(e) = crate::export::export_bin(name, bin) {
            eprintln!("Warning: auto-export bin '{}' failed: {}", bin, e);
        }
    }

    // daemon-reload + reset-failed (5.5 fallback path only)
    if !ver.at_least(5, 6) && which::which("systemctl").is_ok() {
        let reload_args: Vec<std::ffi::OsString> = vec!["--user".into(), "daemon-reload".into()];
        let output = crate::process::run_piped("systemctl", &reload_args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("Warning: daemon-reload failed: {}", stderr);
        } else {
            println!("systemd daemon-reload complete.");
        }
        let reset_args: Vec<std::ffi::OsString> = vec![
            "--user".into(),
            "reset-failed".into(),
            format!("{}.service", name).into(),
            format!("{}.socket", name).into(),
            format!("{}-host.service", name).into(),
            format!("{}-proxy.service", name).into(),
        ];
        let _ = crate::process::run_piped("systemctl", &reset_args);

        let socket_unit = format!("{}.socket", name);
        let host_unit = format!("{}-host.service", name);
        let _ = crate::process::run_piped(
            "systemctl",
            &["--user".into(), "stop".into(), socket_unit.clone().into()],
        );
        let _ = crate::process::run_piped(
            "systemctl",
            &["--user".into(), "stop".into(), host_unit.clone().into()],
        );
        let enable_args: Vec<std::ffi::OsString> = vec![
            "--user".into(),
            "enable".into(),
            "--now".into(),
            socket_unit.into(),
        ];
        let _ = crate::process::run_piped("systemctl", &enable_args);
        let _ = crate::process::run_piped(
            "systemctl",
            &["--user".into(), "start".into(), host_unit.into()],
        );
    }

    // linger
    if config.lifecycle.autostart {
        let whoami = std::env::var("USER").unwrap_or_default();
        if !whoami.is_empty() && which::which("loginctl").is_ok() {
            let args: Vec<std::ffi::OsString> = vec!["enable-linger".into(), whoami.into()];
            let output = crate::process::run_piped("loginctl", &args)?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("Warning: enable-linger failed: {}", stderr);
            } else {
                println!("Linger enabled for user.");
            }
        }
    }

    Ok(())
}

/// Remove Quadlet and systemd files for a container.
pub fn uninstall(name: &str) -> Result<()> {
    let ver = podman_version().unwrap_or(&PodmanVersion {
        major: 5,
        minor: 5,
        patch: 0,
    });
    let qdir = quadlet_dir();
    let sdir = systemd_user_dir();

    if ver.at_least(5, 6) {
        let args: Vec<std::ffi::OsString> = vec![
            "quadlet".into(),
            "rm".into(),
            format!("{}.container", name).into(),
        ];
        let output = crate::process::run_piped("podman", &args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("podman quadlet rm failed: {}", stderr);
        }
        println!("Quadlet files removed via podman quadlet rm.");

        // Also clean up systemd socket/host-service units
        let socket_path = sdir.join(format!("{}.socket", name));
        let host_path = sdir.join(format!("{}-host.service", name));
        let proxy_path = sdir.join(format!("{}-proxy.service", name));
        for path in [socket_path, host_path, proxy_path] {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }
        // daemon-reload
        if which::which("systemctl").is_ok() {
            let args: Vec<std::ffi::OsString> = vec!["--user".into(), "daemon-reload".into()];
            let _ = crate::process::run_piped("systemctl", &args);
        }
        println!("Systemd units removed.");
    } else {
        // 5.5 fallback: remove files manually
        for ext in ["build", "container"] {
            let path = qdir.join(format!("{}.{}", name, ext));
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }

        let socket_path = sdir.join(format!("{}.socket", name));
        let host_path = sdir.join(format!("{}-host.service", name));
        let proxy_path = sdir.join(format!("{}-proxy.service", name));

        for path in [socket_path, host_path, proxy_path] {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }

        // daemon-reload
        if which::which("systemctl").is_ok() {
            let args: Vec<std::ffi::OsString> = vec!["--user".into(), "daemon-reload".into()];
            if let Err(e) = crate::process::run_piped("systemctl", &args) {
                eprintln!("Warning: daemon-reload failed: {}", e);
            }
        }

        println!("Files for '{}' removed.", name);
    }

    Ok(())
}
