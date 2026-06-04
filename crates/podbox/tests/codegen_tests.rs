use std::path::PathBuf;

use podbox::codegen::containerfile;
use podbox::codegen::quadlet;
use podbox::config::{Config, GpuMode};
use podbox::env::HostEnv;
use podbox::xdg::ResolvedXdgDirs;

fn load_config(name: &str) -> Config {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let content = std::fs::read_to_string(path).unwrap();
    Config::parse(&content).unwrap()
}

fn default_env() -> HostEnv {
    HostEnv {
        uid: 1000,
        username: "testuser".into(),
        xdg_runtime_dir: PathBuf::from("/run/user/1000"),
        wayland_display: Some("wayland-0".into()),
        wayland_socket: Some(PathBuf::from("/run/user/1000/wayland-0")),
        pipewire_socket: Some(PathBuf::from("/run/user/1000/pipewire-0")),
        pulse_dir: Some(PathBuf::from("/run/user/1000/pulse")),
        dbus_socket: Some(PathBuf::from("/run/user/1000/bus")),
        gpu_has_dri: false,
        gpu_has_nvidia: false,
        gpu_has_nvidia_uvm: false,
        host_has_localtime: false,
        host_has_timezone_file: false,
        host_has_local_share_themes: false,
        host_has_local_share_icons: false,
        host_has_local_share_fonts: false,
        host_shell: None,
        host_locale: None,
    }
}

fn default_xdg() -> ResolvedXdgDirs {
    ResolvedXdgDirs {
        documents: Some(PathBuf::from("/home/user/Documents")),
        downloads: Some(PathBuf::from("/home/user/Downloads")),
        pictures: None,
        music: None,
        videos: None,
        desktop: None,
        projects: None,
    }
}

// ---- Containerfile tests ----

#[test]
fn containerfile_from_line() {
    let config = load_config("full.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.starts_with("FROM fedora:41\n"));
}

#[test]
fn containerfile_copies_guest_binary() {
    let config = load_config("full.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains("COPY podbox-guest /usr/local/bin/podbox-guest"));
}

#[test]
fn containerfile_sets_container_name_env() {
    let config = load_config("full.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains("ENV PODBOX_CONTAINER=myenv"));
}

#[test]
fn containerfile_has_entrypoint() {
    let config = load_config("full.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains(r#"ENTRYPOINT ["/usr/local/bin/podbox-guest", "--entry"]"#));
}

#[test]
fn containerfile_has_packages() {
    let config = load_config("full.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains("dnf install -y"));
    assert!(cf.contains("git"));
    assert!(cf.contains("gcc"));
}

#[test]
fn containerfile_no_packages_when_empty() {
    let config = load_config("minimal.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains("dnf install -y sudo"));
}

#[test]
fn containerfile_has_custom_run_steps() {
    let config = load_config("full.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains("dnf clean all"));
}

// ---- Quadlet .container tests ----

#[test]
fn quadlet_container_has_userns_keep_id() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("UserNS=keep-id"));
}

#[test]
fn quadlet_container_has_security_label_disable() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("SecurityLabelDisable=true"));
}

#[test]
fn quadlet_container_has_init() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("PodmanArgs=--init"));
}

#[test]
fn quadlet_wayland_volume_present_when_enabled() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=%t/wayland-0:%t/wayland-0"));
    assert!(q.contains("Environment=WAYLAND_DISPLAY=wayland-0"));
    assert!(q.contains("Environment=MOZ_ENABLE_WAYLAND=1"));
}

#[test]
fn quadlet_wayland_absent_when_disabled() {
    let config = load_config("no_wayland.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(!q.contains("wayland-0"));
    assert!(!q.contains("WAYLAND_DISPLAY"));
    assert!(!q.contains("MOZ_ENABLE_WAYLAND"));
}

#[test]
fn quadlet_audio_volumes_present() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=%t/pipewire-0:%t/pipewire-0"));
    assert!(q.contains("Volume=%t/pulse:%t/pulse"));
    assert!(q.contains("Environment=PULSE_SERVER=unix:%t/pulse/native"));
}

#[test]
fn quadlet_dbus_present() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    // No rules means unfiltered %t/bus
    assert!(q.contains("Volume=%t/bus:%t/bus"));
    assert!(q.contains("Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=%t/bus"));
    assert!(!q.contains("Volume=%t/podbox/myenv-dbus.sock:/run/podbox/dbus.sock:ro"));
}

