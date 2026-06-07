use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::error::PodboxError;

// ---------------------------------------------------------------------------
//  ImageConfig
// ---------------------------------------------------------------------------

/// Distinguishes how `podbox` should obtain the container image.
///
/// `Build` produces a new image from a base + Containerfile.
/// `Prebuilt` consumes a registry-hosted image directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// Build a new image: `base` is a `FROM` directive.
    Build { base: String },
    /// Pull a prebuilt image: `ref_str` is the full pullable reference
    /// (e.g. `ghcr.io/foo/bar:tag`), already resolved from shorthand.
    Prebuilt { ref_str: String },
}

impl ImageSource {
    pub fn is_prebuilt(&self) -> bool {
        matches!(self, Self::Prebuilt { .. })
    }

    pub fn is_build(&self) -> bool {
        matches!(self, Self::Build { .. })
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageConfig {
    pub base: String,
    pub name: String,
    /// When set, treat the image as prebuilt — pull this exact reference
    /// instead of building from `base`.
    #[serde(rename = "image", default)]
    pub image_ref: Option<String>,
    /// Number of times to retry pulling the image on failure.
    #[serde(
        default = "default_pull_retry",
        skip_serializing_if = "is_default_pull_retry"
    )]
    pub pull_retry: u32,
    /// Delay between pull retries (Podman duration, e.g. "5s", "2m").
    #[serde(
        default = "default_pull_retry_delay",
        skip_serializing_if = "is_default_pull_delay"
    )]
    pub pull_retry_delay: String,
    #[serde(default, skip_serializing_if = "is_default_packages")]
    pub packages: PackageConfig,
    #[serde(default, skip_serializing_if = "is_default_run")]
    pub run: RunConfig,
}

impl ImageConfig {
    /// Resolve this config into a single `ImageSource` describing how
    /// the image should be obtained.
    pub fn source(&self) -> ImageSource {
        match &self.image_ref {
            Some(ref_str) => ImageSource::Prebuilt {
                ref_str: ref_str.clone(),
            },
            None => ImageSource::Build {
                base: self.base.clone(),
            },
        }
    }
}

