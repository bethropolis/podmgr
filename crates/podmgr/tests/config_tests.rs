use std::path::PathBuf;

use podmgr::config::Config;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn parses_full_config() {
    let path = fixtures_dir().join("full.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert_eq!(cfg.image.base, "fedora:41");
    assert_eq!(cfg.image.name, "myenv");
    assert_eq!(cfg.container.name, "myenv");
    assert_eq!(cfg.container.shell, "fish");
}

#[test]
fn home_tilde_is_expanded() {
    let path = fixtures_dir().join("full.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    let home = dirs::home_dir().unwrap();
    assert!(cfg.container.home.starts_with(&home));
    assert!(cfg.container.home.to_string_lossy().contains("containers/myenv"));
}

#[test]
fn parses_minimal_config() {
    let path = fixtures_dir().join("minimal.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert_eq!(cfg.image.base, "fedora:41");
    assert_eq!(cfg.container.name, "minimal");
    assert_eq!(cfg.container.shell, "fish");
    assert_eq!(cfg.integration.gpu, podmgr::config::GpuMode::Auto);
    assert!(cfg.integration.wayland);
    assert!(cfg.integration.audio);
    assert!(cfg.integration.dbus);
}

#[test]
fn on_stop_defaults_to_keep() {
    let path = fixtures_dir().join("minimal.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    use podmgr::config::OnStop;
    assert_eq!(cfg.lifecycle.on_stop, OnStop::Keep);
}

#[test]
fn xdg_dirs_default_all_false() {
    let path = fixtures_dir().join("minimal.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert!(!cfg.integration.xdg_dirs.documents);
    assert!(!cfg.integration.xdg_dirs.downloads);
    assert!(!cfg.integration.xdg_dirs.pictures);
    assert!(!cfg.integration.xdg_dirs.music);
    assert!(!cfg.integration.xdg_dirs.videos);
    assert!(!cfg.integration.xdg_dirs.desktop);
}

#[test]
fn wayland_default_is_true() {
    let path = fixtures_dir().join("minimal.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert!(cfg.integration.wayland);
    assert!(cfg.integration.audio);
}

#[test]
fn no_wayland_config() {
    let path = fixtures_dir().join("no_wayland.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert!(!cfg.integration.wayland);
    assert!(!cfg.integration.audio);
    assert!(!cfg.integration.dbus);
}

#[test]
fn full_config_packages() {
    let path = fixtures_dir().join("full.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert_eq!(cfg.image.packages.install.len(), 5);
    assert!(cfg.image.packages.install.contains(&"git".into()));
    assert!(cfg.image.packages.install.contains(&"gcc".into()));
}

#[test]
fn full_config_env() {
    let path = fixtures_dir().join("full.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert_eq!(cfg.container.env.get("EDITOR"), Some(&"nvim".into()));
    assert_eq!(cfg.container.env.get("TERM"), Some(&"xterm-256color".into()));
}

#[test]
fn full_config_export() {
    let path = fixtures_dir().join("full.toml");
    let content = std::fs::read_to_string(path).unwrap();
    let cfg = Config::parse(&content).unwrap();

    assert_eq!(cfg.integration.export.apps, vec!["gedit", "nautilus"]);
    assert_eq!(cfg.integration.export.bins, vec!["rg", "gcc"]);
}