#[test]
fn quadlet_xdg_dir_present_when_enabled() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=/home/user/Documents:/home/%u/Documents:z"));
    assert!(q.contains("Volume=/home/user/Downloads:/home/%u/Downloads:z"));
    assert!(q.contains("Environment=HOME=/home/%u"));
    assert!(q.contains("Environment=HOST_USER=testuser"));
    assert!(q.contains("Environment=HOST_UID=%U"));
    assert!(q.contains("Environment=HOST_GID=%G"));
}

#[test]
fn quadlet_xdg_dir_absent_when_disabled() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(!q.contains("Pictures"));
    assert!(!q.contains("Music"));
}

#[test]
fn quadlet_no_host_home_mount() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    let home = dirs::home_dir().unwrap();
    let home_str = home.to_string_lossy();
    // Host home alone must never appear as Volume source
    assert!(!q.contains(&format!("{}:", home_str)));
    // Expanded config.home path is used
    assert!(q.contains(&format!("Volume={}/containers/myenv:/home/%u:Z", home_str)));
}

#[test]
fn quadlet_has_host_guest_socket_volume() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=%t/podbox/myenv.sock:%t/podbox/myenv.sock"));
}

#[test]
fn quadlet_has_extra_env() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Environment=EDITOR=nvim"));
    assert!(q.contains("Environment=TERM=xterm-256color"));
}

#[test]
fn quadlet_gpu_device_when_enabled() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.gpu = GpuMode::Enabled;
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("AddDevice=/dev/dri"));
}

#[test]
fn quadlet_gpu_nvidia() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.gpu = GpuMode::Nvidia;
    let mut env = default_env();
    env.gpu_has_nvidia_uvm = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("AddDevice=/dev/dri"));
    assert!(q.contains("AddDevice=-/dev/nvidiactl"));
    assert!(q.contains("AddDevice=-/dev/nvidia0"));
    assert!(q.contains("AddDevice=-/dev/nvidia-uvm"));
}

#[test]
fn quadlet_gpu_absent_when_disabled() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(!q.contains("AddDevice="));
}

#[test]
fn quadlet_gpu_auto_detects_dri() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.gpu = GpuMode::Auto;
    let mut env = default_env();
    env.gpu_has_dri = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("AddDevice=/dev/dri"));
    assert!(!q.contains("nvidia"));
}

#[test]
fn quadlet_gpu_auto_detects_nvidia() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.gpu = GpuMode::Auto;
    let mut env = default_env();
    env.gpu_has_nvidia = true;
    env.gpu_has_nvidia_uvm = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("AddDevice=-/dev/nvidiactl"));
    assert!(q.contains("AddDevice=-/dev/nvidia0"));
    assert!(q.contains("AddDevice=-/dev/nvidia-uvm"));
    // Should NOT have /dev/dri (no dri detected)
    assert!(!q.contains("AddDevice=/dev/dri"));
}

#[test]
fn quadlet_gpu_auto_nothing_when_no_gpu() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.gpu = GpuMode::Auto;
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(!q.contains("AddDevice="));
}

#[test]
fn quadlet_has_extra_mounts() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=~/Work:/home/user/Work:z"));
}

#[test]
fn quadlet_socket_file_has_listen_stream() {
    let config = load_config("full.toml");
    let q = quadlet::generate_socket(&config);
    assert!(q.contains("ListenStream=%t/podbox/myenv.sock"));
    assert!(q.contains("SocketMode=0600"));
}

#[test]
fn quadlet_build_file_has_image_tag() {
    let config = load_config("full.toml");
    let cf_path = PathBuf::from("/home/user/.local/share/podbox/myenv/Containerfile");
    let q = quadlet::generate_build(&config, &cf_path);
    assert!(q.contains("ImageTag=localhost/podbox-myenv:latest"));
    assert!(q.contains("File=/home/user/.local/share/podbox/myenv/Containerfile"));
}

// ---- Percent specifiers are literal ----

#[test]
fn quadlet_uses_literal_percent_t() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("%t"));
    // %t must NOT be substituted
    assert!(!q.contains("/run/user/1000"));
}

#[test]
fn quadlet_no_literal_percent_h() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    // All home paths use expanded config values, not %h
    assert!(!q.contains("%h"));
}

#[test]
fn quadlet_auto_update_label_present() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.lifecycle.auto_update = true;
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Label=io.containers.autoupdate=registry"));
}

#[test]
fn quadlet_auto_update_label_absent() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(!q.contains("io.containers.autoupdate"));
}

#[test]
fn quadlet_systemd_dependencies() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.systemd.requires = vec!["db-container.service".into()];
    config.systemd.after = vec!["db-container.service".into()];
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Requires=db-container.service"));
    assert!(q.contains("After=db-container.service"));
}

