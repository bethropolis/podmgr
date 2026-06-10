use crate::config::{GpuMode, OnStop, XdgDirConfig, XdgDirValue};
use crate::profiles;

use super::shell::ShellInfo;
use super::ProfileChoice;

pub(super) fn prompt_profile(profiles: &[profiles::Profile]) -> ProfileChoice<'_> {
    let items: Vec<String> = {
        let mut v = vec!["Custom (from scratch)".into()];
        for p in profiles {
            v.push(format!("{}  —  {}", p.label, p.description));
        }
        v
    };
    let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Configuration type")
        .items(&items)
        .default(0)
        .interact()
        .expect("failed to get profile selection");
    if selection == 0 {
        ProfileChoice::Custom
    } else {
        let profile = &profiles[selection - 1];
        let customize = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Customize {} settings?", profile.label))
            .default(false)
            .interact()
            .expect("failed to get customize preference");
        ProfileChoice::Named(profile, customize)
    }
}

pub(super) fn prompt_custom_image() -> anyhow::Result<(crate::config::Config, String)> {
    println!("\n── Image ──\n");
    let is_prebuilt = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Use a prebuilt image? (No = build from distro base)")
        .default(true)
        .interact()
        .expect("failed to get image source");

    let (mut cfg, default_name) = if is_prebuilt {
        let ref_str: String =
            dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Image reference (e.g. ghcr.io/user/image:tag)")
                .default("ghcr.io/bethropolis/podbox:fedora-latest".to_string())
                .interact_text()?;
        let mut c = crate::config::Config::embedded();
        c.image.image_ref = Some(ref_str.clone());
        c.image.base = ref_str
            .rsplit_once(':')
            .map(|(_, t)| t)
            .unwrap_or("latest")
            .to_string();
        let name = ref_str
            .rsplit_once('/')
            .map(|(_, n)| n.split_once(':').map(|(n, _)| n).unwrap_or(n))
            .unwrap_or("container")
            .to_string();
        (c, name)
    } else {
        let base: String =
            dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Base image (e.g. fedora:44)")
                .default("fedora:44".to_string())
                .interact_text()?;
        let mut c = crate::config::Config::embedded();
        c.image.base = base.clone();
        c.image.packages.manager = detect_package_manager(&base);
        let name = base
            .split_once(':')
            .map(|(n, _)| n)
            .unwrap_or(&base)
            .split('/')
            .next_back()
            .unwrap_or(&base)
            .to_string();
        (c, name)
    };

    let packages: String =
        dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Packages to install (space-separated)")
            .default("fish fastfetch".to_string())
            .interact_text()?;
    cfg.image.packages.install = packages
        .split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if !is_prebuilt {
        let run_cmds: String =
            dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Extra RUN commands (one per line, \\n separated)")
                .default("".to_string())
                .interact_text()?;
        cfg.image.run.commands = run_cmds
            .split("\\n")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    Ok((cfg, default_name))
}

pub(super) fn prompt_customize_profile(
    mut cfg: crate::config::Config,
) -> anyhow::Result<crate::config::Config> {
    let current = cfg.image.packages.install.join(" ");
    let packages: String =
        dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Packages to install (space-separated)")
            .default(current)
            .interact_text()?;
    cfg.image.packages.install = packages
        .split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(cfg)
}

pub(super) fn detect_package_manager(image: &str) -> crate::config::PackageManager {
    crate::codegen::distros::detect_package_manager(image)
}

pub(super) fn prompt_name(
    default: &str,
    config_dir: &std::path::Path,
) -> anyhow::Result<String> {
    let name: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Container name")
        .default(default.to_string())
        .validate_with(move |input: &String| -> Result<(), &str> {
            if input.contains('/') || input.contains('\\') {
                return Err("Name must not contain slashes");
            }
            if !input
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return Err("Name must be alphanumeric, hyphens, or underscores");
            }
            let config_path = config_dir.join(format!("{}.toml", input));
            if config_path.exists() {
                return Err("A config with this name already exists");
            }
            Ok(())
        })
        .interact_text()?;
    Ok(name)
}

pub(super) fn prompt_shell(detected: &ShellInfo) -> ShellInfo {
    if detected.detected {
        let ok = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Use detected shell {}?", detected.full_path))
            .default(true)
            .interact()
            .expect("failed to get shell confirmation");
        if ok {
            return super::shell::shell_info_from_bin(&detected.bin_name);
        }
    }
    let fallback = "/usr/bin/bash";
    let input: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Shell path")
        .default(fallback.to_string())
        .interact_text()
        .expect("failed to get shell input");
    let bin = std::path::Path::new(&input)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "bash".to_string());
    let mut info = super::shell::shell_info_from_bin(&bin);
    info.full_path = input;
    info.detected = false;
    info
}

pub(super) fn prompt_memory() -> Option<String> {
    let input: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Memory limit (e.g. 2g, 512m) — leave empty for no limit")
        .default("".to_string())
        .interact_text()
        .expect("failed to get memory input");
    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub(super) fn prompt_extra_mounts() -> Vec<String> {
    let input: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Extra mounts (host:container, comma-separated) — leave empty for none")
        .default("".to_string())
        .interact_text()
        .expect("failed to get mounts input");
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub(super) fn prompt_wayland() -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Wayland support?")
        .default(true)
        .interact()
        .expect("failed to get wayland preference")
}

pub(super) fn prompt_audio() -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Audio support (PipeWire/PulseAudio)?")
        .default(true)
        .interact()
        .expect("failed to get audio preference")
}

