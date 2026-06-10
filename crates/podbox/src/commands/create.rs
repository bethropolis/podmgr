use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use podbox::codegen::distros;
use podbox::config::{self, Config};
use podbox::error::PodboxError;

/// Build image, install Quadlet, and start the container.
fn finish_create(cfg: &Config, container_name: &str, dry_run: bool, no_start: bool) -> Result<()> {
    if dry_run {
        println!("podbox build");
    } else {
        let local_tag = format!("localhost/podbox-{}:latest", cfg.image.name);
        if !podbox::podman::image_exists(&local_tag).unwrap_or(false) {
            let env = podbox::env::resolve()?;
            let xdg = podbox::xdg::resolve(&cfg.integration.xdg_dirs)?;
            podbox::build::run(cfg, &env, &xdg, false, false)?;
        } else {
            println!("Image already exists, skipping build.");
        }
    }

    if dry_run {
        println!("podbox enable");
    } else {
        println!("Installing Quadlet files...");
        let env = podbox::env::resolve()?;
        let xdg = podbox::xdg::resolve(&cfg.integration.xdg_dirs)?;
        podbox::quadlet_install::install(cfg, &env, &xdg, false)?;
    }

    if no_start {
        println!("Container created but not started (--no-start).");
        println!(
            "Run `podbox shell {}` to start and enter it.",
            container_name
        );
    } else if dry_run {
        println!("podman start {}", container_name);
    } else {
        println!("Starting container...");
        if which::which("systemctl").is_ok() {
            let args = podbox::process::args(&["--user", "start", container_name]);
            podbox::process::spawn_interactive("systemctl", &args)?;
        } else {
            let args = podbox::process::args(&["start", container_name]);
            podbox::process::spawn_interactive("podman", &args)?;
        }
        println!("Container '{}' is running!", container_name);
        println!("Run `podbox shell` to enter.");
    }

    if !dry_run {
        let _ = config::write_active_context(container_name);
    }

    Ok(())
}

pub(super) fn read_profile_content(profile: &str) -> Result<String> {
    if profile.contains('/') || profile.contains('\\') {
        std::fs::read_to_string(Path::new(profile))
            .with_context(|| format!("failed to read profile '{}'", profile))
    } else {
        let found = podbox::profiles::find(profile).ok_or_else(|| {
            let names = podbox::profiles::list_names();
            anyhow::anyhow!(
                "Unknown profile '{}'. Available profiles: {}",
                profile,
                names.join(", ")
            )
        })?;
        Ok(found.toml)
    }
}

pub(super) fn derive_container_name(image: &str, custom_name: Option<&str>) -> String {
    if let Some(name) = custom_name {
        return name.to_string();
    }
    let image_part = image.split_once(':').map(|(n, _)| n).unwrap_or(image);
    let short = image_part.split('/').next_back().unwrap_or(image_part);
    let tag = image.split_once(':').map(|(_, t)| t).unwrap_or("latest");
    if tag == "latest" || tag.is_empty() {
        short.to_string()
    } else {
        format!("{}-{}", short, tag.replace('.', "-"))
    }
}

pub(super) fn detect_package_manager(image: &str) -> podbox::config::PackageManager {
    distros::detect_package_manager(image)
}

