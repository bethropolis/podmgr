use std::collections::HashMap;

use crate::config::{Config, GpuMode};

/// Raw OCI labels read from `podman inspect`.
pub type LabelMap = HashMap<String, String>;

/// Fetch the OCI labels for a local image tag.
pub fn fetch(image_ref: &str) -> anyhow::Result<LabelMap> {
    let output = std::process::Command::new("podman")
        .args(["inspect", "--format", "{{json .Labels}}", image_ref])
        .output()?;

    if !output.status.success() {
        return Ok(LabelMap::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let map: LabelMap = serde_json::from_str(stdout.trim()).unwrap_or_default();
    Ok(map)
}

/// Apply podmgr label defaults to a Config, with the user config winning
/// on every field that was explicitly set.
///
/// This is called after parsing the user's TOML but before any codegen.
/// Fields in `user_config` always take precedence; labels only fill in
/// what the user left at the TOML default.
pub fn apply_defaults(config: &mut Config, labels: &LabelMap) {
    // Only proceed if the image declares a compatible schema.
    match labels.get("podmgr.schema").map(|s| s.as_str()) {
        Some("1") => {}
        Some(v) => {
            eprintln!(
                "Warning: image declares podmgr.schema={}, host supports 1. \
                 Ignoring image labels.",
                v
            );
            return;
        }
        None => return,
    }

    let int = &mut config.integration;

    apply_bool(labels, "podmgr.integration.wayland", &mut int.wayland);
    apply_bool(labels, "podmgr.integration.audio", &mut int.audio);
    apply_bool(labels, "podmgr.integration.dbus", &mut int.dbus);
    apply_bool(labels, "podmgr.integration.notify", &mut int.notify);
    apply_bool(labels, "podmgr.integration.xdg_open", &mut int.xdg_open);
    apply_bool(labels, "podmgr.integration.clipboard", &mut int.clipboard);
    apply_bool(labels, "podmgr.integration.sync_fonts", &mut int.sync_fonts);
    apply_bool(labels, "podmgr.integration.sync_icons", &mut int.sync_icons);
    apply_bool(labels, "podmgr.integration.sync_themes", &mut int.sync_themes);

    apply_bool(labels, "podmgr.xdg_dirs.documents", &mut int.xdg_dirs.documents);
    apply_bool(labels, "podmgr.xdg_dirs.downloads", &mut int.xdg_dirs.downloads);
    apply_bool(labels, "podmgr.xdg_dirs.pictures", &mut int.xdg_dirs.pictures);
    apply_bool(labels, "podmgr.xdg_dirs.music", &mut int.xdg_dirs.music);
    apply_bool(labels, "podmgr.xdg_dirs.videos", &mut int.xdg_dirs.videos);

    if let Some(gpu_str) = labels.get("podmgr.integration.gpu") {
        if config.integration.gpu == GpuMode::Auto {
            config.integration.gpu = match gpu_str.as_str() {
                "true" => GpuMode::Enabled,
                "false" => GpuMode::Disabled,
                "nvidia" => GpuMode::Nvidia,
                _ => GpuMode::Auto,
            };
        }
    }

    if let Some(shell) = labels.get("podmgr.default_shell") {
        if config.container.shell == "fish" {
            config.container.shell = shell.clone();
        }
    }
}

fn apply_bool(labels: &LabelMap, key: &str, field: &mut bool) {
    if !*field {
        if let Some(v) = labels.get(key) {
            *field = v == "true";
        }
    }
}
