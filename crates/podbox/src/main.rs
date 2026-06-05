use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use clap_complete::generate;

use podbox::cli::{Cli, Command, ExportCommand};
use podbox::config::{self, Config};
use podbox::error::PodboxError;

use podbox::socket_host;
use podbox::xdg::ResolvedXdgDirs;

mod commands;

pub const VERSION: &str = env!("PODBOX_VERSION");

fn main() -> ExitCode {
    let result = run();
    if let Err(e) = result {
        eprintln!("Error: {:#}", e);
        exit_code_for_error(&e)
    } else {
        ExitCode::SUCCESS
    }
}

fn exit_code_for_error(err: &anyhow::Error) -> ExitCode {
    if let Some(podbox_err) = err.downcast_ref::<PodboxError>() {
        match podbox_err {
            PodboxError::DefinitionNotFound(_)
            | PodboxError::DefinitionReadFailed(_)
            | PodboxError::DefinitionParseFailed(_) => ExitCode::from(2),
            PodboxError::ContainerMissing(_) => ExitCode::from(3),
            PodboxError::BuildFailed(_) | PodboxError::PodmanInspectFailed { .. } => {
                ExitCode::from(4)
            }
            PodboxError::GuestBinaryNotFound | PodboxError::PodmanNotFound => ExitCode::from(5),
            PodboxError::PullFailed(..) | PodboxError::TagFailed(..) => ExitCode::from(6),
            _ => ExitCode::FAILURE,
        }
    } else {
        ExitCode::FAILURE
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Command::FindDefinition => {
            match config::find_definition() {
                Some(path) => println!("{}", path.display()),
                None => println!("(embedded default)"),
            }
            return Ok(());
        }

        Command::Completions { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            let name = cmd.get_name().to_string();
            let shell_generator: clap_complete::shells::Shell = (*shell).into();
            generate(shell_generator, &mut cmd, name, &mut std::io::stdout());
            return Ok(());
        }

        Command::Init {
            image,
            name,
            interactive,
            profile,
        } => {
            return run_init(
                cli.dry_run,
                image.as_deref(),
                name.as_deref(),
                *interactive,
                profile.as_deref(),
            );
        }

        Command::Create {
            image,
            name,
            no_start,
        } => {
            return run_create(cli.dry_run, image, name.as_deref(), *no_start);
        }

        Command::List => {
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
                // 5.5 fallback: list via podman ps
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
            return Ok(());
        }

        _ => {}
    }

    // Resolve container name from Enter or Build commands if provided
    let enter_name = match &cli.command {
        Command::Enter { name } => Some(name.clone()),
        Command::Build { name, .. } => name.clone(),
        _ => None,
    };

    // Load config for all other commands
    let mut config = if let Some(ref path) = cli.config {
        match Config::load(path) {
            Ok(cfg) => cfg,
            Err(e)
                if e.downcast_ref::<PodboxError>()
                    .is_some_and(|pe| matches!(pe, PodboxError::DefinitionNotFound(_))) =>
            {
                eprintln!(
                    "Warning: config file not found at '{}', using embedded default.",
                    path.display()
                );
                Config::embedded()
            }
            Err(e) => return Err(e),
        }
    } else if let Some(ref container_name) = enter_name.or_else(|| cli.container.clone()) {
        let config_dir = config::config_dir();
        let config_path = config_dir.join(format!("{}.toml", container_name));
        Config::load(&config_path).map_err(|e| {
            anyhow::anyhow!(
                "{}\n\nHint: Use `--config <PATH>` to specify a config file, or `-C <NAME>` to use a config from {}",
                e,
                config_dir.display()
            )
        })?
    } else {
        match config::find_definition() {
            Some(path) => Config::load(&path)?,
            None => {
                eprintln!("No definition file found, using embedded default. Create .podbox.toml to customize.");
                Config::embedded()
            }
        }
    };

    let name = config.container.name.clone();

    // Apply image label defaults (best-effort; image may not be pulled yet)
    let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
    if let Ok(true) = podbox::podman::image_exists(&local_tag) {
        if let Ok(labels) = podbox::labels::fetch(&local_tag) {
            podbox::labels::apply_defaults(&mut config, &labels);
        }
    }

    let env = podbox::env::resolve()?;
    let xdg = podbox::xdg::resolve(&config.integration.xdg_dirs)?;

    match &cli.command {
        Command::Build { name: _, rebuild } => {
            commands::lifecycle::run_build(&config, &env, &xdg, cli.dry_run, *rebuild)?;
        }

        Command::Enable => {
            commands::lifecycle::run_enable(&config, &env, &xdg, cli.dry_run)?;
        }

        Command::Disable => {
            commands::lifecycle::run_disable(&name)?;
        }

        Command::Start => {
            commands::lifecycle::run_start(&config, &env, &xdg, &name, cli.dry_run)?;
        }

        Command::Stop => {
            commands::lifecycle::run_stop(&name, cli.dry_run)?;
        }

        Command::Shell | Command::Enter { .. } => {
            commands::runtime::run_shell_enter(&config, &name, cli.dry_run)?;
        }

        Command::Exec { args: cmd_args } => {
            commands::runtime::run_exec(&env, &name, cmd_args, cli.dry_run)?;
        }

        Command::Run { app, app_args } => {
            commands::runtime::run_run(&env, &name, app, app_args, cli.dry_run)?;
        }

        Command::Status => {
            commands::runtime::run_status(&name, cli.dry_run)?;
        }

        Command::Logs { follow, tail } => {
            commands::runtime::run_logs(&name, *follow, *tail, cli.dry_run)?;
        }

        Command::Export { export_cmd } => match export_cmd {
            ExportCommand::App { name: app_name } => {
                podbox::export::export_app(&name, app_name)?;
            }
            ExportCommand::Bin { name: bin_name } => {
                podbox::export::export_bin(&name, bin_name)?;
            }
            ExportCommand::Clean => {
                podbox::export::unexport_all(&name)?;
            }
        },

        Command::Remove { all, force } => {
            commands::lifecycle::run_remove(&config, &name, cli.dry_run, *all, *force)?;
        }

        Command::Serve { name: serve_name } => {
            if cli.dry_run {
                println!("podbox serve {}", serve_name);
                return Ok(());
            }
            let serve_config = if let Some(ref path) = cli.config {
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
            socket_host::run(&socket_path, &serve_config.integration)?;
        }

        Command::Pull { image } => {
            let image_ref = match image {
                Some(ref img) => config::resolve_image_ref(
                    img,
                    &config.image.prebuilt_registry,
                    &config.image.prebuilt_repo,
                ),
                None => config::resolve_image_ref_full(&config),
            };
            if cli.dry_run {
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
            // Write lock file in unified .podbox.lock format
            let context_dir = podbox::build::build_context_dir(&config.image.name);
            std::fs::create_dir_all(&context_dir)?;
            let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
            let digest = podbox::podman::image_digest(&local_tag)?;
            let lock = podbox::lock::LockFile {
                config_checksum: podbox::build::checksum(&image_ref),
                image_digest: digest,
            };
            let lock_path = context_dir.join(".podbox.lock");
            podbox::lock::write(&lock_path, &lock)?;
            println!("Lock file written to {}", lock_path.display());
            return Ok(());
        }

        Command::Doctor { fix } => {
            commands::runtime::run_doctor(&config, &env, *fix)?;
        }

        Command::TranslatePath {
            to_container,
            to_host,
            path,
        } => {
            translate_path(&config, &xdg, *to_container, *to_host, path)?;
        }

        Command::FindDefinition
        | Command::Completions { .. }
        | Command::Init { .. }
        | Command::Create { .. }
        | Command::List => unreachable!(),
    }

    Ok(())
}

fn translate_path(
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

/// Read a profile: if `profile` looks like a path (contains `/` or `\`),
/// read it from disk; otherwise look up a built-in profile by name.
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

fn run_init(
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
        let toml = toml::to_string_pretty(&result.config)?;
        std::fs::write(&config_path, &toml)?;
        println!("Created: {}", config_path.display());
        println!("Run `podbox create {}` to build and start.", result.name);
        return Ok(());
    }

    // --profile mode: load a named profile (prebuilt)
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

    // No image, no profile, no interactive: list available profiles
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

    // Non-prebuilt mode: create a custom config from a base image
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

    podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
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

/// Derive a container name from an image reference.
/// "ubuntu:24.04" → "ubuntu-24-04", "fedora:latest" → "fedora".
fn derive_container_name(image: &str, custom_name: Option<&str>) -> String {
    if let Some(name) = custom_name {
        return name.to_string();
    }
    let image_part = image.split_once(':').map(|(n, _)| n).unwrap_or(image);
    let short = image_part.split('/').last().unwrap_or(image_part);
    let tag = image.split_once(':').map(|(_, t)| t).unwrap_or("latest");
    if tag == "latest" || tag.is_empty() {
        short.to_string()
    } else {
        format!("{}-{}", short, tag.replace('.', "-"))
    }
}

/// Guess the package manager from the base image name.
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
fn finish_create(
    cfg: &Config,
    container_name: &str,
    dry_run: bool,
    no_start: bool,
) -> Result<()> {
    // Build / tag image
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

    // Install Quadlet files
    if dry_run {
        println!("podbox enable");
    } else {
        println!("Installing Quadlet files...");
        let env = podbox::env::resolve()?;
        let xdg = podbox::xdg::resolve(&cfg.integration.xdg_dirs)?;
        podbox::quadlet_install::install(cfg, &env, &xdg, false)?;
    }

    // Start container
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
            // Quadlet hasn't created the container yet — use systemctl to
            // trigger Quadlet's create-and-start pipeline.
            let args = podbox::process::args(&["--user", "start", container_name]);
            podbox::process::spawn_interactive("systemctl", &args)?;
        } else {
            let args = podbox::process::args(&["start", container_name]);
            podbox::process::spawn_interactive("podman", &args)?;
        }
        println!("Container '{}' is running!", container_name);
        println!("Run `podbox shell` to enter.");
    }

    Ok(())
}

fn run_create(dry_run: bool, image: &str, name: Option<&str>, no_start: bool) -> Result<()> {
    let is_profile = !image.contains('/') && !image.contains('\\');

    if is_profile && podbox::profiles::find(image).is_some() {
        let profile_content = read_profile_content(image)?;

        let shell_info = podbox::init_wizard::detect_host_shell();
        if !shell_info.detected {
            eprintln!("Note: $SHELL not set or unrecognized, defaulting to fish.");
        }

        let mut cfg = Config::parse(&profile_content)?;
        podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
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

    // Pull the image first
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

    // If --name was provided, auto-create a config and continue to enable/start
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
        podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);

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

    // No --name: just report the pull and tell user how to proceed
    println!("Image '{}' pulled.", image);
    let suggested = derive_container_name(image, None);
    println!(
        "Run `podbox init {} --name <name>` to create a config (e.g. --name {}).",
        image, suggested
    );
    Ok(())
}