/// Initialize a new container config.
pub fn run_init(
    dry_run: bool,
    image: Option<&str>,
    name: Option<&str>,
    interactive: bool,
    profile: Option<&str>,
) -> Result<()> {
    let shell_info = podbox::wizard::detect_host_shell();
    if !shell_info.detected && !interactive {
        eprintln!("Note: $SHELL not set or unrecognized, defaulting to fish.");
    }

    if interactive {
        if !distros::is_tty() {
            anyhow::bail!("--interactive requires a TTY (stdin is not a terminal)");
        }
        let profiles = podbox::profiles::all();
        let result = podbox::wizard::run_wizard(&profiles, &shell_info)?;
        if !result.confirmed {
            let toml = toml::to_string_pretty(&result.config)?;
            println!("{}", toml);
            return Ok(());
        }
        let config_dir = config::config_dir();
        let config_path = config_dir.join(format!("{}.toml", result.name));
        if config_path.exists() && !dry_run {
            anyhow::bail!(
                "Config already exists at '{}'. Remove it first.",
                config_path.display()
            );
        }
        if dry_run {
            let toml = toml::to_string_pretty(&result.config)?;
            println!("Would write to: {}", config_path.display());
            println!("---\n{}", toml);
            return Ok(());
        }
        std::fs::create_dir_all(&config_dir)?;
        result.config.validate()?;
        let toml = toml::to_string_pretty(&result.config)?;
        std::fs::write(&config_path, &toml)?;
        println!("Created: {}", config_path.display());
        println!(
            "Run `podbox start -C {}` to build, enable, and start.",
            result.name
        );
        return Ok(());
    }

    if let Some(p) = profile {
        let profile_content = read_profile_content(p)?;
        let mut cfg = Config::parse(&profile_content)?;
        podbox::wizard::apply_shell_defaults(&mut cfg, &shell_info);
        let toml_str = toml::to_string_pretty(&cfg)?;
        let container_name = name.unwrap_or(&cfg.container.name).to_string();
        let config_dir = config::config_dir();
        let config_path = config_dir.join(format!("{}.toml", container_name));

        if config_path.exists() && !dry_run {
            anyhow::bail!(
                "Config already exists at '{}'. Remove it first or use a different name.",
                config_path.display()
            );
        }

        if dry_run {
            println!("Would create: {}", config_path.display());
            println!("---\n{}", toml_str);
            return Ok(());
        }

        std::fs::create_dir_all(&config_dir)?;
        std::fs::write(&config_path, &toml_str)?;
        println!("Created config: {}", config_path.display());
        println!();
        println!(
            "Profile created! Run `podbox create {}` or `podbox start` to spin it up.",
            container_name
        );
        return Ok(());
    }

    if image.is_none() {
        let profiles = podbox::profiles::all();
        println!("Available profiles:");
        for p in &profiles {
            println!("  {:<8} {}  —  {}", p.name, p.label, p.description);
        }
        println!();
        println!("Usage:");
        println!("  podbox init <image>         Create a custom container (e.g. fedora:44)");
        println!("  podbox init --profile <name>  Create from a prebuilt profile");
        println!("  podbox init -i               Interactive wizard");
        anyhow::bail!("Specify a base image or use --profile.");
    }

    let base = image.unwrap();
    let container_name = derive_container_name(base, name);

    let mut cfg = Config::embedded();
    cfg.image.base = base.to_string();
    cfg.image.name = container_name.clone();
    cfg.container.name = container_name.clone();
    cfg.container.home = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("containers")
        .join(&container_name);
    cfg.image.packages.manager = detect_package_manager(base);

    cfg.container.shell.clear();
    podbox::wizard::apply_shell_defaults(&mut cfg, &shell_info);
    cfg.validate()?;
    let toml_str = toml::to_string_pretty(&cfg)?;
    let config_dir = config::config_dir();
    let config_path = config_dir.join(format!("{}.toml", container_name));

    if config_path.exists() && !dry_run {
        let alt = format!("{}-alt", container_name);
        anyhow::bail!(
            "Config already exists at '{}'.\n\
             Use --name to specify a different name (e.g. --name {}).",
            config_path.display(),
            alt
        );
    }

    if dry_run {
        println!("Would create: {}", config_path.display());
        println!("---\n{}", toml_str);
        return Ok(());
    }

    std::fs::create_dir_all(&config_dir)?;
    std::fs::write(&config_path, &toml_str)?;
    println!("Created config: {}", config_path.display());
    println!();
    println!(
        "Container created! Run `podbox create {}` or `podbox start` to spin it up.",
        container_name
    );

    Ok(())
}

