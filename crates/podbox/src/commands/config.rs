use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap_complete::generate;

use podbox::cli::{Cli, ExportCommand};
use podbox::codegen::quadlet;
use podbox::config::{self, Config};
use podbox::env::HostEnv;
use podbox::error::PodboxError;
use podbox::xdg::ResolvedXdgDirs;

/// Print the path to the definition file.
pub fn run_find_definition(name: Option<&str>) -> Result<()> {
    match name {
        Some(n) => {
            let path = config::config_dir().join(format!("{}.toml", n));
            if path.exists() {
                println!("{}", path.display());
            } else {
                println!("(no config found for '{}')", n);
            }
        }
        None => match config::find_definition() {
            Some(path) => println!("{}", path.display()),
            None => println!("(embedded default)"),
        },
    }
    Ok(())
}

/// Generate shell completions.
pub fn run_completions(shell: clap_complete::shells::Shell) -> Result<()> {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let name = cmd.get_name().to_string();
    generate(shell, &mut cmd, name, &mut std::io::stdout());
    Ok(())
}

/// List containers (Quadlet or plain podman).
pub fn run_list() -> Result<()> {
    let ver = podbox::podman::podman_version().ok();
    if ver.is_some_and(|v| v.at_least(5, 6)) {
        let status = std::process::Command::new("podman")
            .args([
                "quadlet",
                "list",
                "--format",
                "table {{.Name}}\t{{.Path}}\t{{.Status}}\t{{.UnitName}}",
            ])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|_| PodboxError::PodmanNotFound)?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    } else {
        let status = std::process::Command::new("podman")
            .args([
                "ps",
                "-a",
                "--filter",
                "label=podbox.protocol_version",
                "--format",
                "table {{.Names}}\t{{.Image}}\t{{.Status}}",
            ])
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|_| PodboxError::PodmanNotFound)?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}

/// Export an app (desktop entry) or binary from the container.
pub fn run_export(name: &str, config: Option<&Config>, export_cmd: &ExportCommand) -> Result<()> {
    match export_cmd {
        ExportCommand::App {
            name: app_name,
            all,
        } => {
            if *all {
                let cfg =
                    config.ok_or_else(|| anyhow::anyhow!("Config required for --all export"))?;
                for app in &cfg.integration.export.apps {
                    if let Err(e) = podbox::export::export_app(name, app) {
                        eprintln!("Warning: export app '{}' failed: {}", app, e);
                    }
                }
            } else if let Some(app_name) = app_name {
                podbox::export::export_app(name, app_name)?;
            }
        }
        ExportCommand::Bin {
            name: bin_name,
            all,
        } => {
            if *all {
                let cfg =
                    config.ok_or_else(|| anyhow::anyhow!("Config required for --all export"))?;
                for bin in &cfg.integration.export.bins {
                    if let Err(e) = podbox::export::export_bin(name, bin) {
                        eprintln!("Warning: export bin '{}' failed: {}", bin, e);
                    }
                }
            } else if let Some(bin_name) = bin_name {
                podbox::export::export_bin(name, bin_name)?;
            }
        }
        ExportCommand::Clean => {
            podbox::export::unexport_all(name)?;
        }
    }
    Ok(())
}

/// Run the host-side socket server for a container.
pub fn run_serve(cli_config_path: Option<&PathBuf>, serve_name: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("podbox serve {}", serve_name);
        return Ok(());
    }
    let serve_config = if let Some(path) = cli_config_path {
        Config::load(path)?
    } else {
        let config_dir = config::config_dir();
        let config_path = config_dir.join(format!("{}.toml", serve_name));
        Config::load(&config_path)?
    };
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let uid = nix::unistd::getuid().as_raw();
        format!("/run/user/{}", uid)
    });
    let socket_path = std::path::PathBuf::from(&xdg_runtime)
        .join("podbox")
        .join(format!("{}.sock", serve_name));
    podbox::socket_host::run(&socket_path, &serve_config.integration)?;
    Ok(())
}

/// Pull a container image and tag it for podbox use.
pub fn run_pull(config: &Config, image: &Option<String>, dry_run: bool) -> Result<()> {
    let image_ref = match image {
        Some(ref img) => img.clone(),
        None => match config.image.source() {
            podbox::config::ImageSource::Prebuilt { ref_str } => ref_str,
            podbox::config::ImageSource::Build { base } => base,
        },
    };
    if dry_run {
        println!("podman pull {}", image_ref);
        println!(
            "podman tag {} localhost/podbox-{}:latest",
            image_ref, config.image.name
        );
        return Ok(());
    }
    let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
    println!("Pulling {}...", image_ref);
    let status = std::process::Command::new("podman")
        .args(["pull", &image_ref])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|_| PodboxError::PullFailed(image_ref.clone()))?;
    if !status.success() {
        return Err(PodboxError::PullFailed(image_ref.clone()).into());
    }
    println!("Tagging {} as {}...", image_ref, local_tag);
    let tag_status = std::process::Command::new("podman")
        .args(["tag", &image_ref, &local_tag])
        .status()
        .map_err(|_| PodboxError::TagFailed(image_ref.clone()))?;
    if !tag_status.success() {
        return Err(PodboxError::TagFailed(image_ref.clone()).into());
    }
    let context_dir = podbox::build::build_context_dir(&config.image.name);
    std::fs::create_dir_all(&context_dir)?;
    let digest = podbox::podman::image_digest(&local_tag)?;
    let lock = podbox::lock::LockFile {
        config_checksum: podbox::build::checksum(&image_ref),
        image_digest: digest,
    };
    let lock_path = context_dir.join(".podbox.lock");
    podbox::lock::write(&lock_path, &lock)?;
    println!("Lock file written to {}", lock_path.display());
    Ok(())
}

