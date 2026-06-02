use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::error::PodmgrError;

// ---------------------------------------------------------------------------
//  ImageConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageConfig {
    pub base: String,
    pub name: String,
    /// When true, treat `base` as a ready-to-use image.
    /// Skip Containerfile generation and `podman build`.
    /// `packages`, `run`, and other build-time fields are ignored.
    #[serde(default)]
    pub prebuilt: bool,
    /// Registry for shorthand resolution (e.g. "cachy" → "ghcr.io/you/podmgr-images:cachy").
    #[serde(default = "default_prebuilt_registry")]
    pub prebuilt_registry: String,
    /// Repository for shorthand resolution.
    #[serde(default = "default_prebuilt_repo")]
    pub prebuilt_repo: String,
    #[serde(default)]
    pub packages: PackageConfig,
    #[serde(default)]
    pub run: RunConfig,
}

fn default_prebuilt_registry() -> String {
    "ghcr.io".into()
}

fn default_prebuilt_repo() -> String {
    "yourname/podmgr-images".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PackageConfig {
    #[serde(default)]
    pub install: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default = "default_package_manager")]
    pub manager: String,
}

impl Default for PackageConfig {
    fn default() -> Self {
        Self {
            install: Vec::new(),
            remove: Vec::new(),
            manager: default_package_manager(),
        }
    }
}

fn default_package_manager() -> String {
    "dnf".into()
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct RunConfig {
    #[serde(default)]
    pub commands: Vec<String>,
}

// ---------------------------------------------------------------------------
//  ContainerConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContainerConfig {
    pub name: String,
    #[serde(deserialize_with = "deserialize_home")]
    pub home: PathBuf,
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub mounts: MountConfig,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MountConfig {
    #[serde(default)]
    pub extra: Vec<String>,
}

fn default_shell() -> String {
    "fish".into()
}

fn deserialize_home<'de, D>(deserializer: D) -> std::result::Result<PathBuf, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let path = String::deserialize(deserializer)?;
    Ok(expand_tilde(&path))
}

// ---------------------------------------------------------------------------
//  GpuMode
// ---------------------------------------------------------------------------

/// Controls GPU acceleration passthrough to the container.
///
/// Accepts `true`, `false`, `"auto"`, or `"nvidia"` in TOML:
/// - `"auto"` (default) — detects available GPU devices at runtime
/// - `true` / `"true"` — enables `/dev/dri` (Intel/AMD GPU)
/// - `false` / `"false"` — disables all GPU passthrough
/// - `"nvidia"` — enables DRI + NVIDIA device nodes (nvidiactl, nvidia0, nvidia-uvm)
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum GpuMode {
    #[default]
    Auto,
    Enabled,
    Disabled,
    Nvidia,
}

impl<'de> Deserialize<'de> for GpuMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct GpuModeVisitor;

        impl<'de> de::Visitor<'de> for GpuModeVisitor {
            type Value = GpuMode;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("true, false, \"auto\", or \"nvidia\"")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<GpuMode, E> {
                Ok(if v { GpuMode::Enabled } else { GpuMode::Disabled })
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<GpuMode, E> {
                match v {
                    "auto" => Ok(GpuMode::Auto),
                    "nvidia" => Ok(GpuMode::Nvidia),
                    "true" => Ok(GpuMode::Enabled),
                    "false" => Ok(GpuMode::Disabled),
                    _ => Err(de::Error::unknown_variant(v, &["auto", "nvidia", "true", "false"])),
                }
            }
        }

        deserializer.deserialize_any(GpuModeVisitor)
    }
}

impl Serialize for GpuMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            GpuMode::Auto => serializer.serialize_str("auto"),
            GpuMode::Enabled => serializer.serialize_bool(true),
            GpuMode::Disabled => serializer.serialize_bool(false),
            GpuMode::Nvidia => serializer.serialize_str("nvidia"),
        }
    }
}

