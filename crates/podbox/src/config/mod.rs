use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::error::PodboxError;

pub mod defaults;
pub mod enums;
pub mod fs;
pub mod types;
pub mod validation;

pub use defaults::EMBEDDED_DEFAULT;
pub use enums::{GpuMode, ImageSource, OnStop, XdgDirValue};
pub use fs::{
    active_context_path, clear_active_context, config_dir, expand_tilde, find_definition,
    list_configs, read_active_context, write_active_context,
};
pub use types::{
    ContainerConfig, DbusConfig, ExportConfig, HostExecConfig, ImageConfig, IntegrationConfig,
    LifecycleConfig, MountConfig, PackageConfig, RunConfig, SecurityConfig, SystemdConfig,
    XdgDirConfig,
};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub image: ImageConfig,
    pub container: ContainerConfig,
    #[serde(default)]
    pub integration: IntegrationConfig,
    #[serde(default)]
    pub lifecycle: LifecycleConfig,
    #[serde(default)]
    pub systemd: SystemdConfig,
    #[serde(default)]
    pub dbus: DbusConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

impl Config {
    pub fn use_dbus_proxy(&self) -> bool {
        self.integration.dbus
            && (!self.dbus.effective_talk().is_empty() || !self.dbus.own.is_empty())
    }

    pub fn parse(content: &str) -> Result<Config> {
        let mut config: Config = toml::from_str(content)
            .with_context(|| "failed to parse definition file".to_string())?;
        config.apply_defaults();
        config.validate()?;
        Ok(config)
    }

    pub fn load(path: &std::path::Path) -> Result<Config> {
        if !path.exists() {
            return Err(PodboxError::DefinitionNotFound(path.to_path_buf()).into());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read definition file '{}'", path.display()))?;
        Self::parse(&content)
    }

    pub fn embedded() -> Config {
        Self::parse(EMBEDDED_DEFAULT).expect("embedded default is valid TOML")
    }

    fn apply_defaults(&mut self) {
        if self.integration.dbus
            && self.dbus.preset.is_empty()
            && self.dbus.talk.is_empty()
            && self.dbus.own.is_empty()
        {
            self.dbus.preset = "portal".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_minimal() {
        let toml = r#"
[image]
base = "fedora:41"
name = "myenv"

[container]
name = "myenv"
home = "~/containers/myenv"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.image.base, "fedora:41");
        assert_eq!(cfg.image.name, "myenv");
        assert_eq!(cfg.container.name, "myenv");
        assert_eq!(cfg.container.shell, "fish");
        assert_eq!(cfg.integration.gpu, GpuMode::Auto);
        assert!(cfg.integration.wayland);
        assert!(cfg.integration.audio);
        assert!(cfg.integration.dbus);
        assert!(cfg.integration.notify);
        assert!(cfg.integration.xdg_open);
        assert!(cfg.integration.clipboard);
        assert!(!cfg.integration.host_exec.enabled);
        assert!(cfg.integration.host_exec.allowlist.is_none());
        assert!(!cfg.integration.ssh_agent);
    }

    #[test]
    fn test_home_tilde_expanded() {
        let toml = r#"
[image]
base = "fedora:41"
name = "myenv"

[container]
name = "myenv"
home = "~/containers/myenv"
"#;
        let cfg = Config::parse(toml).unwrap();
        let home = dirs::home_dir().unwrap();
        assert!(cfg.container.home.starts_with(&home));
        assert!(cfg
            .container
            .home
            .to_string_lossy()
            .contains("containers/myenv"));
    }

    #[test]
    fn test_on_stop_defaults_to_keep() {
        let toml = r#"
[image]
base = "fedora:41"
name = "myenv"

[container]
name = "myenv"
home = "~/containers/myenv"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.lifecycle.on_stop, OnStop::Keep);
    }

    #[test]
    fn test_xdg_dirs_default_all_false() {
        let toml = r#"
[image]
base = "fedora:41"
name = "myenv"

[container]
name = "myenv"
home = "~/containers/myenv"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert!(!cfg.integration.xdg_dirs.documents.is_enabled());
        assert!(!cfg.integration.xdg_dirs.downloads.is_enabled());
        assert!(!cfg.integration.xdg_dirs.pictures.is_enabled());
        assert!(!cfg.integration.xdg_dirs.music.is_enabled());
        assert!(!cfg.integration.xdg_dirs.videos.is_enabled());
        assert!(!cfg.integration.xdg_dirs.desktop.is_enabled());
    }

    #[test]
    fn test_wayland_default_is_true() {
        let toml = r#"
[image]
base = "fedora:41"
name = "myenv"

[container]
name = "myenv"
home = "~/containers/myenv"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert!(cfg.integration.wayland);
        assert!(cfg.integration.audio);
    }

    #[test]
    fn test_embedded_default_parses() {
        let cfg = Config::embedded();
        assert_eq!(cfg.image.base, "fedora:44");
        assert_eq!(cfg.image.name, "podbox");
        assert_eq!(cfg.container.name, "podbox");
        assert!(cfg.integration.wayland);
        assert!(cfg.integration.audio);
        assert!(cfg.integration.dbus);
        assert_eq!(cfg.integration.gpu, GpuMode::Auto);
        assert!(!cfg.lifecycle.quadlet);
    }

    #[test]
    fn test_config_load_not_found() {
        let path = std::path::Path::new("/tmp/does_not_exist_XXXXX.toml");
        let result = Config::load(path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.downcast_ref::<PodboxError>().is_some());
    }

    #[test]
    fn test_systemd_config_parses() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[systemd]
requires = ["db.service", "cache.service"]
after = ["network.target"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.systemd.requires, vec!["db.service", "cache.service"]);
        assert_eq!(cfg.systemd.after, vec!["network.target"]);
    }

