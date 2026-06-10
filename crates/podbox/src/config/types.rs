use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::defaults::{
    default_package_manager, default_pull_retry, default_pull_retry_delay, default_shell,
    default_true, is_default_gpu, is_default_host_exec, is_default_mounts,
    is_default_packages, is_default_pkg_mgr, is_default_pull_retry, is_default_pull_retry_delay,
    is_default_run, is_default_shell, is_empty_hashmap, is_false, is_true,
};
use crate::config::enums::{GpuMode, ImageSource, OnStop, XdgDirValue};
use crate::config::expand_tilde;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageConfig {
    pub base: String,
    pub name: String,
    #[serde(rename = "image", default)]
    pub image_ref: Option<String>,
    #[serde(
        default = "default_pull_retry",
        skip_serializing_if = "is_default_pull_retry"
    )]
    pub pull_retry: u32,
    #[serde(
        default = "default_pull_retry_delay",
        skip_serializing_if = "is_default_pull_retry_delay"
    )]
    pub pull_retry_delay: String,
    #[serde(default, skip_serializing_if = "is_default_packages")]
    pub packages: PackageConfig,
    #[serde(default, skip_serializing_if = "is_default_run")]
    pub run: RunConfig,
}

impl ImageConfig {
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PackageConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct RunConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContainerConfig {
    pub name: String,
    #[serde(deserialize_with = "deserialize_home")]
    pub home: PathBuf,
    #[serde(default = "default_shell", skip_serializing_if = "is_default_shell")]
    pub shell: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "is_default_mounts")]
    pub mounts: MountConfig,
    #[serde(default, skip_serializing_if = "is_empty_hashmap")]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MountConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<String>,
}

fn deserialize_home<'de, D>(deserializer: D) -> std::result::Result<PathBuf, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let path = String::deserialize(deserializer)?;
    Ok(expand_tilde(&path))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostExecConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist: Option<std::collections::HashMap<String, String>>,
}

impl HostExecConfig {
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub gpg_agent: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_fonts: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_icons: bool,
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
            gpg_agent: false,
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
    pub documents: XdgDirValue,
    #[serde(default)]
    pub downloads: XdgDirValue,
    #[serde(default)]
    pub pictures: XdgDirValue,
    #[serde(default)]
    pub music: XdgDirValue,
    #[serde(default)]
    pub videos: XdgDirValue,
    #[serde(default)]
    pub desktop: XdgDirValue,
    #[serde(default)]
    pub projects: XdgDirValue,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ExportConfig {
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub bins: Vec<String>,
}

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

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct SystemdConfig {
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct DbusConfig {
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub talk: Vec<String>,
    #[serde(default)]
    pub own: Vec<String>,
}

impl DbusConfig {
    pub fn effective_talk(&self) -> Vec<String> {
        let mut result = self.talk.clone();
        if !self.preset.is_empty() && self.preset != "none" {
            for svc in dbus_preset_talk(&self.preset) {
                if !result.contains(&svc.to_string()) {
                    result.push(svc.to_string());
                }
            }
        }
        result
    }
}

pub fn dbus_preset_talk(preset: &str) -> &[&str] {
    match preset {
        "flatpak" => &[
            "org.freedesktop.Flatpak",
            "org.freedesktop.Flatpak.*",
            "org.freedesktop.portal.*",
            "org.freedesktop.portal.Flatpak.*",
        ],
        "gnome" => &[
            "org.gnome.Shell",
            "org.gnome.Shell.*",
            "org.gnome.ScreenSaver",
            "org.gnome.Mutter.*",
            "org.gnome.keyring.*",
            "org.freedesktop.portal.*",
        ],
        "kde" => &["org.kde.*", "org.freedesktop.portal.*"],
        "portal" => &["org.freedesktop.portal.*"],
        _ => &[],
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SecurityConfig {
    #[serde(default)]
    pub apparmor: Option<String>,
    #[serde(default)]
    pub seccomp: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub security_label_disable: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub no_new_privileges: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        SecurityConfig {
            apparmor: None,
            seccomp: None,
            security_label_disable: true,
            no_new_privileges: true,
        }
    }
}

