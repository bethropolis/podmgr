use std::collections::HashMap;

use crate::config::enums::{GpuMode, OnStop};
use crate::config::types::{HostExecConfig, LifecycleConfig, PackageConfig, RunConfig, SecurityConfig, SystemdConfig};

pub const EMBEDDED_DEFAULT: &str = r#"
[image]
base = "fedora:44"
name = "podbox"

[container]
    name = "podbox"
home = "~/containers/podbox"
"#;

pub fn default_true() -> bool {
    true
}

pub fn is_true(v: &bool) -> bool {
    *v
}

pub fn is_false(v: &bool) -> bool {
    !*v
}

pub fn default_shell() -> String {
    "fish".into()
}

pub fn is_default_shell(v: &str) -> bool {
    v == "fish"
}

pub fn default_package_manager() -> String {
    "dnf".into()
}

pub fn is_default_pkg_mgr(v: &str) -> bool {
    v == "dnf"
}

pub fn default_pull_retry() -> u32 {
    3
}

pub fn is_default_pull_retry(v: &u32) -> bool {
    *v == 3
}

pub fn default_pull_retry_delay() -> String {
    "5s".into()
}

pub fn is_default_pull_retry_delay(v: &str) -> bool {
    v == "5s"
}

pub fn is_empty_hashmap(v: &HashMap<String, String>) -> bool {
    v.is_empty()
}

pub fn is_default_mounts(v: &super::types::MountConfig) -> bool {
    v.extra.is_empty()
}

pub fn is_default_gpu(v: &GpuMode) -> bool {
    *v == GpuMode::Auto
}

pub fn is_default_packages(v: &PackageConfig) -> bool {
    v.install.is_empty() && v.remove.is_empty() && v.manager == "dnf"
}

pub fn is_default_run(v: &RunConfig) -> bool {
    v.commands.is_empty()
}

pub fn is_default_host_exec(v: &HostExecConfig) -> bool {
    !v.enabled && v.allowlist.is_none()
}

pub fn is_default_lifecycle(v: &LifecycleConfig) -> bool {
    !v.quadlet && !v.autostart && v.on_stop == OnStop::Keep && !v.auto_update
}

pub fn is_default_systemd(v: &SystemdConfig) -> bool {
    v.requires.is_empty() && v.after.is_empty()
}

pub fn is_default_dbus(v: &super::types::DbusConfig) -> bool {
    v.preset.is_empty() && v.talk.is_empty() && v.own.is_empty()
}

pub fn is_default_security(v: &SecurityConfig) -> bool {
    v.apparmor.is_none() && v.seccomp.is_none() && v.security_label_disable && v.no_new_privileges
}