fn default_pull_retry() -> u32 {
    3
}
fn default_pull_retry_delay() -> String {
    "5s".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PackageConfig {
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub install: Vec<String>,
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    pub remove: Vec<String>,
    #[serde(
        default = "default_package_manager",
        skip_serializing_if = "is_default_pkg_mgr"
    )]
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
    #[serde(default, skip_serializing_if = "is_empty_vec")]
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
    #[serde(default = "default_shell", skip_serializing_if = "is_default_shell")]
    pub shell: String,
    /// Optional memory limit (e.g. "2g", "512m").
    #[serde(default, skip_serializing_if = "is_none")]
    pub memory: Option<String>,
    /// Optional systemd ExecReload command.
    #[serde(default, skip_serializing_if = "is_none")]
    pub reload_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "is_default_mounts")]
    pub mounts: MountConfig,
    #[serde(default, skip_serializing_if = "is_empty_hashmap")]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MountConfig {
    #[serde(default, skip_serializing_if = "is_empty_vec")]
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
                Ok(if v {
                    GpuMode::Enabled
                } else {
                    GpuMode::Disabled
                })
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<GpuMode, E> {
                match v {
                    "auto" => Ok(GpuMode::Auto),
                    "nvidia" => Ok(GpuMode::Nvidia),
                    "true" => Ok(GpuMode::Enabled),
                    "false" => Ok(GpuMode::Disabled),
                    _ => Err(de::Error::unknown_variant(
                        v,
                        &["auto", "nvidia", "true", "false"],
                    )),
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
//  HostExecConfig
// ---------------------------------------------------------------------------

/// Configuration for the host-exec capability — allows the container
/// to run commands on the host via a configurable allowlist.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostExecConfig {
    /// Whether host-exec is enabled at all.
    #[serde(default)]
    pub enabled: bool,
    /// Optional alias → absolute-path map for command allowlisting.
    /// When `None`, any command is allowed (legacy mode — only use when `enabled` is true).
    /// When `Some`, only commands whose aliases appear in this map may be executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist: Option<std::collections::HashMap<String, String>>,
}

impl HostExecConfig {
    /// Check whether `cmd` is allowed and return the host path to execute.
    ///
    /// - If an allowlist is configured, `cmd` must be a key in the map;
    ///   the associated absolute path is returned.
    /// - If no allowlist is configured (legacy mode), `cmd` is used as-is
    ///   but only when `enabled` is true.
    pub fn resolve<'a>(&'a self, cmd: &'a str) -> Option<&'a str> {
        if !self.enabled {
            return None;
        }
        match &self.allowlist {
            Some(map) => map.get(cmd).map(|s| s.as_str()),
            None => Some(cmd),
        }
    }
}

fn is_default_host_exec(v: &HostExecConfig) -> bool {
    !v.enabled && v.allowlist.is_none()
}

// ---------------------------------------------------------------------------
//  IntegrationConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IntegrationConfig {
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub wayland: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub audio: bool,
    #[serde(default, skip_serializing_if = "is_default_gpu")]
    pub gpu: GpuMode,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub dbus: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub notify: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub xdg_open: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub clipboard: bool,
    #[serde(default, skip_serializing_if = "is_default_host_exec")]
    pub host_exec: HostExecConfig,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ssh_agent: bool,
    /// Bind-mount host font directory (`~/.fonts`) as read-only.
    /// Only top-level directories are mounted to keep `.local` and `.config` writable.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_fonts: bool,
    /// Bind-mount host icon directory (`~/.icons`) as read-only.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_icons: bool,
    /// Bind-mount host theme directory (`~/.themes`) as read-only.
    /// Only top-level directories are mounted to keep `.local` and `.config` writable.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
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
            notify: true,
            xdg_open: true,
            clipboard: true,
            host_exec: HostExecConfig::default(),
            ssh_agent: false,
            sync_fonts: true,
            sync_icons: true,
            sync_themes: true,
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
/// `podbox` generates a companion `podbox-proxy-<name>.service` unit that
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
//  SecurityConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SecurityConfig {
    /// AppArmor profile name or path. Empty/None = use Podman default.
    #[serde(default)]
    pub apparmor: Option<String>,
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
    #[serde(default, skip_serializing_if = "is_default_lifecycle")]
    pub lifecycle: LifecycleConfig,
    #[serde(default, skip_serializing_if = "is_default_systemd")]
    pub systemd: SystemdConfig,
    #[serde(default, skip_serializing_if = "is_default_dbus")]
    pub dbus: DbusConfig,
    #[serde(default, skip_serializing_if = "is_default_security")]
    pub security: SecurityConfig,
}

impl Config {
    /// Returns `true` when D-Bus integration is active (proxy unit is generated).
    pub fn use_dbus_proxy(&self) -> bool {
        self.integration.dbus && (!self.dbus.talk.is_empty() || !self.dbus.own.is_empty())
    }

    /// Parse a TOML definition from a string.
    pub fn parse(content: &str) -> Result<Config> {
        let config: Config = toml::from_str(content)
            .with_context(|| "failed to parse definition file".to_string())?;
        config.validate()?;
        Ok(config)
    }

