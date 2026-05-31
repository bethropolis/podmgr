use std::path::{Path, PathBuf};

use crate::config::{Config, GpuMode};
use crate::env::HostEnv;
use crate::xdg::ResolvedXdgDirs;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"))
}

/// Generate the `.build` Quadlet file.
pub fn generate_build(config: &Config, containerfile_path: &Path) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("[Build]".into());
    lines.push(format!(
        "ImageTag=localhost/podmgr-{}:latest",
        config.image.name
    ));
    lines.push(format!(
        "File={}",
        containerfile_path.to_string_lossy()
    ));

    lines.join("\n")
}

/// Generate the `.socket` Quadlet file.
pub fn generate_socket(config: &Config) -> String {
    let name = &config.container.name;
    let host_service = format!("{}-host.service", name);
    let mut lines: Vec<String> = Vec::new();

    lines.push("[Unit]".into());
    lines.push(format!(
        "Description=podmgr host-guest socket -- {}",
        name
    ));
    lines.push(String::new());

    lines.push("[Socket]".into());
    lines.push(format!("ListenStream=%t/podmgr/{}.sock", name));
    lines.push(format!("Service={}", host_service));
    lines.push("SocketMode=0600".into());
    lines.push("DirectoryMode=0700".into());
    lines.push(String::new());

    lines.push("[Install]".into());
    lines.push("WantedBy=sockets.target".into());

    lines.join("\n")
}

/// Generate the `.container` Quadlet file.
///
/// Pure function: all paths via HostEnv and ResolvedXdgDirs.
pub fn generate_container(
    config: &Config,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
) -> String {
    let name = &config.container.name;
    let home_in_container = "/home/%u";
    let mut lines: Vec<String> = Vec::new();

    // [Unit]
    lines.push("[Unit]".into());
    lines.push(format!("Description=podmgr -- {}", name));
    lines.push(format!("Requires={}.socket", name));
    lines.push(format!("After={}.socket", name));

    for dep in &config.systemd.requires {
        lines.push(format!("Requires={}", dep));
    }
    for dep in &config.systemd.after {
        lines.push(format!("After={}", dep));
    }

    // D-Bus proxy dependency
    if config.integration.dbus {
        lines.push(format!("Requires={}-proxy.service", name));
        lines.push(format!("After={}-proxy.service", name));
    }

    lines.push(String::new());

    // [Container]
    lines.push("[Container]".into());
    if config.image.prebuilt {
        let ref_str = crate::config::resolve_image_ref_full(config);
        lines.push(format!("Image={}", ref_str));
    } else {
        lines.push(format!("Image=podmgr-{}.build", config.image.name));
    }
    lines.push(format!("ContainerName={}", name));
    lines.push("UserNS=keep-id".into());
    lines.push("SecurityLabelDisable=true".into());
    lines.push(format!("Environment=HOME={}", home_in_container));
    lines.push("Environment=HOST_USER=%u".into());
    lines.push("Environment=HOST_UID=%U".into());
    lines.push("Environment=HOST_GID=%G".into());
    lines.push(String::new());

    // Isolated custom home
    let host_home = config.container.home.to_string_lossy().to_string();
    lines.push(format!(
        "Volume={host_home}:{home_in_container}:Z",
    ));
    lines.push(String::new());

    // Selective XDG dirs
    emit_xdg_dir(&mut lines, "Documents", &xdg.documents, home_in_container);
    emit_xdg_dir(&mut lines, "Downloads", &xdg.downloads, home_in_container);
    emit_xdg_dir(&mut lines, "Pictures", &xdg.pictures, home_in_container);
    emit_xdg_dir(&mut lines, "Music", &xdg.music, home_in_container);
    emit_xdg_dir(&mut lines, "Videos", &xdg.videos, home_in_container);
    emit_xdg_dir(&mut lines, "Desktop", &xdg.desktop, home_in_container);

    if xdg.documents.is_some()
        || xdg.downloads.is_some()
        || xdg.pictures.is_some()
        || xdg.music.is_some()
        || xdg.videos.is_some()
        || xdg.desktop.is_some()
    {
        lines.push(String::new());
    }

    // Visual integration: themes, fonts, icons
    // Skip mounts when the host path doesn't exist.
    if config.integration.sync_themes {
        let h = home();
        if h.join(".themes").exists() {
            lines.push(format!("Volume=%h/.themes:{home_in_container}/.themes:ro"));
        }
        if h.join(".local/share/themes").exists() {
            lines.push(format!("Volume=%h/.local/share/themes:{home_in_container}/.local/share/themes:ro"));
        }
    }
    if config.integration.sync_icons {
        let h = home();
        if h.join(".icons").exists() {
            lines.push(format!("Volume=%h/.icons:{home_in_container}/.icons:ro"));
        }
    }
    if config.integration.sync_fonts {
        let h = home();
        if h.join(".fonts").exists() {
            lines.push(format!("Volume=%h/.fonts:{home_in_container}/.fonts:ro"));
        }
        if h.join(".config/fontconfig").exists() {
            lines.push(format!("Volume=%h/.config/fontconfig:{home_in_container}/.config/fontconfig:ro"));
        }
    }
    if config.integration.sync_themes || config.integration.sync_icons || config.integration.sync_fonts {
        lines.push(String::new());
    }

    // Wayland
    if config.integration.wayland {
        if let Some(ref display) = env.wayland_display {
            lines.push(format!(
                "Environment=WAYLAND_DISPLAY={}",
                display
            ));
            lines.push("Environment=XDG_RUNTIME_DIR=%t".into());
            lines.push("Environment=MOZ_ENABLE_WAYLAND=1".into());
            lines.push(format!(
                "Volume=%t/{}:%t/{}",
                display, display
            ));
            lines.push(String::new());
        }
    }

    // Audio (PipeWire + PulseAudio)
    if config.integration.audio {
        if env.pipewire_socket.is_some() {
            lines.push("Volume=%t/pipewire-0:%t/pipewire-0".into());
            lines.push("Environment=PIPEWIRE_RUNTIME_DIR=%t".into());
        }
        if env.pulse_dir.is_some() {
            lines.push("Volume=%t/pulse:%t/pulse".into());
            lines.push("Environment=PULSE_SERVER=unix:%t/pulse/native".into());
        }
        if env.pipewire_socket.is_some() || env.pulse_dir.is_some() {
            lines.push(String::new());
        }
    }

    // D-Bus (always through proxy, never unfiltered `%t/bus`)
    if config.integration.dbus && env.dbus_socket.is_some() {
        lines.push(format!(
            "Volume=%t/podmgr/{}-dbus.sock:/run/podmgr/dbus.sock:ro",
            name
        ));
        lines.push(
            "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/podmgr/dbus.sock"
                .into(),
        );
        lines.push(String::new());
    }

    // Host-guest socket
    lines.push(format!(
        "Volume=%t/podmgr/{}.sock:%t/podmgr/{}.sock",
        name, name
    ));
    lines.push(String::new());

    // Extra user env
    for (key, value) in &config.container.env {
        lines.push(format!("Environment={}={}", key, value));
    }
    lines.push(format!(
        "Environment=PODMGR_CONTAINER={}",
        name
    ));
    lines.push(String::new());

    // Extra mounts
    for mount in &config.container.mounts.extra {
        lines.push(format!("Volume={}", mount));
    }
    if !config.container.mounts.extra.is_empty() {
        lines.push(String::new());
    }

    // GPU
    match config.integration.gpu {
        GpuMode::Enabled => {
            lines.push("AddDevice=/dev/dri".into());
            lines.push(String::new());
        }
        GpuMode::Nvidia => {
            lines.push("AddDevice=/dev/dri".into());
            lines.push("AddDevice=-/dev/nvidiactl".into());
            lines.push("AddDevice=-/dev/nvidia0".into());
            if env.gpu_has_nvidia_uvm {
                lines.push("AddDevice=-/dev/nvidia-uvm".into());
            }
            lines.push(String::new());
        }
        GpuMode::Auto => {
            if env.gpu_has_dri {
                lines.push("AddDevice=/dev/dri".into());
            }
            if env.gpu_has_nvidia {
                lines.push("AddDevice=-/dev/nvidiactl".into());
                lines.push("AddDevice=-/dev/nvidia0".into());
                if env.gpu_has_nvidia_uvm {
                    lines.push("AddDevice=-/dev/nvidia-uvm".into());
                }
            }
            if env.gpu_has_dri || env.gpu_has_nvidia {
                lines.push(String::new());
            }
        }
        GpuMode::Disabled => {}
    }

    // Auto-update
    if config.lifecycle.auto_update {
        lines.push("Label=io.containers.autoupdate=registry".into());
        lines.push(String::new());
    }

    // Podman init
    lines.push("PodmanArgs=--init".into());
    lines.push(String::new());

    // [Service]
    lines.push("[Service]".into());
    lines.push("Restart=on-failure".into());
    if config.lifecycle.on_stop == crate::config::OnStop::Remove {
        lines.push("AutoRemove=true".into());
    }
    lines.push(String::new());

    // [Install]
    lines.push("[Install]".into());
    if config.lifecycle.autostart {
        lines.push("WantedBy=default.target".into());
    }

    lines.join("\n")
}