    #[test]
    fn test_visual_config_parses() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
sync_themes = true
sync_icons = true
sync_fonts = true
"#;
        let cfg = Config::parse(toml).unwrap();
        assert!(cfg.integration.sync_themes);
        assert!(cfg.integration.sync_icons);
        assert!(cfg.integration.sync_fonts);
    }

    #[test]
    fn test_dbus_config_defaults_empty() {
        let cfg = Config::embedded();
        assert_eq!(cfg.dbus.preset, "portal");
        assert_eq!(cfg.dbus.effective_talk(), vec!["org.freedesktop.portal.*"]);
        assert!(cfg.use_dbus_proxy());
    }

    #[test]
    fn test_dbus_config_parses_talk_own() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[dbus]
talk = ["org.freedesktop.Notifications", "org.mpris.MediaPlayer2.*"]
own = ["org.mpris.MediaPlayer2.podbox_app"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(
            cfg.dbus.talk,
            vec!["org.freedesktop.Notifications", "org.mpris.MediaPlayer2.*"]
        );
        assert_eq!(cfg.dbus.own, vec!["org.mpris.MediaPlayer2.podbox_app"]);
        assert!(cfg.use_dbus_proxy());
    }

    #[test]
    fn test_dbus_config_talk_only() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[dbus]
talk = ["org.freedesktop.Notifications"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.dbus.talk.len(), 1);
        assert!(cfg.dbus.own.is_empty());
        assert!(cfg.use_dbus_proxy());
    }

    #[test]
    fn test_dbus_config_own_only() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[dbus]
own = ["org.example.Service"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert!(cfg.dbus.talk.is_empty());
        assert_eq!(cfg.dbus.own.len(), 1);
        assert!(cfg.use_dbus_proxy());
    }

    #[test]
    fn test_invalid_toml_errors() {
        let toml = r#"
[image
base = "fedora:41"
"#;
        assert!(Config::parse(toml).is_err());
    }

    #[test]
    fn test_missing_required_fields_errors() {
        let toml = r#"
[image]
base = "fedora:41"
"#;
        assert!(Config::parse(toml).is_err());
    }
}