/// Convert a host path to a container path (or vice versa).
pub fn run_translate_path(
    _config: &Config,
    xdg: &ResolvedXdgDirs,
    to_container: bool,
    to_host: bool,
    path_str: &str,
) -> Result<()> {
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string());
    let home_in_container = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", username)
    };
    let host_path = if Path::new(path_str).is_relative() {
        std::env::current_dir()
            .map(|d| d.join(path_str))
            .unwrap_or_else(|_| PathBuf::from(path_str))
    } else {
        PathBuf::from(path_str)
    };

    if to_container {
        let host_to_container: Vec<(&str, &PathBuf)> = vec![
            ("Documents", &xdg.documents),
            ("Downloads", &xdg.downloads),
            ("Pictures", &xdg.pictures),
            ("Music", &xdg.music),
            ("Videos", &xdg.videos),
            ("Desktop", &xdg.desktop),
            ("Projects", &xdg.projects),
        ]
        .into_iter()
        .filter_map(|(name, opt)| opt.as_ref().map(|p| (name, p)))
        .collect();

        for (dir_name, host_dir) in &host_to_container {
            if let Ok(relative) = host_path.strip_prefix(host_dir) {
                let container_path =
                    format!("{}/{}/{}", home_in_container, dir_name, relative.display());
                println!("{container_path}");
                return Ok(());
            }
        }

        let host_home = dirs::home_dir().unwrap_or_default();
        if let Ok(relative) = host_path.strip_prefix(&host_home) {
            let container_path = format!("{}/{}", home_in_container, relative.display());
            println!("{container_path}");
            return Ok(());
        }

        println!("{path_str}");
    }

    if to_host {
        for (dir_name, host_dir) in [
            ("Documents", &xdg.documents),
            ("Downloads", &xdg.downloads),
            ("Pictures", &xdg.pictures),
            ("Music", &xdg.music),
            ("Videos", &xdg.videos),
            ("Desktop", &xdg.desktop),
        ] {
            if let Some(ref host_dir) = host_dir {
                let container_prefix = format!("{}/{}/", home_in_container, dir_name);
                if path_str.starts_with(&container_prefix) {
                    let relative = path_str.strip_prefix(&container_prefix).unwrap_or("");
                    let host_path = host_dir.join(relative);
                    println!("{}", host_path.display());
                    return Ok(());
                }
            }
        }

        println!("{path_str}");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
//  Init / Create (config management)
// ---------------------------------------------------------------------------

fn read_profile_content(profile: &str) -> Result<String> {
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

fn derive_container_name(image: &str, custom_name: Option<&str>) -> String {
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

    // Set active context so the user can run `podbox exec` immediately.
    if !dry_run {
        let _ = config::write_active_context(container_name);
    }

    Ok(())
}

/// Initialize a new container config.
pub fn run_init(
    dry_run: bool,
    image: Option<&str>,
    name: Option<&str>,
    interactive: bool,
    profile: Option<&str>,
) -> Result<()> {
    let shell_info = podbox::init_wizard::detect_host_shell();
    if !shell_info.detected && !interactive {
        eprintln!("Note: $SHELL not set or unrecognized, defaulting to fish.");
    }

    if interactive {
        if !nix::unistd::isatty(0).unwrap_or(false) {
            anyhow::bail!("--interactive requires a TTY (stdin is not a terminal)");
        }
        let profiles = podbox::profiles::all();
        let result = podbox::init_wizard::run_wizard(&profiles, &shell_info)?;
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
        podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
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
    cfg.image.packages.manager = detect_package_manager(base).to_string();

    // Clear default shell so apply_shell_defaults fills in the host shell
    cfg.container.shell.clear();
    podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
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

/// Set or show the active context.
pub fn run_use(name: Option<String>, clear: bool, dry_run: bool) -> Result<()> {
    if clear {
        if dry_run {
            println!("Would clear active context.");
            return Ok(());
        }
        config::clear_active_context()?;
        println!("Active context cleared.");
        return Ok(());
    }

    match name {
        Some(n) => {
            let config_path = config::config_dir().join(format!("{}.toml", n));
            if !config_path.exists() {
                anyhow::bail!("Config '{}' not found at {}", n, config_path.display());
            }
            if dry_run {
                println!("Would set active context to '{}'.", n);
                return Ok(());
            }
            config::write_active_context(&n)?;
            println!("Active context set to '{}'.", n);
        }
        None => match config::read_active_context() {
            Some(n) => println!("{}", n),
            None => println!("No active context set."),
        },
    }
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

        let shell_info = podbox::init_wizard::detect_host_shell();
        if !shell_info.detected {
            eprintln!("Note: $SHELL not set or unrecognized, defaulting to fish.");
        }

        let mut cfg = Config::parse(&profile_content)?;
        podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
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

    // Check for existing local config before attempting OCI pull
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
        let shell_info = podbox::init_wizard::detect_host_shell();
        let mut cfg = Config::embedded();
        cfg.image.base = image.to_string();
        cfg.image.name = container_name.clone();
        cfg.container.name = container_name.clone();
        cfg.container.home = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("containers")
            .join(&container_name);
        cfg.image.packages.manager = detect_package_manager(image).to_string();
        cfg.container.shell.clear();
        podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
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

/// Clone an existing container config to a new name.
pub fn run_clone(src: &str, dst: &str, copy_home: bool, dry_run: bool) -> Result<()> {
    let config_dir = config::config_dir();
    let src_path = config_dir.join(format!("{}.toml", src));
    let dst_path = config_dir.join(format!("{}.toml", dst));

    if !src_path.exists() {
        anyhow::bail!(
            "Source config '{}' not found at {}",
            src,
            src_path.display()
        );
    }
    if dst_path.exists() {
        anyhow::bail!(
            "Destination config '{}' already exists at {}",
            dst,
            dst_path.display()
        );
    }

    if dry_run {
        println!("Would clone '{}' to '{}'", src, dst);
        if copy_home {
            println!("Would also copy home directory contents.");
        }
        return Ok(());
    }

    let content = std::fs::read_to_string(&src_path)?;
    let mut cfg: Config = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse '{}': {}", src_path.display(), e))?;

    let new_home = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join("containers")
        .join(dst);
    cfg.image.name = dst.to_string();
    cfg.container.name = dst.to_string();
    cfg.container.home = new_home.clone();

    let new_content = toml::to_string_pretty(&cfg)?;
    std::fs::write(&dst_path, &new_content)?;
    println!("Created config: {}", dst_path.display());

    if copy_home {
        let home = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("~"))
            .join("containers")
            .join(src);
        if home.exists() {
            let entries = std::fs::read_dir(&home).map(|e| e.count()).unwrap_or(0);
            if entries > 500 {
                eprintln!(
                    "Warning: source home has {} items. Copying may take a while.",
                    entries
                );
            }
            let status = std::process::Command::new("cp")
                .args(["-a", &home.to_string_lossy(), &new_home.to_string_lossy()])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to copy home directory contents.");
            }
            println!("Home directory copied.");
        } else {
            eprintln!(
                "Warning: source home '{}' does not exist, skipping copy.",
                home.display()
            );
        }
    }

    println!();
    println!("Cloned '{}' → '{}'", src, dst);
    println!("Run `podbox build {}` to build and start.", dst);
    Ok(())
}

/// Inspect container config, Quadlet, or computed env.
pub fn run_inspect(
    config: &Config,
    _name: &str,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    show_config: bool,
    show_quadlet: bool,
    show_env: bool,
) -> Result<()> {
    let all = !show_config && !show_quadlet && !show_env;

    if all || show_config {
        println!("--- Config ---");
        let toml_str = toml::to_string_pretty(config)?;
        println!("{}", toml_str);
    }

    if all || show_quadlet {
        println!("--- Quadlet (.container) ---");
        let q = quadlet::generate_container(config, env, xdg);
        println!("{}", q);
        println!();
        println!("--- Quadlet (.socket) ---");
        let s = quadlet::generate_socket(config);
        println!("{}", s);
    }

    if all || show_env {
        println!("--- Environment ---");
        println!("Container name:  {}", config.container.name);
        let image_ref = match config.image.source() {
            podbox::config::ImageSource::Build { base } => format!("build:{}", base),
            podbox::config::ImageSource::Prebuilt { ref_str } => ref_str.clone(),
        };
        println!("Image ref:       {}", image_ref);
        println!("Image source:    {:?}", config.image.source());
        println!("Quadlet:         {}", config.lifecycle.quadlet);
        println!("Auto-start:      {}", config.lifecycle.autostart);
        println!("Auto-update:     {}", config.lifecycle.auto_update);
        println!();
        println!("XDG_RUNTIME_DIR: {}", env.xdg_runtime_dir.display());
        if let Some(ref w) = env.wayland_display {
            println!("WAYLAND_DISPLAY: {}", w);
        }
        if env.gpu_has_dri {
            println!("GPU (DRI):       yes");
        }
        if env.gpu_has_nvidia {
            println!("GPU (NVIDIA):    yes");
        }
        if let Some(ref dbus) = env.dbus_socket {
            println!("D-Bus socket:    {}", dbus.display());
        }
        if env.gpg_agent_socket.is_some() {
            println!("GPG agent:       available");
        }
        if let Some(ref shell) = env.host_shell {
            println!("Host shell:      {}", shell);
        }
        if let Some(ref locale) = env.host_locale {
            println!("Host locale:     {}", locale);
        }
    }

    Ok(())
}