/// Generate the companion D-Bus proxy `.service` unit.
///
/// Returns `None` when D-Bus integration is disabled.
pub fn generate_dbus_proxy_service(name: &str, config: &Config) -> Option<String> {
    if !config.integration.dbus {
        return None;
    }

    let mut args = vec![
        "unix:path=%t/bus".to_string(),
        format!("%t/podmgr/{}-dbus.sock", name),
    ];

    // Always filter — no rules means no D-Bus access at all
    args.push("--filter".into());

    for service in &config.dbus.talk {
        args.push(format!("--talk={}", service));
    }
    for service in &config.dbus.own {
        args.push(format!("--own={}", service));
    }

    let exec_start = format!("/usr/bin/xdg-dbus-proxy {}", args.join(" "));

    Some(format!(
        r#"[Unit]
Description=D-Bus Proxy for podmgr container {name}
PartOf={name}.service

[Service]
Type=simple
RuntimeDirectory=podmgr
ExecStart={exec_start}
Restart=on-failure

[Install]
WantedBy={name}.service
"#,
        name = name,
        exec_start = exec_start,
    ))
}

/// Generate the companion host socket server `.service` unit.
///
/// This service is socket-activated by the `.socket` unit and handles
/// guest daemon connections (notifications, xdg-open, clipboard).
pub fn generate_host_service(name: &str) -> String {
    let podmgr_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/usr/local/bin/podmgr".into());

    format!(
        r#"[Unit]
Description=podmgr host socket server -- {name}

[Service]
Type=simple
ExecStart={podmgr_bin} serve {name}
Restart=on-failure
RuntimeDirectory=podmgr

[Install]
WantedBy={name}.socket
"#,
        name = name,
        podmgr_bin = podmgr_bin,
    )
}

fn emit_xdg_dir(
    lines: &mut Vec<String>,
    dir_name: &str,
    host_path: &Option<std::path::PathBuf>,
    container_home: &str,
) {
    if let Some(ref path) = host_path {
        lines.push(format!(
            "Volume={}:{container_home}/{dir_name}:z",
            path.display()
        ));
    }
}