// ---------------------------------------------------------------------------
//  IntegrationConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IntegrationConfig {
    #[serde(default = "default_true")]
    pub wayland: bool,
    #[serde(default = "default_true")]
    pub audio: bool,
    #[serde(default)]
    pub gpu: GpuMode,
    #[serde(default = "default_true")]
    pub dbus: bool,
    #[serde(default)]
    pub notify: bool,
    #[serde(default)]
    pub xdg_open: bool,
    #[serde(default)]
    pub clipboard: bool,
    /// Bind-mount host font directory (`~/.fonts`) as read-only.
    /// Only top-level directories are mounted to keep `.local` and `.config` writable.
    #[serde(default)]
    pub sync_fonts: bool,
    /// Bind-mount host icon directory (`~/.icons`) as read-only.
    #[serde(default)]
    pub sync_icons: bool,
    /// Bind-mount host theme directory (`~/.themes`) as read-only.
    /// Only top-level directories are mounted to keep `.local` and `.config` writable.
    #[serde(default)]
    pub sync_themes: bool,
    #[serde(default)]
    pub xdg_dirs: XdgDirConfig,
    #[serde(default)]
    pub export: ExportConfig,
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        IntegrationConfig {
            wayland: true,
            audio: true,
            gpu: GpuMode::Auto,
            dbus: true,
            notify: false,
            xdg_open: false,
            clipboard: false,
            sync_fonts: false,
            sync_icons: false,
            sync_themes: false,
            xdg_dirs: XdgDirConfig::default(),
            export: ExportConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct XdgDirConfig {
    #[serde(default)]
    pub documents: bool,
    #[serde(default)]
    pub downloads: bool,
    #[serde(default)]
    pub pictures: bool,
    #[serde(default)]
    pub music: bool,
    #[serde(default)]
    pub videos: bool,
    #[serde(default)]
    pub desktop: bool,
    #[serde(default)]
    pub projects: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ExportConfig {
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub bins: Vec<String>,
}

// ---------------------------------------------------------------------------
//  LifecycleConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LifecycleConfig {
    #[serde(default)]
    pub quadlet: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub on_stop: OnStop,
    #[serde(default)]
    pub auto_update: bool,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        LifecycleConfig {
            quadlet: false,
            autostart: false,
            on_stop: OnStop::Keep,
            auto_update: false,
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OnStop {
    #[default]
    Keep,
    Remove,
}

// ---------------------------------------------------------------------------
//  SystemdConfig
// ---------------------------------------------------------------------------

/// Systemd unit dependency declarations for the generated Quadlet.
///
/// Emitted as `Requires=` and `After=` directives in the `[Unit]` section
/// of the `.container` file, in addition to the automatic socket dependency.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SystemdConfig {
    /// Systemd units that must be active before this container starts.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Systemd units this container should start after.
    #[serde(default)]
    pub after: Vec<String>,
}

// ---------------------------------------------------------------------------
//  DbusConfig
// ---------------------------------------------------------------------------

/// D-Bus access control via `xdg-dbus-proxy`.
///
/// When `integration.dbus = true` and at least one rule is present here,
/// `podmgr` generates a companion `podmgr-proxy-<name>.service` unit that
/// runs `xdg-dbus-proxy` to filter which D-Bus services the container
/// can talk to or own.  Otherwise (or when empty) the container gets
/// unfiltered access to the host session bus via `Volume=%t/bus:%t/bus`.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DbusConfig {
    /// D-Bus services the container is allowed to call (two-way).
    #[serde(default)]
    pub talk: Vec<String>,
    /// D-Bus services the container is allowed to register on the host bus.
    #[serde(default)]
    pub own: Vec<String>,
}

// ---------------------------------------------------------------------------
//  Config (root)
// ---------------------------------------------------------------------------

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
}

impl Config {
    /// Returns `true` when D-Bus integration is active (proxy unit is generated).
    pub fn use_dbus_proxy(&self) -> bool {
        self.integration.dbus && (!self.dbus.talk.is_empty() || !self.dbus.own.is_empty())
    }
}

impl Config {
    /// Parse a TOML definition from a string.
    pub fn parse(content: &str) -> Result<Config> {
        let config: Config = toml::from_str(content)
            .with_context(|| "failed to parse definition file".to_string())?;
        Ok(config)
    }

    /// Load a definition from a file path.
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Err(
                PodmgrError::DefinitionNotFound(path.to_path_buf()).into()
            );
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read definition file '{}'", path.display()))?;
        Self::parse(&content)
    }

    /// Return the built-in embedded default definition.
    pub fn embedded() -> Config {
        Self::parse(EMBEDDED_DEFAULT).expect("embedded default is valid TOML")
    }
}

/// Built-in default definition used when no definition file is found.
pub const EMBEDDED_DEFAULT: &str = r#"
[image]
base = "fedora:44"
name = "podmgr"

[container]
name = "podmgr"
home = "~/containers/podmgr"
"#;

// ---------------------------------------------------------------------------
//  Helpers
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

/// Expand a leading `~` in a path to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Return the podmgr configuration directory (`~/.config/podmgr/`).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("podmgr"))
        .unwrap_or_else(|| PathBuf::from("~/.config/podmgr"))
}

/// Resolve a potentially-shorthand image reference to a full pullable ref.
///
/// - `"cachy"` → `"ghcr.io/yourname/podmgr-images:cachy"`
/// - `"ghcr.io/foo/bar:tag"` → unchanged
/// - `"fedora:41"` → unchanged (contains `:` and a digit)
pub fn resolve_image_ref(base: &str, registry: &str, repo: &str) -> String {
    // Full URI already: contains `/` OR contains `:` + domain-like prefix
    if base.contains('/')
        || (base.contains(':')
            && base.split(':').next().unwrap_or("").contains('.'))
    {
        return base.to_string();
    }
    // Shorthand: "cachy" → "ghcr.io/yourname/podmgr-images:cachy"
    format!(
        "{}/{}:{}",
        registry.trim_end_matches('/'),
        repo.trim_end_matches('/'),
        base
    )
}

pub fn resolve_image_ref_full(config: &Config) -> String {
    resolve_image_ref(&config.image.base, &config.image.prebuilt_registry, &config.image.prebuilt_repo)
}