    /// Load a definition from a file path.
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Err(PodboxError::DefinitionNotFound(path.to_path_buf()).into());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read definition file '{}'", path.display()))?;
        Self::parse(&content)
    }

    /// Return the built-in embedded default definition.
    pub fn embedded() -> Config {
        Self::parse(EMBEDDED_DEFAULT).expect("embedded default is valid TOML")
    }

    /// Validate config fields and return structured errors.
    pub fn validate(&self) -> Result<()> {
        let mut errors: Vec<String> = Vec::new();

        // -- image --
        if self.image.base.trim().is_empty() {
            errors.push("image.base: must not be empty".into());
        }
        if self.image.name.trim().is_empty() {
            errors.push("image.name: must not be empty".into());
        } else if !is_valid_name(&self.image.name) {
            errors.push(format!(
                "image.name: '{}' contains invalid characters (use letters, digits, hyphens, underscores, dots)",
                self.image.name
            ));
        }
        if let Some(ref r) = self.image.image_ref {
            if r.trim().is_empty() {
                errors.push("image.image: must not be empty when set".into());
            } else if !r.contains(':') && !r.contains('/') {
                errors.push(format!(
                    "image.image: '{}' does not look like a valid image reference (missing ':' or '/')",
                    r
                ));
            }
        }

        // -- container --
        if self.container.name.trim().is_empty() {
            errors.push("container.name: must not be empty".into());
        } else if !is_valid_name(&self.container.name) {
            errors.push(format!(
                "container.name: '{}' contains invalid characters (use letters, digits, hyphens, underscores, dots)",
                self.container.name
            ));
        }
        if self.container.home.as_os_str().is_empty() {
            errors.push("container.home: must not be empty".into());
        }
        if self.container.shell.trim().is_empty() {
            errors.push("container.shell: must not be empty".into());
        }
        if let Some(ref mem) = self.container.memory {
            if !is_valid_memory(mem) {
                errors.push(format!(
                    "container.memory: '{}' is not a valid memory limit (e.g. '2g', '512m')",
                    mem
                ));
            }
        }
        for (i, mount) in self.container.mounts.extra.iter().enumerate() {
            if !mount.contains(':') {
                errors.push(format!(
                    "container.mounts.extra[{}]: '{}' missing ':' separator (expected host:container[:options])",
                    i, mount
                ));
            }
        }
        for (key, val) in &self.container.env {
            if key.contains('\n') {
                errors.push(format!("container.env: key {:?} contains newline", key));
            }
            if val.contains('\n') {
                errors.push(format!(
                    "container.env: value for {:?} contains newline",
                    key
                ));
            }
        }

        // -- integration.host_exec --
        if let Some(ref map) = self.integration.host_exec.allowlist {
            for (alias, path) in map {
                if !is_absolute_path(path) {
                    errors.push(format!(
                        "integration.host_exec.allowlist.{}: path '{}' is not absolute (must start with '/')",
                        alias, path
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(PodboxError::ConfigValidationFailed(errors.join("\n  - ")).into())
        }
    }
}

/// Built-in default definition used when no definition file is found.
pub const EMBEDDED_DEFAULT: &str = r#"
[image]
base = "fedora:44"
name = "podbox"

[container]
    name = "podbox"
home = "~/containers/podbox"
"#;

// ---------------------------------------------------------------------------
//  Validation helpers
// ---------------------------------------------------------------------------

fn is_valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn is_absolute_path(s: &str) -> bool {
    s.starts_with('/')
}

fn is_valid_memory(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let digits: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let suffix: String = s.chars().skip(digits.len()).collect();
    if digits.is_empty() || digits == "." {
        return false;
    }
    if digits.starts_with('.') || (digits.chars().filter(|&c| c == '.').count() > 1) {
        return false;
    }
    suffix.is_empty()
        || matches!(
            suffix.as_str(),
            "k" | "K" | "m" | "M" | "g" | "G" | "t" | "T"
        )
}

// ---------------------------------------------------------------------------
//  Helpers
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_empty_vec(v: &[String]) -> bool {
    v.is_empty()
}
fn is_empty_hashmap(v: &HashMap<String, String>) -> bool {
    v.is_empty()
}
fn is_default_mounts(v: &MountConfig) -> bool {
    v.extra.is_empty()
}
fn is_default_gpu(v: &GpuMode) -> bool {
    *v == GpuMode::Auto
}
fn is_none<T>(v: &Option<T>) -> bool {
    v.is_none()
}
fn is_default_shell(v: &str) -> bool {
    v == "fish"
}
fn is_default_pkg_mgr(v: &str) -> bool {
    v == "dnf"
}
fn is_default_pull_retry(v: &u32) -> bool {
    *v == 3
}
fn is_default_pull_delay(v: &str) -> bool {
    v == "5s"
}

fn is_default_packages(v: &PackageConfig) -> bool {
    v.install.is_empty() && v.remove.is_empty() && v.manager == "dnf"
}

fn is_default_run(v: &RunConfig) -> bool {
    v.commands.is_empty()
}

fn is_default_lifecycle(v: &LifecycleConfig) -> bool {
    !v.quadlet && !v.autostart && v.on_stop == OnStop::Keep && !v.auto_update
}

fn is_default_systemd(v: &SystemdConfig) -> bool {
    v.requires.is_empty() && v.after.is_empty()
}

fn is_default_dbus(v: &DbusConfig) -> bool {
    v.talk.is_empty() && v.own.is_empty()
}

fn is_default_security(v: &SecurityConfig) -> bool {
    v.apparmor.is_none()
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

/// Return the podbox configuration directory (`~/.config/podbox/`).
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("podbox"))
        .unwrap_or_else(|| PathBuf::from("~/.config/podbox"))
}

/// Find a definition file.
///
/// Search order:
/// 1. `./.podbox.toml` (then `./.podmgr.toml` as compat fallback)
/// 2. Files in `~/.config/podbox/` matching `*.toml`
///
/// Returns `None` if no definition file exists anywhere — callers should
/// fall back to [`Config::embedded`].
pub fn find_definition() -> Option<PathBuf> {
    // New name takes precedence
    let new_local = PathBuf::from(".podbox.toml");
    if new_local.exists() {
        return Some(new_local);
    }

    // Compat: old .podmgr.toml still works with a warning
    let old_local = PathBuf::from(".podmgr.toml");
    if old_local.exists() {
        eprintln!(
            "Warning: '.podmgr.toml' found. Rename it to '.podbox.toml' to silence this warning."
        );
        return Some(old_local);
    }

    let config_dir = config_dir();

    if config_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&config_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "toml")
                    .unwrap_or(false)
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

/// List all config files in `~/.config/podbox/`.
pub fn list_configs() -> Vec<std::path::PathBuf> {
    let config_dir = config_dir();
    if !config_dir.is_dir() {
        return vec![];
    }
    let mut entries: Vec<_> = std::fs::read_dir(&config_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    entries.sort();
    entries
}

// ---------------------------------------------------------------------------
//  Active Context
// ---------------------------------------------------------------------------

/// Path to the active context marker file (`~/.config/podbox/.active`).
pub fn active_context_path() -> PathBuf {
    config_dir().join(".active")
}

/// Read the active context name from the marker file.
///
/// Validates that a corresponding `<name>.toml` exists; stale markers are
/// silently cleaned up.
pub fn read_active_context() -> Option<String> {
    let path = active_context_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let name = content.trim().to_string();
    if name.is_empty() {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    let config_path = config_dir().join(format!("{}.toml", name));
    if config_path.exists() {
        Some(name)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Write the active context name to the marker file.
pub fn write_active_context(name: &str) -> Result<()> {
    let path = active_context_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, name)?;
    Ok(())
}

/// Clear (remove) the active context marker file.
pub fn clear_active_context() -> Result<()> {
    let path = active_context_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
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
        assert_eq!(cfg.image.name, "podbox");
        assert_eq!(cfg.container.name, "podbox");
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
        assert_eq!(
            serde_json::to_string(&GpuMode::Nvidia).unwrap(),
            "\"nvidia\""
        );
        // Verify TOML serialization works inside a table
        #[derive(Serialize)]
        struct Wrapper {
            gpu: GpuMode,
        }
        let wrapper = Wrapper {
            gpu: GpuMode::Nvidia,
        };
        let toml_out = toml::to_string(&wrapper).unwrap();
        assert!(toml_out.contains("gpu = \"nvidia\""));
        let wrapper = Wrapper {
            gpu: GpuMode::Enabled,
        };
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
        assert!(err.downcast_ref::<PodboxError>().is_some());
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
}