/// Create a container: pull profile/image, build, install Quadlet, and start.
pub fn run_create(
    dry_run: bool,
    image: &str,
    name: Option<&str>,
    packages: Option<&str>,
    no_start: bool,
) -> Result<()> {
    let is_profile = !image.contains('/') && !image.contains('\\');

    if is_profile && podbox::profiles::find(image).is_some() {
        let profile_content = read_profile_content(image)?;

        let shell_info = podbox::wizard::detect_host_shell();
        if !shell_info.detected {
            eprintln!("Note: $SHELL not set or unrecognized, defaulting to fish.");
        }

        let mut cfg = Config::parse(&profile_content)?;
        podbox::wizard::apply_shell_defaults(&mut cfg, &shell_info);
        if let Some(pkgs) = packages {
            for pkg in pkgs.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                if !cfg.image.packages.install.contains(&pkg.to_string()) {
                    cfg.image.packages.install.push(pkg.to_string());
                }
            }
        }
        let container_name = name.unwrap_or(&cfg.container.name).to_string();
        cfg.container.name = container_name.clone();
        cfg.image.name = container_name.clone();
        let config_dir = config::config_dir();
        let config_path = config_dir.join(format!("{}.toml", container_name));

        if config_path.exists() && !dry_run {
            eprintln!(
                "Config already exists at '{}'. Reusing existing config.",
                config_path.display()
            );
        } else {
            let config_toml = toml::to_string_pretty(&cfg)?;
            if dry_run {
                println!("Would create config: {}", config_path.display());
                println!("---\n{}", config_toml);
            } else {
                std::fs::create_dir_all(&config_dir)?;
                std::fs::write(&config_path, &config_toml)?;
                println!("Created config: {}", config_path.display());
            }
        }

        return finish_create(&cfg, &container_name, dry_run, no_start);
    }

    if is_profile {
        eprintln!(
            "Note: '{}' is not a known profile (available: {}). Treating it as an image reference to pull.",
            image,
            podbox::profiles::list_names().join(", ")
        );
    }

    let config_dir = config::config_dir();
    let existing = match name {
        Some(n) => config_dir.join(format!("{}.toml", n)),
        None => config_dir.join(format!("{}.toml", image)),
    };
    if existing.exists() {
        let stem = existing.file_stem().unwrap_or_default().to_string_lossy();
        anyhow::bail!(
            "Config '{}' already exists at {}.\n\
             Use `podbox build -C {}` to build, or `podbox start -C {}` to start.",
            stem,
            existing.display(),
            stem,
            stem
        );
    }

    if dry_run {
        println!("podman pull {}", image);
        return Ok(());
    }

    println!("Pulling image...");
    let status = std::process::Command::new("podman")
        .args(["pull", image])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|_| PodboxError::PullFailed(image.into()))?;

    if !status.success() {
        return Err(PodboxError::PullFailed(image.into()).into());
    }

    if let Some(n) = name {
        let container_name = n.to_string();
        let shell_info = podbox::wizard::detect_host_shell();
        let mut cfg = Config::embedded();
        cfg.image.base = image.to_string();
        cfg.image.name = container_name.clone();
        cfg.container.name = container_name.clone();
        cfg.container.home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("containers")
            .join(&container_name);
        cfg.image.packages.manager = detect_package_manager(image);
        cfg.container.shell.clear();
        podbox::wizard::apply_shell_defaults(&mut cfg, &shell_info);
        if let Some(pkgs) = packages {
            for pkg in pkgs.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                if !cfg.image.packages.install.contains(&pkg.to_string()) {
                    cfg.image.packages.install.push(pkg.to_string());
                }
            }
        }
        cfg.validate()?;

        let config_dir = config::config_dir();
        let config_path = config_dir.join(format!("{}.toml", container_name));
        if config_path.exists() {
            eprintln!(
                "Config already exists at '{}'. Reusing existing config.",
                config_path.display()
            );
        } else {
            std::fs::create_dir_all(&config_dir)?;
            let toml_str = toml::to_string_pretty(&cfg)?;
            std::fs::write(&config_path, &toml_str)?;
            println!("Created config: {}", config_path.display());
        }

        println!("Image '{}' pulled and configured.", image);
        return finish_create(&cfg, &container_name, dry_run, no_start);
    }

    println!("Image '{}' pulled.", image);
    let suggested = derive_container_name(image, None);
    println!(
        "Run `podbox init {} --name <name>` to create a config (e.g. --name {}).",
        image, suggested
    );
    Ok(())
}