pub(super) fn prompt_dbus() -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("D-Bus session bus access?")
        .default(true)
        .interact()
        .expect("failed to get dbus preference")
}

pub(super) fn confirm_default(prompt: &str, default: bool) -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(prompt)
        .default(default)
        .interact()
        .expect("failed to get confirmation")
}

pub(super) fn prompt_gpu() -> GpuMode {
    let items = ["auto", "enabled (DRI)", "nvidia", "disabled"];
    let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("GPU acceleration")
        .items(&items)
        .default(0)
        .interact()
        .expect("failed to get GPU selection");
    match selection {
        0 => GpuMode::Auto,
        1 => GpuMode::Enabled,
        2 => GpuMode::Nvidia,
        _ => GpuMode::Disabled,
    }
}

pub(super) fn prompt_integration_extras(
    default_notify: bool,
    default_clipboard: bool,
    default_xdg_open: bool,
    default_ssh_agent: bool,
) -> (bool, bool, bool, bool) {
    let items = [
        "notify     (desktop notifications passthrough)",
        "clipboard  (wl-copy/wl-paste passthrough)",
        "xdg_open   (open URIs on host)",
        "ssh_agent  (SSH agent forwarding)",
    ];
    let defaults = [
        default_notify,
        default_clipboard,
        default_xdg_open,
        default_ssh_agent,
    ];
    let selections: Vec<usize> =
        dialoguer::MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Integration extras (space to toggle, enter to confirm)")
            .items(&items)
            .defaults(&defaults)
            .interact()
            .expect("failed to get integration selection");
    (
        selections.contains(&0),
        selections.contains(&1),
        selections.contains(&2),
        selections.contains(&3),
    )
}

pub(super) fn prompt_xdg_dirs() -> XdgDirConfig {
    let items = [
        "Documents",
        "Downloads",
        "Pictures",
        "Music",
        "Videos",
        "Desktop",
        "Projects",
    ];
    let selections: Vec<usize> =
        dialoguer::MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("XDG directories to share (space to toggle, enter to confirm)")
            .items(&items)
            .interact()
            .expect("failed to get XDG directory selection");
    XdgDirConfig {
        documents: XdgDirValue::Simple(selections.contains(&0)),
        downloads: XdgDirValue::Simple(selections.contains(&1)),
        pictures: XdgDirValue::Simple(selections.contains(&2)),
        music: XdgDirValue::Simple(selections.contains(&3)),
        videos: XdgDirValue::Simple(selections.contains(&4)),
        desktop: XdgDirValue::Simple(selections.contains(&5)),
        projects: XdgDirValue::Simple(selections.contains(&6)),
    }
}

pub(super) fn prompt_lifecycle() -> (bool, bool) {
    let quadlet = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Quadlet + systemd management?")
        .default(true)
        .interact()
        .expect("failed to get lifecycle selection");
    let autostart = if quadlet {
        dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Auto-start at login?")
            .default(false)
            .interact()
            .expect("failed to get autostart selection")
    } else {
        false
    };
    (quadlet, autostart)
}

pub(super) fn prompt_on_stop() -> OnStop {
    let items = [
        "keep  (container stays stopped until started again)",
        "remove  (auto-clean container on stop)",
    ];
    let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("On-stop behavior")
        .items(&items)
        .default(0)
        .interact()
        .expect("failed to get on_stop selection");
    if selection == 0 {
        OnStop::Keep
    } else {
        OnStop::Remove
    }
}

pub(super) fn prompt_auto_update(config: &crate::config::Config) -> bool {
    let is_prebuilt = matches!(config.image.source(), crate::config::ImageSource::Prebuilt { .. });
    if !is_prebuilt {
        let enabled = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Auto-update image? (only works with prebuilt images)")
            .default(false)
            .interact()
            .expect("failed to get auto_update selection");
        if enabled {
            eprintln!("Warning: auto_update is enabled but the image is built from source. Auto-update requires a prebuilt image.");
        }
        return enabled;
    }
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Auto-update image?")
        .default(false)
        .interact()
        .expect("failed to get auto_update selection")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_package_manager_dnf() {
        assert_eq!(
            detect_package_manager("fedora:44"),
            crate::config::PackageManager::Dnf
        );
        assert_eq!(
            detect_package_manager("centos:stream"),
            crate::config::PackageManager::Dnf
        );
        assert_eq!(
            detect_package_manager("registry.fedoraproject.org/fedora:41"),
            crate::config::PackageManager::Dnf
        );
    }

    #[test]
    fn detect_package_manager_pacman() {
        assert_eq!(
            detect_package_manager("archlinux:latest"),
            crate::config::PackageManager::Pacman
        );
        assert_eq!(
            detect_package_manager("cachyos:latest"),
            crate::config::PackageManager::Pacman
        );
        assert_eq!(
            detect_package_manager("manjaro:latest"),
            crate::config::PackageManager::Pacman
        );
    }

    #[test]
    fn detect_package_manager_apt() {
        assert_eq!(
            detect_package_manager("ubuntu:24.04"),
            crate::config::PackageManager::Apt
        );
        assert_eq!(
            detect_package_manager("debian:bookworm"),
            crate::config::PackageManager::Apt
        );
    }

    #[test]
    fn detect_package_manager_apk() {
        assert_eq!(
            detect_package_manager("alpine:latest"),
            crate::config::PackageManager::Apk
        );
    }

    #[test]
    fn detect_package_manager_fallback() {
        assert_eq!(
            detect_package_manager("unknown:latest"),
            crate::config::PackageManager::Dnf
        );
    }
}
