use std::path::PathBuf;

use podbox::config::{self, Config};
use podbox::labels;

fn load_config(name: &str) -> Config {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let content = std::fs::read_to_string(path).unwrap();
    Config::parse(&content).unwrap()
}

// ---- Shorthand resolver ----

#[test]
fn resolve_shorthand_to_full_ref() {
    let ref_str = config::resolve_image_ref("cachy", "ghcr.io", "podbox");
    assert_eq!(ref_str, "ghcr.io/podbox:cachy");
}

#[test]
fn resolve_full_ref_unchanged() {
    let ref_str = config::resolve_image_ref("ghcr.io/user/img:tag", "ghcr.io", "podbox");
    assert_eq!(ref_str, "ghcr.io/user/img:tag");
}

#[test]
fn resolve_tagged_ref_with_domain_unchanged() {
    // `fedora:41` has `:` but prefix `fedora` has no `.` — treated as shorthand.
    // Use a domain-like prefix to keep it as-is:
    let ref_str = config::resolve_image_ref("docker.io/fedora:41", "ghcr.io", "podbox");
    assert_eq!(ref_str, "docker.io/fedora:41");
}

#[test]
fn resolve_shorthand_with_tag() {
    // `fedora:41` — no `/`, prefix `fedora` has no `.`, so it's shorthand + tag
    let ref_str = config::resolve_image_ref("fedora:41", "ghcr.io", "podbox");
    assert_eq!(ref_str, "ghcr.io/podbox:fedora:41");
}

#[test]
fn resolve_shorthand_uses_custom_registry() {
    let ref_str = config::resolve_image_ref("cachy", "docker.io", "myuser/images");
    assert_eq!(ref_str, "docker.io/myuser/images:cachy");
}

#[test]
fn resolve_full_from_config() {
    let config = load_config("prebuilt.toml");
    let ref_str = config::resolve_image_ref(
        &config.image.base,
        &config.image.prebuilt_registry,
        &config.image.prebuilt_repo,
    );
    assert_eq!(ref_str, "ghcr.io/bethropolis/podbox:cachy-latest");
}

// ---- Containerfile prebuilt generation ----

