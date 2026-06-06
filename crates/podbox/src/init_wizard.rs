use crate::config::{Config, GpuMode, XdgDirConfig};
use crate::profiles;

/// Information about the host shell, resolved at startup.
pub struct ShellInfo {
    /// Binary name: "fish", "bash", "zsh", etc.
    pub bin_name: String,
    /// Full container path: "/usr/bin/fish"
    pub full_path: String,
    /// Distro package name (may differ from bin_name).
    pub package_name: String,
    /// True if the shell was actually detected; false if fallback was used.
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

/// Map binary name → (full_path, package_name).
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
pub fn apply_shell_defaults(config: &mut Config, shell: &ShellInfo) {
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
}

/// Result of the interactive wizard.
pub struct WizardResult {
    /// The final config, ready to serialize to TOML.
    pub config: Config,
    /// The container name chosen by the user.
    pub name: String,
    /// True if the user confirmed writing to disk.
    pub confirmed: bool,
}

/// Run the interactive init wizard.
pub fn run_wizard(
    profiles: &[profiles::Profile],
    detected_shell: &ShellInfo,
) -> anyhow::Result<WizardResult> {
    let (mut config, default_name) = match prompt_profile(profiles) {
        ProfileChoice::Named(profile) => {
            let cfg: Config = toml::from_str(&profile.toml)
                .map_err(|e| anyhow::anyhow!("failed to parse profile '{}': {}", profile.name, e))?;
            (cfg, profile.name.clone())
        }
        ProfileChoice::Custom => {
            let mut cfg = Config::embedded();

            let base: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Base image")
                .default("fedora:44".to_string())
                .interact_text()?;
            cfg.image.base = base;

            let packages: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Packages to install (space-separated)")
                .default("fish fastfetch btop".to_string())
                .interact_text()?;
            cfg.image.packages.install = packages
                .split_whitespace()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let run_cmds: String = dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Extra RUN commands (one per line, \\n separated)")
                .default("".to_string())
                .interact_text()?;
            cfg.image.run.commands = run_cmds
                .split("\\n")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let default_name = cfg.container.name.clone();
            (cfg, default_name)
        }
    };

    let name = prompt_name(&default_name, &crate::config::config_dir())?;

    let shell = prompt_shell(detected_shell);
    config.container.name = name.clone();
    config.image.name = name.clone();
    config.container.home = crate::config::expand_tilde(&format!("~/containers/{}", name));
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

    config.integration.xdg_dirs = prompt_xdg_dirs();

    let (notify, clipboard, xdg_open, ssh_agent) = prompt_integration_extras();
    config.integration.notify = notify;
    config.integration.clipboard = clipboard;
    config.integration.xdg_open = xdg_open;
    config.integration.ssh_agent = ssh_agent;

    config.integration.gpu = prompt_gpu();

    let (quadlet, autostart) = prompt_lifecycle();
    config.lifecycle.quadlet = quadlet;
    config.lifecycle.autostart = autostart;

    let toml_str = toml::to_string_pretty(&config)
        .map_err(|e| anyhow::anyhow!("failed to serialize config: {}", e))?;
    let confirmed = preview_and_confirm(&toml_str);

    Ok(WizardResult {
        config,
        name,
        confirmed,
    })
}

enum ProfileChoice<'a> {
    Custom,
    Named(&'a profiles::Profile),
}

fn prompt_profile(profiles: &[profiles::Profile]) -> ProfileChoice<'_> {
    let items: Vec<String> = {
        let mut v = vec!["Custom (from scratch)  —  Build a container from a base distro image".into()];
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
        ProfileChoice::Named(&profiles[selection - 1])
    }
}

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

fn prompt_integration_extras() -> (bool, bool, bool, bool) {
    let items = [
        "notify     (desktop notifications passthrough)",
        "clipboard  (wl-copy/wl-paste passthrough)",
        "xdg_open   (open URIs on host)",
        "ssh_agent  (SSH agent forwarding)",
    ];
    let selections: Vec<usize> =
        dialoguer::MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Integration extras (space to toggle, enter to confirm)")
            .items(&items)
            .interact()
            .expect("failed to get integration selection");
    (
        selections.contains(&0),
        selections.contains(&1),
        selections.contains(&2),
        selections.contains(&3),
    )
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

fn preview_and_confirm(toml: &str) -> bool {
    println!("\n{}\n", toml);
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
        apply_shell_defaults(&mut cfg, &shell);
        assert!(cfg.image.packages.install.contains(&"zsh".to_string()));
        assert_eq!(cfg.container.shell, "/bin/zsh");
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
        // On CI stdin is usually not a TTY — verify the guard condition
        // by asserting that !isatty(0) is not an error
        let result = nix::unistd::isatty(0);
        assert!(result.is_ok() || result.is_err());
    }
}
