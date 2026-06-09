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
        "ImageTag=localhost/podbox-{}:latest",
        config.image.name
    ));
    lines.push(format!("File={}", containerfile_path.to_string_lossy()));
    lines.push(format!("Retry={}", config.image.pull_retry));
    lines.push(format!("RetryDelay={}", config.image.pull_retry_delay));

    lines.join("\n")
}

/// Generate the `.socket` Quadlet file.
pub fn generate_socket(config: &Config) -> String {
    let name = &config.container.name;
    let host_service = format!("{}-host.service", name);
    let mut lines: Vec<String> = Vec::new();

    lines.push("[Unit]".into());
    lines.push(format!("Description=podbox host-guest socket -- {}", name));
    lines.push(String::new());

    lines.push("[Socket]".into());
    lines.push(format!("ListenStream=%t/podbox/{}.sock", name));
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
pub fn generate_container(config: &Config, env: &HostEnv, xdg: &ResolvedXdgDirs) -> String {
    let name = &config.container.name;
    let home_in_container = "/home/%u";
    let mut lines: Vec<String> = Vec::new();

    // [Unit]
    lines.push("[Unit]".into());
    lines.push(format!("Description=podbox -- {}", name));
    lines.push(format!("Requires={}.socket", name));
    lines.push(format!("After={}.socket", name));

    for dep in &config.systemd.requires {
        lines.push(format!("Requires={}", dep));
    }
    for dep in &config.systemd.after {
        lines.push(format!("After={}", dep));
    }

    // D-Bus proxy dependency
    if config.use_dbus_proxy() {
        lines.push(format!("Requires={}-proxy.service", name));
        lines.push(format!("After={}-proxy.service", name));
    }

    lines.push(String::new());

    // [Container]
    lines.push("[Container]".into());
    if config.image.source().is_prebuilt() && config.image.packages.install.is_empty() {
        // No packages to install — reference the prebuilt image directly.
        let ref_str = match config.image.source() {
            crate::config::ImageSource::Prebuilt { ref_str } => ref_str,
            _ => config.image.base.clone(),
        };
        lines.push(format!("Image={}", ref_str));
        lines.push(format!("Retry={}", config.image.pull_retry));
        lines.push(format!("RetryDelay={}", config.image.pull_retry_delay));
    } else {
        // Packages to install (or custom build) — use the local overlay image.
        lines.push(format!(
            "Image=localhost/podbox-{}:latest",
            config.image.name
        ));
    }
    lines.push(format!("ContainerName={}", name));
    lines.push("UserNS=keep-id".into());
    lines.push("User=root".into());
    if config.security.security_label_disable {
        lines.push("SecurityLabelDisable=true".into());
    }
    if let Some(ref seccomp) = config.security.seccomp {
        lines.push(format!("SeccompProfile={}", seccomp));
    }
    if !config.security.no_new_privileges {
        lines.push("NoNewPrivileges=false".into());
    }
    if let Some(ref mem) = config.container.memory {
        lines.push(format!("Memory={}", mem));
    }
    if let Some(ref profile) = config.security.apparmor {
        lines.push(format!("AppArmor={}", profile));
    }
    lines.push(format!("Environment=HOME={}", home_in_container));
    lines.push(format!("Environment=HOST_USER={}", env.username));
    lines.push("Environment=HOST_UID=%U".into());
    lines.push("Environment=HOST_GID=%G".into());
    lines.push("Environment=PATH=/run/podbox/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into());
    lines.push(String::new());

    // Isolated custom home
    let host_home = config.container.home.to_string_lossy().to_string();
    lines.push(format!("Volume={host_home}:{home_in_container}:Z",));
    lines.push(String::new());

    // Selective XDG dirs
    emit_xdg_dir(&mut lines, "Documents", &xdg.documents, home_in_container);
    emit_xdg_dir(&mut lines, "Downloads", &xdg.downloads, home_in_container);
    emit_xdg_dir(&mut lines, "Pictures", &xdg.pictures, home_in_container);
    emit_xdg_dir(&mut lines, "Music", &xdg.music, home_in_container);
    emit_xdg_dir(&mut lines, "Videos", &xdg.videos, home_in_container);
    emit_xdg_dir(&mut lines, "Desktop", &xdg.desktop, home_in_container);
    emit_xdg_dir(&mut lines, "Projects", &xdg.projects, home_in_container);

    if xdg.documents.is_some()
        || xdg.downloads.is_some()
        || xdg.pictures.is_some()
        || xdg.music.is_some()
        || xdg.videos.is_some()
        || xdg.desktop.is_some()
        || xdg.projects.is_some()
    {
        lines.push(String::new());
    }

    // Visual integration: themes, fonts, icons
    if config.integration.sync_themes {
        let h = home();
        if h.join(".themes").exists() {
            lines.push(format!("Volume=%h/.themes:{home_in_container}/.themes:ro"));
        }
        if env.host_has_local_share_themes {
            lines.push(format!(
                "Volume=%h/.local/share/themes:{home_in_container}/.local/share/themes:ro"
            ));
        }
    }
    if config.integration.sync_icons {
        let h = home();
        if h.join(".icons").exists() {
            lines.push(format!("Volume=%h/.icons:{home_in_container}/.icons:ro"));
        }
        if env.host_has_local_share_icons {
            lines.push(format!(
                "Volume=%h/.local/share/icons:{home_in_container}/.local/share/icons:ro"
            ));
        }
    }
    if config.integration.sync_fonts {
        let h = home();
        if h.join(".fonts").exists() {
            lines.push(format!("Volume=%h/.fonts:{home_in_container}/.fonts:ro"));
        }
        if env.host_has_local_share_fonts {
            lines.push(format!(
                "Volume=%h/.local/share/fonts:{home_in_container}/.local/share/fonts:ro"
            ));
        }
    }
    if config.integration.sync_themes
        || config.integration.sync_icons
        || config.integration.sync_fonts
    {
        lines.push(String::new());
    }

    // Timezone sync
    if env.host_has_localtime {
        lines.push("Volume=/etc/localtime:/etc/localtime:ro".into());
    }
    if env.host_has_timezone_file {
        lines.push("Volume=/etc/timezone:/etc/timezone:ro".into());
    }
    if env.host_has_localtime || env.host_has_timezone_file {
        lines.push(String::new());
    }

    // Locale environment
    if let Some(ref locale) = env.host_locale {
        lines.push(format!("Environment=LANG={}", locale));
        lines.push(format!("Environment=LC_ALL={}", locale));
        lines.push(format!("Environment=LC_CTYPE={}", locale));
        lines.push(String::new());
    }

    // Wayland
    if config.integration.wayland {
        if let Some(ref display) = env.wayland_display {
            lines.push(format!("Environment=WAYLAND_DISPLAY={}", display));
            lines.push("Environment=XDG_RUNTIME_DIR=%t".into());
            lines.push("Environment=MOZ_ENABLE_WAYLAND=1".into());
            lines.push(format!("Volume=%t/{}:%t/{}", display, display));
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

    // SSH agent
    if config.integration.ssh_agent {
        let ver = crate::podman::podman_version().ok();
        if ver.is_some_and(|v| v.at_least(5, 6)) {
            lines.push("SshAgent=default".into());
            lines.push("Environment=SSH_AUTH_SOCK=/run/podbox/ssh-agent.sock".into());
        } else {
            eprintln!("Warning: ssh_agent = true requires Podman >= 5.6 for SSH_AUTH_SOCK passthrough. Skipping SSH agent.");
        }
        lines.push(String::new());
    }

    // GPG agent
    if config.integration.gpg_agent {
        if let Some(ref sock) = env.gpg_agent_socket {
            lines.push(format!(
                "Volume={}:/run/podbox/gnupg/S.gpg-agent:ro",
                sock.display()
            ));
            lines.push("Environment=GPG_TTY=/dev/pts/0".into());
            lines.push("Environment=GNUPGHOME=/run/podbox/gnupg".into());
        } else {
            eprintln!("Warning: gpg_agent = true but S.gpg-agent socket not found on host. Skipping GPG agent.");
        }
        lines.push(String::new());
    }

    // D-Bus
    if config.integration.dbus && env.dbus_socket.is_some() {
        if config.use_dbus_proxy() {
            lines.push(format!(
                "Volume=%t/podbox/{}-dbus.sock:/run/podbox/dbus.sock:ro",
                name
            ));
            lines.push(
                "Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/podbox/dbus.sock".into(),
            );
        } else {
            lines.push("Volume=%t/bus:%t/bus".into());
            lines.push("Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=%t/bus".into());
        }
        lines.push(String::new());
    }

    // Host-guest socket
    lines.push(format!(
        "Volume=%t/podbox/{}.sock:%t/podbox/{}.sock",
        name, name
    ));
    lines.push(String::new());

    // Extra user env
    for (key, value) in &config.container.env {
        if key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            let clean = value.replace('\n', " ").replace('\r', "");
            let escaped = clean.replace('\\', "\\\\").replace('"', "\\\"");
            let env_val = if escaped.contains(' ') || escaped.is_empty() {
                format!("\"{}\"", escaped)
            } else {
                escaped
            };
            lines.push(format!("Environment={}={}", key, env_val));
        } else {
            eprintln!(
                "Warning: ignoring invalid environment variable key '{}'",
                key
            );
        }
    }
    lines.push(format!("Environment=PODBOX_CONTAINER={}", name));
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
        if config.image.source().is_prebuilt() {
            lines.push("Label=io.containers.autoupdate=registry".into());
        } else {
            eprintln!(
                "Warning: auto_update is true for '{}' but the image is built \
                 from source. Auto-update only works with prebuilt images.",
                config.container.name
            );
        }
        lines.push(String::new());
    }

    // Podman init + working directory
    lines.push("PodmanArgs=--init".into());
    lines.push("PodmanArgs=--workdir=/home/%u".into());
    lines.push(String::new());

    // Reload command
    if let Some(ref cmd) = config.container.reload_cmd {
        lines.push(format!("ReloadCmd={}", cmd));
        lines.push(String::new());
    }

    // [Service]
    lines.push("[Service]".into());
    lines.push("Restart=on-failure".into());
    lines.push("RestartSec=2s".into());
    lines.push("StartLimitBurst=5".into());
    lines.push("StartLimitIntervalSec=30s".into());
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
pub fn generate_dbus_proxy_service(name: &str, config: &Config) -> Option<String> {
    if !config.use_dbus_proxy() {
        return None;
    }

    let mut args = vec![
        "unix:path=%t/bus".to_string(),
        format!("%t/podbox/{}-dbus.sock", name),
    ];

    args.push("--filter".into());

    for service in &config.dbus.effective_talk() {
        args.push(format!("--talk={}", service));
    }
    for service in &config.dbus.own {
        args.push(format!("--own={}", service));
    }

    let exec_start = format!("/usr/bin/xdg-dbus-proxy {}", args.join(" "));

    Some(format!(
        r#"[Unit]
Description=D-Bus Proxy for podbox container {name}
PartOf={name}.service

[Service]
Type=simple
RuntimeDirectory=podbox
ExecStart={exec_start}
Restart=on-failure
RestartSec=1s

[Install]
WantedBy={name}.service
"#,
        name = name,
        exec_start = exec_start,
    ))
}

/// Generate the companion host socket server `.service` unit.
pub fn generate_host_service(name: &str) -> String {
    let podbox_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/usr/local/bin/podbox".into());

    format!(
        r#"[Unit]
Description=podbox host socket server -- {name}

[Service]
Type=simple
ExecStart={podbox_bin} serve {name}
Restart=on-failure
RestartSec=2s
RuntimeDirectory=podbox

[Install]
WantedBy={name}.socket
"#,
        name = name,
        podbox_bin = podbox_bin,
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
