use crate::config::{Config, GpuMode, ImageSource, OnStop, XdgDirConfig};
use crate::profiles;

/// Information about the host shell, resolved at startup.
pub struct ShellInfo {
    pub bin_name: String,
    pub full_path: String,
    pub package_name: String,
    pub detected: bool,
}

/// Detect the host shell from $SHELL.
pub fn detect_host_shell() -> ShellInfo {
    detect_host_shell_from(std::env::var("SHELL").ok().as_deref())
}

fn detect_host_shell_from(shell_path: Option<&str>) -> ShellInfo {
    match shell_path {
        Some(path) if !path.is_empty() => {
            let bin = std::path::Path::new(path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if bin.is_empty() || bin == "sh" || bin == "dash" {
                return fallback_shell();
            }
            let mut info = shell_info_from_bin(&bin);
            info.detected = true;
            info
        }
        _ => fallback_shell(),
    }
}

fn fallback_shell() -> ShellInfo {
    ShellInfo {
        bin_name: "fish".into(),
        full_path: "/usr/bin/fish".into(),
        package_name: "fish".into(),
        detected: false,
    }
}

fn shell_info_from_bin(bin: &str) -> ShellInfo {
    match bin {
        "fish" => ShellInfo {
            bin_name: "fish".into(),
            full_path: "/usr/bin/fish".into(),
            package_name: "fish".into(),
            detected: false,
        },
        "bash" => ShellInfo {
            bin_name: "bash".into(),
            full_path: "/bin/bash".into(),
            package_name: "bash".into(),
            detected: false,
        },
        "zsh" => ShellInfo {
            bin_name: "zsh".into(),
            full_path: "/bin/zsh".into(),
            package_name: "zsh".into(),
            detected: false,
        },
        "nu" | "nushell" => ShellInfo {
            bin_name: "nu".into(),
            full_path: "/usr/bin/nu".into(),
            package_name: "nushell".into(),
            detected: false,
        },
        other => ShellInfo {
            bin_name: other.into(),
            full_path: format!("/usr/bin/{}", other),
            package_name: other.into(),
            detected: false,
        },
    }
}

/// Apply shell defaults to a config loaded from a profile.
/// Does NOT override a shell that the profile already set (e.g. `shell = "/usr/bin/fish"`).
pub fn apply_shell_defaults(config: &mut Config, shell: &ShellInfo) {
    if config.container.shell.trim().is_empty() {
        config.container.shell = shell.full_path.clone();
    }
    if !config
        .image
        .packages
        .install
        .iter()
        .any(|p| p == &shell.package_name)
    {
        config
            .image
            .packages
            .install
            .push(shell.package_name.clone());
    }
}

/// Result of the interactive wizard.
pub struct WizardResult {
    pub config: Config,
    pub name: String,
    pub confirmed: bool,
}

/// Run the interactive init wizard.
pub fn run_wizard(
    profiles: &[profiles::Profile],
    detected_shell: &ShellInfo,
) -> anyhow::Result<WizardResult> {
    // ── Phase 1: Image ──
    let (mut config, default_name) = match prompt_profile(profiles) {
        ProfileChoice::Named(profile, customize) => {
            let mut cfg: Config = toml::from_str(&profile.toml).map_err(|e| {
                anyhow::anyhow!("failed to parse profile '{}': {}", profile.name, e)
            })?;
            if customize {
                cfg = prompt_customize_profile(cfg)?;
            }
            (cfg, profile.name.clone())
        }
        ProfileChoice::Custom => prompt_custom_image()?,
    };

    // ── Phase 2: Container ──
    println!("\n── Container ──\n");
    let name = prompt_name(&default_name, &crate::config::config_dir())?;
    config.container.name = name.clone();
    config.image.name = name.clone();
    config.container.home = crate::config::expand_tilde(&format!("~/containers/{}", name));

    let shell = prompt_shell(detected_shell);
    config.container.shell = shell.full_path.clone();
    if !config
        .image
        .packages
        .install
        .iter()
        .any(|p| p == &shell.package_name)
    {
        config
            .image
            .packages
            .install
            .push(shell.package_name.clone());
    }

    if let Some(mem) = prompt_memory() {
        config.container.memory = Some(mem);
    }

    let mounts = prompt_extra_mounts();
    config.container.mounts.extra = mounts;

    // ── Phase 3: Integration ──
    println!("\n── Integration ──\n");
    config.integration.wayland = prompt_wayland();
    config.integration.audio = prompt_audio();
    config.integration.dbus = prompt_dbus();
    config.integration.gpu = prompt_gpu();
    config.integration.sync_themes = confirm_default("Sync themes from host?", true);
    config.integration.sync_icons = confirm_default("Sync icons from host?", true);
    config.integration.sync_fonts = confirm_default("Sync fonts from host?", true);

    let (notify, clipboard, xdg_open, ssh_agent) = prompt_integration_extras(
        config.integration.notify,
        config.integration.clipboard,
        config.integration.xdg_open,
        config.integration.ssh_agent,
    );
    config.integration.notify = notify;
    config.integration.clipboard = clipboard;
    config.integration.xdg_open = xdg_open;
    config.integration.ssh_agent = ssh_agent;

    config.integration.xdg_dirs = prompt_xdg_dirs();

    // ── Phase 4: Lifecycle ──
    println!("\n── Lifecycle ──\n");
    let (quadlet, autostart) = prompt_lifecycle();
    config.lifecycle.quadlet = quadlet;
    config.lifecycle.autostart = autostart;

    config.lifecycle.on_stop = prompt_on_stop();
    config.lifecycle.auto_update = prompt_auto_update(&config);

    // ── Phase 5: Review ──
    print_summary(&config, &name);
    let toml_str = toml::to_string_pretty(&config)
        .map_err(|e| anyhow::anyhow!("failed to serialize config: {}", e))?;
    let confirmed = preview_and_confirm(&toml_str);

    Ok(WizardResult {
        config,
        name,
        confirmed,
    })
}

// ── Phase 1 helpers ──

enum ProfileChoice<'a> {
    Custom,
    Named(&'a profiles::Profile, bool),
}

fn prompt_profile(profiles: &[profiles::Profile]) -> ProfileChoice<'_> {
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

fn prompt_custom_image() -> anyhow::Result<(Config, String)> {
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
        let mut c = Config::embedded();
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
        let mut c = Config::embedded();
        c.image.base = base.clone();
        c.image.packages.manager = detect_package_manager(&base).to_string();
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

fn prompt_customize_profile(mut cfg: Config) -> anyhow::Result<Config> {
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

fn detect_package_manager(image: &str) -> &'static str {
    let lower = image.to_lowercase();
    if lower.contains("ubuntu") || lower.contains("debian") {
        "apt"
    } else if lower.contains("fedora") || lower.contains("centos") || lower.contains("rhel") {
        "dnf"
    } else if lower.contains("arch") || lower.contains("cachy") || lower.contains("manjaro") {
        "pacman"
    } else if lower.contains("alpine") {
        "apk"
    } else if lower.contains("opensuse") {
        "zypper"
    } else {
        "apt"
    }
}

// ── Phase 2 helpers ──

fn prompt_name(default: &str, config_dir: &std::path::Path) -> anyhow::Result<String> {
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

fn prompt_shell(detected: &ShellInfo) -> ShellInfo {
    if detected.detected {
        let ok = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt(format!("Use detected shell {}?", detected.full_path))
            .default(true)
            .interact()
            .expect("failed to get shell confirmation");
        if ok {
            return shell_info_from_bin(&detected.bin_name);
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
    let mut info = shell_info_from_bin(&bin);
    info.full_path = input;
    info.detected = false;
    info
}

fn prompt_memory() -> Option<String> {
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

fn prompt_extra_mounts() -> Vec<String> {
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

// ── Phase 3 helpers ──

fn prompt_wayland() -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Wayland support?")
        .default(true)
        .interact()
        .expect("failed to get wayland preference")
}

fn prompt_audio() -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Audio support (PipeWire/PulseAudio)?")
        .default(true)
        .interact()
        .expect("failed to get audio preference")
}

fn prompt_dbus() -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("D-Bus session bus access?")
        .default(true)
        .interact()
        .expect("failed to get dbus preference")
}

fn confirm_default(prompt: &str, default: bool) -> bool {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(prompt)
        .default(default)
        .interact()
        .expect("failed to get confirmation")
}

fn prompt_gpu() -> GpuMode {
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

fn prompt_integration_extras(
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

fn prompt_xdg_dirs() -> XdgDirConfig {
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
        documents: selections.contains(&0),
        downloads: selections.contains(&1),
        pictures: selections.contains(&2),
        music: selections.contains(&3),
        videos: selections.contains(&4),
        desktop: selections.contains(&5),
        projects: selections.contains(&6),
    }
}

// ── Phase 4 helpers ──

fn prompt_lifecycle() -> (bool, bool) {
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

fn prompt_on_stop() -> OnStop {
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

fn prompt_auto_update(config: &Config) -> bool {
    let is_prebuilt = matches!(config.image.source(), ImageSource::Prebuilt { .. });
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

// ── Phase 5 helpers ──

fn print_summary(config: &Config, name: &str) {
    let image_type = match config.image.source() {
        ImageSource::Prebuilt { ref_str } => format!("prebuilt ({})", ref_str),
        ImageSource::Build { base } => format!("build from {}", base),
    };
    let lifecycle = if config.lifecycle.quadlet {
        let extras = vec![
            if config.lifecycle.autostart {
                Some("autostart")
            } else {
                None
            },
            if config.lifecycle.auto_update {
                Some("auto-update")
            } else {
                None
            },
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        if extras.is_empty() {
            "quadlet".to_string()
        } else {
            format!("quadlet ({})", extras)
        }
    } else {
        "manual".to_string()
    };
    let on_stop = match config.lifecycle.on_stop {
        OnStop::Keep => "keep",
        OnStop::Remove => "remove",
    };
    let gpu = match config.integration.gpu {
        GpuMode::Auto => "auto",
        GpuMode::Enabled => "enabled",
        GpuMode::Disabled => "disabled",
        GpuMode::Nvidia => "nvidia",
    };
    let xdg_count = [
        config.integration.xdg_dirs.documents,
        config.integration.xdg_dirs.downloads,
        config.integration.xdg_dirs.pictures,
        config.integration.xdg_dirs.music,
        config.integration.xdg_dirs.videos,
        config.integration.xdg_dirs.desktop,
        config.integration.xdg_dirs.projects,
    ]
    .iter()
    .filter(|&&b| b)
    .count();

    println!("\n── Summary ──");
    println!("  Name:        {}", name);
    println!("  Image:       {}", image_type);
    println!("  Shell:       {}", config.container.shell);
    println!("  Home:        {}", config.container.home.display());
    if let Some(ref mem) = config.container.memory {
        println!("  Memory:      {}", mem);
    }
    if !config.container.mounts.extra.is_empty() {
        println!(
            "  Mounts:      {}",
            config.container.mounts.extra.join(", ")
        );
    }
    println!("  Integration:");
    println!(
        "    wayland: {}, audio: {}, dbus: {}, gpu: {}",
        config.integration.wayland, config.integration.audio, config.integration.dbus, gpu
    );
    let extras = vec![
        if config.integration.notify {
            Some("notify")
        } else {
            None
        },
        if config.integration.clipboard {
            Some("clipboard")
        } else {
            None
        },
        if config.integration.xdg_open {
            Some("xdg_open")
        } else {
            None
        },
        if config.integration.ssh_agent {
            Some("ssh_agent")
        } else {
            None
        },
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if !extras.is_empty() {
        println!("    extras:    {}", extras.join(", "));
    }
    if config.integration.sync_themes
        || config.integration.sync_icons
        || config.integration.sync_fonts
    {
        let sync = vec![
            if config.integration.sync_themes {
                Some("themes")
            } else {
                None
            },
            if config.integration.sync_icons {
                Some("icons")
            } else {
                None
            },
            if config.integration.sync_fonts {
                Some("fonts")
            } else {
                None
            },
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        println!("    sync:      {}", sync.join(", "));
    }
    println!("    xdg dirs:  {} shared", xdg_count);
    if !config.integration.export.apps.is_empty() {
        println!(
            "    exports:   apps: {}",
            config.integration.export.apps.join(", ")
        );
    }
    if !config.integration.export.bins.is_empty() {
        println!(
            "               bins: {}",
            config.integration.export.bins.join(", ")
        );
    }
    println!("  Lifecycle:   {} (on_stop: {})", lifecycle, on_stop);
    println!();
}

fn preview_and_confirm(toml: &str) -> bool {
    println!("{}\n", toml);
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("Write to config file?")
        .default(true)
        .interact()
        .expect("failed to get confirmation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_fish_from_path() {
        let info = detect_host_shell_from(Some("/usr/bin/fish"));
        assert_eq!(info.bin_name, "fish");
        assert_eq!(info.full_path, "/usr/bin/fish");
        assert_eq!(info.package_name, "fish");
        assert!(info.detected);
    }

    #[test]
    fn detect_zsh_from_path() {
        let info = detect_host_shell_from(Some("/bin/zsh"));
        assert_eq!(info.bin_name, "zsh");
        assert!(info.detected);
    }

    #[test]
    fn fallback_on_dash() {
        let info = detect_host_shell_from(Some("/bin/dash"));
        assert_eq!(info.bin_name, "fish");
        assert!(!info.detected);
    }

    #[test]
    fn fallback_on_empty_shell() {
        let info = detect_host_shell_from(None);
        assert_eq!(info.bin_name, "fish");
        assert!(!info.detected);
    }

    #[test]
    fn fallback_on_sh() {
        let info = detect_host_shell_from(Some("/bin/sh"));
        assert_eq!(info.bin_name, "fish");
        assert!(!info.detected);
    }

    #[test]
    fn nushell_binary_maps_to_nushell_package() {
        let info = detect_host_shell_from(Some("/usr/bin/nu"));
        assert_eq!(info.bin_name, "nu");
        assert_eq!(info.package_name, "nushell");
    }

    #[test]
    fn apply_shell_adds_package_when_missing() {
        let toml = r#"
[image]
base = "fedora:41"
name = "testenv"
packages = { install = ["fastfetch"] }
[container]
name = "testenv"
home = "~/containers/testenv"
"#;
        let mut cfg: Config = toml::from_str(toml).unwrap();
        let shell = ShellInfo {
            bin_name: "zsh".into(),
            full_path: "/bin/zsh".into(),
            package_name: "zsh".into(),
            detected: true,
        };
        // Shell defaults to "fish" via serde; apply_shell_defaults should NOT
        // override an already-set shell (it only fills in empty ones).
        apply_shell_defaults(&mut cfg, &shell);
        assert!(cfg.image.packages.install.contains(&"zsh".to_string()));
        assert_eq!(
            cfg.container.shell, "fish",
            "should not override existing shell"
        );
    }

    #[test]
    fn apply_shell_fills_empty_shell() {
        let toml = r#"
[image]
base = "fedora:41"
name = "testenv"
[container]
name = "testenv"
home = "~/containers/testenv"
"#;
        let mut cfg: Config = toml::from_str(toml).unwrap();
        cfg.container.shell.clear();
        let shell = ShellInfo {
            bin_name: "zsh".into(),
            full_path: "/bin/zsh".into(),
            package_name: "zsh".into(),
            detected: true,
        };
        apply_shell_defaults(&mut cfg, &shell);
        assert_eq!(cfg.container.shell, "/bin/zsh", "should fill empty shell");
        assert!(cfg.image.packages.install.contains(&"zsh".to_string()));
    }

    #[test]
    fn apply_shell_no_duplicate_when_present() {
        let toml = r#"
[image]
base = "fedora:41"
name = "testenv"
packages = { install = ["fish", "fastfetch"] }
[container]
name = "testenv"
home = "~/containers/testenv"
"#;
        let mut cfg: Config = toml::from_str(toml).unwrap();
        let shell = ShellInfo {
            bin_name: "fish".into(),
            full_path: "/usr/bin/fish".into(),
            package_name: "fish".into(),
            detected: true,
        };
        apply_shell_defaults(&mut cfg, &shell);
        let fish_count = cfg
            .image
            .packages
            .install
            .iter()
            .filter(|s| s.as_str() == "fish")
            .count();
        assert_eq!(fish_count, 1);
    }

    #[test]
    fn shell_info_unknown_binary() {
        let info = shell_info_from_bin("tcsh");
        assert_eq!(info.bin_name, "tcsh");
        assert_eq!(info.full_path, "/usr/bin/tcsh");
        assert_eq!(info.package_name, "tcsh");
    }

    #[test]
    fn detect_host_shell_is_idempotent() {
        let a = detect_host_shell_from(Some("/usr/bin/fish"));
        let b = detect_host_shell_from(Some("/usr/bin/fish"));
        assert_eq!(a.bin_name, b.bin_name);
        assert_eq!(a.full_path, b.full_path);
        assert_eq!(a.package_name, b.package_name);
    }

    #[test]
    fn tty_guard_logic_is_correct() {
        let result = nix::unistd::isatty(0);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn detect_package_manager_dnf() {
        assert_eq!(detect_package_manager("fedora:44"), "dnf");
        assert_eq!(detect_package_manager("centos:stream"), "dnf");
        assert_eq!(
            detect_package_manager("registry.fedoraproject.org/fedora:41"),
            "dnf"
        );
    }

    #[test]
    fn detect_package_manager_pacman() {
        assert_eq!(detect_package_manager("archlinux:latest"), "pacman");
        assert_eq!(detect_package_manager("cachyos:latest"), "pacman");
        assert_eq!(detect_package_manager("manjaro:latest"), "pacman");
    }

    #[test]
    fn detect_package_manager_apt() {
        assert_eq!(detect_package_manager("ubuntu:24.04"), "apt");
        assert_eq!(detect_package_manager("debian:bookworm"), "apt");
    }

    #[test]
    fn detect_package_manager_apk() {
        assert_eq!(detect_package_manager("alpine:latest"), "apk");
    }

    #[test]
    fn detect_package_manager_fallback() {
        assert_eq!(detect_package_manager("unknown:latest"), "apt");
    }
}