#[test]
fn containerfile_prebuilt_generates_minimal() {
    let config = load_config("prebuilt.toml");
    let cf = podbox::codegen::containerfile::generate(&config, "podbox-guest");
    assert!(cf.starts_with("FROM cachy-latest\n"));
    // Prebuilt Containerfile does NOT COPY the guest binary
    // (the image already has it embedded)
    assert!(!cf.contains("COPY podbox-guest"));
    assert!(cf.contains(r#"ENTRYPOINT ["/usr/local/bin/podbox-guest", "--entry"]"#));
    assert!(cf.contains(r#"CMD ["/usr/bin/fish"]"#));
    assert!(cf.contains("ENV PODBOX_CONTAINER=prebuilt"));
    // Prebuilt should NOT include packages or RUN commands
    assert!(!cf.contains("dnf install"));
    assert!(!cf.contains("RUN "));
}

#[test]
fn containerfile_custom_has_packages() {
    let config = load_config("full.toml");
    let cf = podbox::codegen::containerfile::generate(&config, "podbox-guest");
    assert!(!cf.starts_with("FROM ghcr.io"));
    assert!(cf.contains("dnf install -y"));
    assert!(cf.contains("RUN "));
    // Custom Containerfile should COPY the guest binary
    assert!(cf.contains("COPY podbox-guest /usr/local/bin/podbox-guest"));
}

// ---- Label defaults ----

#[test]
fn label_apply_defaults_prebuilt() {
    let mut config = load_config("prebuilt.toml");
    let labels = std::collections::HashMap::from([
        ("podbox.schema".to_string(), "1".to_string()),
        ("podbox.xdg_dirs.documents".to_string(), "true".to_string()),
        ("podbox.integration.gpu".to_string(), "true".to_string()),
        ("podbox.default_shell".to_string(), "/bin/fish".to_string()),
    ]);
    labels::apply_defaults(&mut config, &labels);
    assert_eq!(config.integration.gpu, podbox::config::GpuMode::Enabled);
    assert!(config.integration.xdg_dirs.documents);
}

#[test]
fn label_apply_defaults_empty_does_not_override() {
    let mut config = load_config("prebuilt.toml");
    let labels = std::collections::HashMap::from([("podbox.schema".to_string(), "1".to_string())]);
    labels::apply_defaults(&mut config, &labels);
    // Should keep existing defaults
    assert_eq!(config.container.shell, "/usr/bin/fish");
    assert_eq!(config.integration.gpu, podbox::config::GpuMode::Auto);
}

#[test]
fn label_apply_defaults_schema_mismatch_returns_early() {
    let mut config = load_config("prebuilt.toml");
    let labels =
        std::collections::HashMap::from([("podbox.protocol_version".to_string(), "2".to_string())]);
    // No podbox.schema label -> apply_defaults returns early
    labels::apply_defaults(&mut config, &labels);
    assert!(config.image.prebuilt);
}

// ---- Quadlet prebuilt Image ref ----

#[test]
fn quadlet_prebuilt_uses_registry_image() {
    use podbox::codegen::quadlet;
    use podbox::env::HostEnv;
    use podbox::xdg::ResolvedXdgDirs;
    let config = load_config("prebuilt.toml");
    let env = HostEnv {
        uid: 1000,
        username: "testuser".into(),
        xdg_runtime_dir: PathBuf::from("/run/user/1000"),
        wayland_display: None,
        wayland_socket: None,
        pipewire_socket: None,
        pulse_dir: None,
        dbus_socket: None,
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
    };
    let xdg = ResolvedXdgDirs {
        documents: None,
        downloads: None,
        pictures: None,
        music: None,
        videos: None,
        desktop: None,
        projects: None,
    };
    let q = quadlet::generate_container(&config, &env, &xdg);
    assert!(q.contains("Image=ghcr.io/bethropolis/podbox:cachy-latest"));
    assert!(!q.contains("Image=prebuilt.build"));
}

#[test]
fn quadlet_custom_uses_build_ref() {
    use podbox::codegen::quadlet;
    use podbox::env::HostEnv;
    use podbox::xdg::ResolvedXdgDirs;
    let cfg = load_config("full.toml");
    let env = HostEnv {
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
    };
    let xdg = ResolvedXdgDirs {
        documents: Some(PathBuf::from("/home/user/Documents")),
        downloads: Some(PathBuf::from("/home/user/Downloads")),
        pictures: None,
        music: None,
        videos: None,
        desktop: None,
        projects: None,
    };
    let q = quadlet::generate_container(&cfg, &env, &xdg);
    assert!(q.contains("Image=myenv.build"));
    assert!(!q.contains("Image=ghcr.io"));
}

#[test]
fn quadlet_has_environment_home() {
    use podbox::codegen::quadlet;
    let config = load_config("full.toml");
    let env = podbox::env::HostEnv {
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
    };
    let xdg = podbox::xdg::ResolvedXdgDirs {
        documents: None,
        downloads: None,
        pictures: None,
        music: None,
        videos: None,
        desktop: None,
        projects: None,
    };
    let q = quadlet::generate_container(&config, &env, &xdg);
    assert!(q.contains("Environment=HOME=/home/%u"));
}

// ---- Profile loading ----

#[test]
fn profile_cachy_parses() {
    let profile = podbox::profiles::find("cachy").expect("cachy profile exists");
    let cfg = Config::parse(&profile.toml).unwrap();
    assert_eq!(cfg.image.base, "cachy-latest");
    assert!(cfg.image.prebuilt);
    assert_eq!(cfg.container.shell, "/usr/bin/fish");
}

#[test]
fn profile_fedora_parses() {
    let profile = podbox::profiles::find("fedora").expect("fedora profile exists");
    let cfg = Config::parse(&profile.toml).unwrap();
    assert_eq!(cfg.image.base, "fedora-latest");
    assert!(cfg.image.prebuilt);
}

#[test]
fn profile_gaming_parses() {
    let profile = podbox::profiles::find("gaming").expect("gaming profile exists");
    let cfg = Config::parse(&profile.toml).unwrap();
    assert_eq!(cfg.image.base, "cachy-latest");
    assert!(cfg.integration.wayland);
    assert_eq!(cfg.integration.gpu, podbox::config::GpuMode::Enabled);
}

#[test]
fn profile_unknown_returns_none() {
    assert!(podbox::profiles::find("nonexistent").is_none());
}

#[test]
fn profile_list_contains_all() {
    let names = podbox::profiles::list_names();
    assert!(names.iter().any(|n| n == "cachy"));
    assert!(names.iter().any(|n| n == "fedora"));
    assert!(names.iter().any(|n| n == "gaming"));
}