/// Find a definition file.
///
/// Search order:
/// 1. `./.podmgr.toml`
/// 2. Files in `~/.config/podmgr/` matching `*.toml`
///
/// Returns `None` if no definition file exists anywhere — callers should
/// fall back to [`Config::embedded`].
pub fn find_definition() -> Option<PathBuf> {
    let local = PathBuf::from(".podmgr.toml");
    if local.exists() {
        return Some(local);
    }

    let config_dir = config_dir();

    if config_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&config_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map(|ext| ext == "toml").unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();

        entries.sort();
        if !entries.is_empty() {
            if entries.len() > 1 {
                eprintln!(
                    "Warning: multiple configuration files found in {}. Selecting '{}' alphabetically. Use --config to specify a different file.",
                    config_dir.display(),
                    entries[0].display()
                );
            }
            return Some(entries.remove(0));
        }
    }

    None
}

// ---------------------------------------------------------------------------
//  Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/foo"), home.join("foo"));
        assert_eq!(expand_tilde("~"), home.clone());
        assert_eq!(expand_tilde("/foo/bar"), PathBuf::from("/foo/bar"));
    }

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
        assert!(!cfg.integration.notify);
        assert!(!cfg.integration.xdg_open);
        assert!(!cfg.integration.clipboard);
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
        assert!(cfg.container.home.to_string_lossy().contains("containers/myenv"));
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
        assert!(!cfg.integration.xdg_dirs.documents);
        assert!(!cfg.integration.xdg_dirs.downloads);
        assert!(!cfg.integration.xdg_dirs.pictures);
        assert!(!cfg.integration.xdg_dirs.music);
        assert!(!cfg.integration.xdg_dirs.videos);
        assert!(!cfg.integration.xdg_dirs.desktop);
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
        assert_eq!(cfg.image.name, "podmgr");
        assert_eq!(cfg.container.name, "podmgr");
        assert!(cfg.integration.wayland);
        assert!(cfg.integration.audio);
        assert!(cfg.integration.dbus);
        assert_eq!(cfg.integration.gpu, GpuMode::Auto);
        assert!(!cfg.lifecycle.quadlet);
    }

    #[test]
    fn test_gpu_mode_parses_true() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = true
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Enabled);
    }

    #[test]
    fn test_gpu_mode_parses_false() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = false
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Disabled);
    }

    #[test]
    fn test_gpu_mode_parses_auto_string() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = "auto"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Auto);
    }

    #[test]
    fn test_gpu_mode_parses_nvidia_string() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = "nvidia"
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Nvidia);
    }

    #[test]
    fn test_gpu_mode_serialize() {
        assert_eq!(serde_json::to_string(&GpuMode::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&GpuMode::Enabled).unwrap(), "true");
        assert_eq!(serde_json::to_string(&GpuMode::Disabled).unwrap(), "false");
        assert_eq!(serde_json::to_string(&GpuMode::Nvidia).unwrap(), "\"nvidia\"");
        // Verify TOML serialization works inside a table
        #[derive(Serialize)]
        struct Wrapper {
            gpu: GpuMode,
        }
        let wrapper = Wrapper { gpu: GpuMode::Nvidia };
        let toml_out = toml::to_string(&wrapper).unwrap();
        assert!(toml_out.contains("gpu = \"nvidia\""));
        let wrapper = Wrapper { gpu: GpuMode::Enabled };
        let toml_out = toml::to_string(&wrapper).unwrap();
        assert!(toml_out.contains("gpu = true"));
    }

    #[test]
    fn test_gpu_mode_deserialize_toml_key() {
        let cases = [
            ("gpu = true", GpuMode::Enabled),
            ("gpu = false", GpuMode::Disabled),
            ("gpu = \"auto\"", GpuMode::Auto),
            ("gpu = \"nvidia\"", GpuMode::Nvidia),
        ];
        for (toml_snippet, expected) in &cases {
            let full = format!(
                r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
{}
"#,
                toml_snippet
            );
            let cfg: Config = toml::from_str(&full).unwrap();
            assert_eq!(cfg.integration.gpu, *expected);
        }
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

    #[test]
    fn test_config_load_not_found() {
        let path = std::path::Path::new("/tmp/does_not_exist_XXXXX.toml");
        let result = Config::load(path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.downcast_ref::<PodmgrError>().is_some());
    }

    #[test]
    fn test_dbus_config_defaults_empty() {
        let cfg = Config::embedded();
        assert!(cfg.dbus.talk.is_empty());
        assert!(cfg.dbus.own.is_empty());
        assert!(!cfg.use_dbus_proxy());
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
own = ["org.mpris.MediaPlayer2.podmgr_app"]
"#;
        let cfg = Config::parse(toml).unwrap();
        assert_eq!(
            cfg.dbus.talk,
            vec!["org.freedesktop.Notifications", "org.mpris.MediaPlayer2.*"]
        );
        assert_eq!(cfg.dbus.own, vec!["org.mpris.MediaPlayer2.podmgr_app"]);
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
}