#[test]
fn quadlet_systemd_dependencies_absent_by_default() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    let requires_lines: Vec<&str> = q.lines().filter(|l| l.starts_with("Requires=")).collect();
    // Socket Requires should be present, proxy service absent
    assert_eq!(requires_lines.len(), 1);
    assert!(requires_lines.iter().any(|l| l.ends_with(".socket")));
}

#[test]
fn quadlet_visual_themes_present() {
    // Create host dirs so the existence check passes
    let home = dirs::home_dir().unwrap();
    std::fs::create_dir_all(home.join(".local/share/themes")).ok();
    let _ = std::fs::create_dir_all(home.join(".themes")).ok();
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.sync_themes = true;
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=%h/.themes:/home/%u/.themes:ro"));
}

#[test]
fn quadlet_visual_icons_present() {
    let home = dirs::home_dir().unwrap();
    let _ = std::fs::create_dir_all(home.join(".icons")).ok();
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.sync_icons = true;
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=%h/.icons:/home/%u/.icons:ro"));
}

#[test]
fn quadlet_visual_fonts_present() {
    let home = dirs::home_dir().unwrap();
    let _ = std::fs::create_dir_all(home.join(".fonts")).ok();
    let _ = std::fs::create_dir_all(home.join(".config/fontconfig")).ok();
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.sync_fonts = true;
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Volume=%h/.fonts:/home/%u/.fonts:ro"));
}

#[test]
fn quadlet_visual_mounts_absent_by_default() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(!q.contains("Volume=%h/.themes"));
    assert!(!q.contains("Volume=%h/.icons"));
    assert!(!q.contains("Volume=%h/.fonts"));
    assert!(!q.contains("/home/%u/.themes"));
    assert!(!q.contains("/home/%u/.icons"));
    assert!(!q.contains("/home/%u/.fonts"));
}

#[test]
fn quadlet_dbus_proxy_when_configured() {
    let mut config = load_config("full.toml");
    config.dbus.talk = vec!["org.freedesktop.Notifications".into()];
    config.dbus.own = vec!["org.mpris.MediaPlayer2.podbox_app".into()];
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    // Proxy socket always, never unfiltered %t/bus
    assert!(q.contains("Volume=%t/podbox/myenv-dbus.sock:/run/podbox/dbus.sock:ro"));
    assert!(q.contains("Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/podbox/dbus.sock"));
    assert!(!q.contains("Volume=%t/bus:%t/bus"));
}

#[test]
fn quadlet_dbus_proxy_deps_in_unit() {
    let mut config = load_config("full.toml");
    config.dbus.talk = vec!["org.freedesktop.Notifications".into()];
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Requires=myenv-proxy.service"));
    assert!(q.contains("After=myenv-proxy.service"));
}

#[test]
fn quadlet_dbus_proxy_unit_generated() {
    let mut config = load_config("full.toml");
    config.dbus.talk = vec!["org.freedesktop.Notifications".into()];
    config.dbus.own = vec!["org.mpris.MediaPlayer2.podbox_app".into()];
    let unit = quadlet::generate_dbus_proxy_service("myenv", &config)
        .expect("proxy service should be generated");
    assert!(unit.contains("[Unit]"));
    assert!(unit.contains("Description=D-Bus Proxy for podbox container myenv"));
    assert!(unit.contains("PartOf=myenv.service"));
    assert!(unit.contains("[Service]"));
    assert!(unit.contains("RuntimeDirectory=podbox"));
    assert!(unit.contains("/usr/bin/xdg-dbus-proxy"));
    assert!(unit.contains("--filter"));
    assert!(unit.contains("--talk=org.freedesktop.Notifications"));
    assert!(unit.contains("--own=org.mpris.MediaPlayer2.podbox_app"));
    assert!(unit.contains("%t/podbox/myenv-dbus.sock"));
    assert!(unit.contains("[Install]"));
    assert!(unit.contains("WantedBy=myenv.service"));
}

#[test]
fn quadlet_dbus_proxy_unit_none_when_dbus_disabled() {
    let mut config = load_config("full.toml");
    config.integration.dbus = false;
    assert!(quadlet::generate_dbus_proxy_service("myenv", &config).is_none());
}

#[test]
fn quadlet_dbus_proxy_unit_generated_when_dbus_enabled() {
    let mut config = load_config("full.toml");
    config.dbus.talk = vec!["org.freedesktop.Notifications".into()];
    let unit = quadlet::generate_dbus_proxy_service("myenv", &config)
        .expect("proxy service should be generated when dbus enabled");
    assert!(unit.contains("--filter"));
    assert!(unit.contains("--talk="));
    assert!(!unit.contains("--own="));
}

#[test]
fn quadlet_pipewire_runtime_dir() {
    let config = load_config("full.toml");
    let q = quadlet::generate_container(&config, &default_env(), &default_xdg());
    assert!(q.contains("Environment=PIPEWIRE_RUNTIME_DIR=%t"));
}

#[test]
fn quadlet_gpu_nvidia_without_uvm() {
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.gpu = GpuMode::Nvidia;
    let mut env = default_env();
    env.gpu_has_nvidia_uvm = false;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("AddDevice=/dev/dri"));
    assert!(q.contains("AddDevice=-/dev/nvidiactl"));
    assert!(q.contains("AddDevice=-/dev/nvidia0"));
    assert!(!q.contains("nvidia-uvm"));
}

#[test]
fn quadlet_timezone_mount_present_when_localtime_exists() {
    let config = load_config("full.toml");
    let mut env = default_env();
    env.host_has_localtime = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("Volume=/etc/localtime:/etc/localtime:ro"));
}

#[test]
fn quadlet_timezone_file_mount_present_when_exists() {
    let config = load_config("full.toml");
    let mut env = default_env();
    env.host_has_timezone_file = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("Volume=/etc/timezone:/etc/timezone:ro"));
}

#[test]
fn quadlet_timezone_mounts_absent_when_unavailable() {
    let config = load_config("full.toml");
    let env = default_env();
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(!q.contains("/etc/localtime"));
    assert!(!q.contains("/etc/timezone"));
}

#[test]
fn quadlet_modern_theme_path_present() {
    let home = dirs::home_dir().unwrap();
    std::fs::create_dir_all(home.join(".local/share/themes")).ok();
    let _ = std::fs::create_dir_all(home.join(".themes")).ok();
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.sync_themes = true;
    let mut env = default_env();
    env.host_has_local_share_themes = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("Volume=%h/.themes:/home/%u/.themes:ro"));
    assert!(q.contains("Volume=%h/.local/share/themes:/home/%u/.local/share/themes:ro"));
}

#[test]
fn quadlet_modern_icon_path_present() {
    let home = dirs::home_dir().unwrap();
    let _ = std::fs::create_dir_all(home.join(".icons")).ok();
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.sync_icons = true;
    let mut env = default_env();
    env.host_has_local_share_icons = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("Volume=%h/.icons:/home/%u/.icons:ro"));
    assert!(q.contains("Volume=%h/.local/share/icons:/home/%u/.local/share/icons:ro"));
}

#[test]
fn quadlet_modern_font_path_present() {
    let home = dirs::home_dir().unwrap();
    let _ = std::fs::create_dir_all(home.join(".fonts")).ok();
    let _ = std::fs::create_dir_all(home.join(".config/fontconfig")).ok();
    let config = load_config("full.toml");
    let mut config = config.clone();
    config.integration.sync_fonts = true;
    let mut env = default_env();
    env.host_has_local_share_fonts = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("Volume=%h/.fonts:/home/%u/.fonts:ro"));
    assert!(q.contains("Volume=%h/.local/share/fonts:/home/%u/.local/share/fonts:ro"));
}

#[test]
fn quadlet_modern_paths_absent_when_disabled() {
    let config = load_config("full.toml");
    let mut env = default_env();
    env.host_has_local_share_themes = true;
    env.host_has_local_share_icons = true;
    env.host_has_local_share_fonts = true;
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(!q.contains(".local/share/themes"));
    assert!(!q.contains(".local/share/icons"));
    assert!(!q.contains(".local/share/fonts"));
}

#[test]
fn quadlet_locale_env_vars_present() {
    let config = load_config("full.toml");
    let mut env = default_env();
    env.host_locale = Some("en_US.UTF-8".into());
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(q.contains("Environment=LANG=en_US.UTF-8"));
    assert!(q.contains("Environment=LC_ALL=en_US.UTF-8"));
    assert!(q.contains("Environment=LC_CTYPE=en_US.UTF-8"));
}

#[test]
fn quadlet_locale_env_vars_absent_when_unset() {
    let config = load_config("full.toml");
    let env = default_env();
    let q = quadlet::generate_container(&config, &env, &default_xdg());
    assert!(!q.contains("Environment=LANG="));
    assert!(!q.contains("Environment=LC_ALL="));
}

#[test]
fn containerfile_includes_base_packages() {
    let config = load_config("minimal.toml");
    let cf = containerfile::generate(&config, "podbox-guest");
    assert!(cf.contains("sudo"));
    assert!(cf.contains("curl"));
    assert!(cf.contains("tar"));
    assert!(cf.contains("wget"));
    assert!(cf.contains("coreutils"));
    assert!(cf.contains("diffutils"));
    assert!(cf.contains("findutils"));
    assert!(cf.contains("grep"));
    assert!(cf.contains("sed"));
    assert!(cf.contains("gawk"));
}
