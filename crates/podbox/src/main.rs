use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use clap_complete::generate;

use podbox::cli::{Cli, Command, ExportCommand};
use podbox::config::{self, Config};
use podbox::env::HostEnv;
use podbox::error::PodboxError;
use podbox::podman::{query_state, ContainerState};
use podbox::socket_host;
use podbox::xdg::ResolvedXdgDirs;

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
            podbox::build::run(&config, &env, &xdg, cli.dry_run, *rebuild)?;
            if !cli.dry_run && config.lifecycle.quadlet {
                println!("\nRun `podbox enable` to install Quadlet files.");
            }
        }

        Command::Enable => {
            podbox::quadlet_install::install(&config, &env, &xdg, cli.dry_run)?;
            if !cli.dry_run {
                println!(
                    "\nRun `podbox shell` to start and enter the container.",
                );
            }
        }

        Command::Disable => {
            podbox::quadlet_install::uninstall(&name)?;
        }

        Command::Start => {
            if cli.dry_run {
                println!("podman start {}", name);
                return Ok(());
            }

            // Auto-heal: build image if missing
            let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
            if !podbox::podman::image_exists(&local_tag).unwrap_or(false) {
                println!("Image not found, building first...");
                podbox::build::run(&config, &env, &xdg, false, false)?;
            }

            // Auto-heal: install Quadlet files if missing
            let qdir = dirs::config_dir()
                .unwrap_or_else(|| podbox::config::expand_tilde("~/.config"))
                .join("containers/systemd");
            let container_file = qdir.join(format!("{}.container", name));
            if !container_file.exists() {
                println!("Quadlet files not found, installing...");
                podbox::quadlet_install::install(&config, &env, &xdg, false)?;
            }

            let args: Vec<OsString> = vec!["start".into(), name.clone().into()];
            podbox::process::spawn_interactive("podman", &args)?;
        }

        Command::Stop => {
            if cli.dry_run {
                println!("podman stop {}", name);
                return Ok(());
            }
            let args: Vec<OsString> = vec!["stop".into(), name.clone().into()];
            podbox::process::spawn_interactive("podman", &args)?;
        }

        Command::Shell | Command::Enter { .. } => {
            let env = podbox::env::resolve()?;
            let tty_flag = if nix::unistd::isatty(0).unwrap_or(false) {
                OsString::from("-it")
            } else {
                OsString::from("-i")
            };
            let home_in_container: OsString = format!("/home/{}", env.username).into();
            if cli.dry_run {
                let exec_args: Vec<OsString> = vec![
                    "exec".into(),
                    tty_flag,
                    "--workdir".into(),
                    home_in_container.clone(),
                    name.clone().into(),
                    config.container.shell.clone().into(),
                ];
                println!("podman {}", args_to_string(&exec_args));
                return Ok(());
            }
            ensure_running(&name, cli.dry_run)?;
            let exec_args: Vec<OsString> = vec![
                "exec".into(),
                tty_flag,
                "--workdir".into(),
                home_in_container.clone(),
                name.clone().into(),
                config.container.shell.clone().into(),
            ];
            let err = podbox::process::exec_replace("podman", &exec_args);
            return Err(err);
        }

        Command::Exec { args: cmd_args } => {
            let tty_flag = if nix::unistd::isatty(0).unwrap_or(false) {
                OsString::from("-it")
            } else {
                OsString::from("-i")
            };
            if cli.dry_run {
                let mut exec_args: Vec<OsString> = vec![
                    "exec".into(),
                    tty_flag.clone(),
                    name.clone().into(),
                ];
                for a in cmd_args {
                    exec_args.push(a.into());
                }
                println!("podman {}", args_to_string(&exec_args));
                return Ok(());
            }
            ensure_running(&name, cli.dry_run)?;
            let mut exec_args: Vec<OsString> = vec![
                "exec".into(),
                tty_flag.clone(),
                name.clone().into(),
            ];
            for a in cmd_args {
                exec_args.push(a.into());
            }
            let err = podbox::process::exec_replace("podman", &exec_args);
            return Err(err);
        }

        Command::Run { app, app_args } => {
            if cli.dry_run {
                let mut exec_args: Vec<OsString> = vec![
                    "exec".into(),
                    "-d".into(),
                    name.clone().into(),
                    app.clone().into(),
                ];
                for a in app_args {
                    exec_args.push(a.into());
                }
                println!("podman {}", args_to_string(&exec_args));
                return Ok(());
            }
            ensure_running(&name, cli.dry_run)?;
            let mut exec_args: Vec<OsString> = vec![
                "exec".into(),
                "-d".into(),
                name.clone().into(),
                app.clone().into(),
            ];
            for a in app_args {
                exec_args.push(a.into());
            }
            podbox::process::spawn_interactive("podman", &exec_args)?;
        }

        Command::Status => {
            if cli.dry_run {
                println!("podman inspect --format {{{{.State.Status}}}} {}", name);
                return Ok(());
            }
            let state = query_state(&name)?;
            let state_str = match state {
                ContainerState::Running => "running",
                ContainerState::Stopped => "stopped",
                ContainerState::Missing => "missing",
            };
            println!("{} [{}]", name, state_str);
        }

        Command::Logs { follow, tail } => {
            let mut args: Vec<OsString> = vec!["logs".into()];
            if *follow {
                args.push("-f".into());
            }
            if let Some(t) = tail {
                args.push("--tail".into());
                args.push(t.to_string().into());
            }
            args.push(name.clone().into());
            if cli.dry_run {
                println!("podman {}", args_to_string(&args));
                return Ok(());
            }
            podbox::process::spawn_interactive("podman", &args)?;
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
            if cli.dry_run {
                println!("podman stop {}", name);
                println!("podman rm {}", name);
                if *all {
                    let home = &config.container.home;
                    println!("rm -rf {}", home.display());
                }
                return Ok(());
            }

            if !force {
                print!("Remove container '{}'? [y/N] ", name);
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            // Stop first, then remove
            let stop_args: Vec<OsString> = vec!["stop".into(), name.clone().into()];
            let _ = podbox::process::run_piped("podman", &stop_args);

            let rm_args: Vec<OsString> = vec!["rm".into(), name.clone().into()];
            let output = podbox::process::run_piped("podman", &rm_args)?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(podbox::error::PodboxError::ContainerRemoveFailed(
                    name.clone(),
                    stderr.to_string(),
                )
                .into());
            }
            println!("Container '{}' removed.", name);

            if *all {
                let home = &config.container.home;
                if home.exists() {
                    if !force {
                        print!("Remove home directory '{}'? [y/N] ", home.display());
                        std::io::stdout().flush()?;
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        if !input.trim().eq_ignore_ascii_case("y") {
                            println!("Home directory kept.");
                            return Ok(());
                        }
                    }
                    std::fs::remove_dir_all(home)?;
                    println!("Home directory '{}' removed.", home.display());
                }
            }
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
                config_checksum: {
                    use sha2::Digest;
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(image_ref.as_bytes());
                    hex::encode(hasher.finalize())
                },
                image_digest: digest,
            };
            let lock_path = context_dir.join(".podbox.lock");
            podbox::lock::write(&lock_path, &lock)?;
            println!("Lock file written to {}", lock_path.display());
            return Ok(());
        }

        Command::Doctor { fix } => {
            run_doctor(&config, &env, *fix)?;
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

fn ensure_running(name: &str, dry_run: bool) -> Result<()> {
    match query_state(name)? {
        ContainerState::Running => Ok(()),
        ContainerState::Stopped | ContainerState::Missing => {
            if dry_run {
                println!("podman start {}", name);
                return Ok(());
            }
            let args: Vec<OsString> = vec!["start".into(), name.into()];
            podbox::process::spawn_interactive("podman", &args)?;
            match query_state(name)? {
                ContainerState::Running => Ok(()),
                state => Err(anyhow::anyhow!(
                    "Failed to start container '{}' (state: {:?})",
                    name,
                    state
                )),
            }
        }
    }
}

fn args_to_string(args: &[OsString]) -> String {
    args.iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ")
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
        let profile_content = if p.contains('/') || p.contains('\\') {
            let path = Path::new(p);
            std::fs::read_to_string(path)
                .with_context(|| format!("failed to read profile '{}'", path.display()))?
        } else {
            let found = podbox::profiles::find(p).ok_or_else(|| {
                let names = podbox::profiles::list_names();
                anyhow::anyhow!(
                    "Unknown profile '{}'. Available profiles: {}",
                    p,
                    names.join(", ")
                )
            })?;
            found.toml
        };
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

    // Non-prebuilt mode: create a custom config from a base image
    let base = image.unwrap_or("fedora:44");
    let base_name = base.split(':').next().unwrap_or(base)
        .split('/').last().unwrap_or(base);
    let container_name = name.unwrap_or(base_name).to_string();

    let mut cfg = Config::embedded();
    cfg.image.base = base.to_string();
    cfg.image.prebuilt = false;
    cfg.image.name = container_name.clone();
    cfg.container.name = container_name.clone();
    cfg.container.home = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("containers")
        .join(&container_name);

    podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
    let toml_str = toml::to_string_pretty(&cfg)?;
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
        "Container created! Run `podbox create {}` or `podbox start` to spin it up.",
        container_name
    );

    Ok(())
}

fn run_create(dry_run: bool, image: &str, name: Option<&str>, no_start: bool) -> Result<()> {
    // Determine if profile name or full image ref
    let is_profile = !image.contains('/') && !image.contains('\\');

    if is_profile && podbox::profiles::find(image).is_some() {
        // Treat as profile — run init pipeline
        let profile_content = if image.contains('/') || image.contains('\\') {
            let path = Path::new(image);
            std::fs::read_to_string(path)
                .with_context(|| format!("failed to read profile '{}'", path.display()))?
        } else {
            let found = podbox::profiles::find(image).ok_or_else(|| {
                let names = podbox::profiles::list_names();
                anyhow::anyhow!(
                    "Unknown profile '{}'. Available profiles: {}",
                    image,
                    names.join(", ")
                )
            })?;
            found.toml
        };

        let shell_info = podbox::init_wizard::detect_host_shell();
        if !shell_info.detected {
            eprintln!("Note: $SHELL not set or unrecognized, defaulting to fish.");
        }

        let mut cfg = Config::parse(&profile_content)?;
        podbox::init_wizard::apply_shell_defaults(&mut cfg, &shell_info);
        let container_name = name.unwrap_or(&cfg.container.name).to_string();
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

        // Build image
        if dry_run {
            println!("podbox build");
        } else {
            println!("Building image...");
            let local_tag = format!("localhost/podbox-{}:latest", cfg.image.name);
            if !podbox::podman::image_exists(&local_tag).unwrap_or(false) {
                let env = podbox::env::resolve()?;
                let xdg = podbox::xdg::resolve(&cfg.integration.xdg_dirs)?;
                podbox::build::run(&cfg, &env, &xdg, false, false)?;
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
            podbox::quadlet_install::install(&cfg, &env, &xdg, false)?;
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
            let args: Vec<OsString> = vec!["start".into(), container_name.clone().into()];
            podbox::process::spawn_interactive("podman", &args)?;
            println!("Container '{}' is running!", container_name);
            println!("Run `podbox shell` to enter.");
        }

        return Ok(());
    }

    if is_profile {
        eprintln!(
            "Note: '{}' is not a known profile (available: {}). Treating it as an image reference to pull.",
            image,
            podbox::profiles::list_names().join(", ")
        );
    }

    // If not a profile, treat as full image reference
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

    println!(
        "Image '{}' pulled. Create a config with `podbox init --profile <name>` to use it.",
        image
    );
    Ok(())
}

fn run_doctor(config: &Config, env: &HostEnv, fix: bool) -> Result<()> {
    let mut checks = 0;
    let mut passes = 0;
    let mut failures = 0;

    // 1. podman installed & version
    checks += 1;
    match podbox::podman::podman_version() {
        Ok(ver) if ver.at_least(5, 6) => {
            println!(
                "[PASS] podman {}.{}.{} (>= 5.6)",
                ver.major, ver.minor, ver.patch
            );
            passes += 1;
        }
        Ok(ver) if ver.at_least(5, 5) => {
            println!(
                "[WARN] podman {}.{}.{} (< 5.6) — upgrade to 5.6+ for Environment passthrough and native Quadlet management",
                ver.major, ver.minor, ver.patch
            );
            passes += 1;
        }
        Ok(ver) => {
            println!(
                "[FAIL] podman {}.{}.{} (< 5.5) — minimum supported version is 5.5",
                ver.major, ver.minor, ver.patch
            );
            failures += 1;
        }
        Err(_) => {
            println!("[FAIL] podman not found in PATH");
            failures += 1;
        }
    }

    // 2. Wayland socket
    if config.integration.wayland {
        checks += 1;
        if let Some(ref socket) = env.wayland_socket {
            println!("[PASS] Wayland socket found");
            passes += 1;

            // Check ownership — must match env.uid to work through idmapped mounts
            checks += 1;
            match socket.metadata() {
                Ok(meta) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        let owner = meta.uid();
                        if owner != env.uid {
                            println!(
                                "[WARN] Wayland socket owner {} != host UID {}",
                                owner, env.uid
                            );
                            if fix {
                                match fix_wayland_socket_ownership(socket) {
                                    Ok(()) => {
                                        println!("       -> Ownership fixed");
                                        passes += 1;
                                    }
                                    Err(e) => {
                                        println!("       -> Fix failed: {e}");
                                        failures += 1;
                                    }
                                }
                            } else {
                                println!(
                                    "       Run with --fix to repair, or: podman unshare chown 0:0 {}",
                                    socket.display()
                                );
                            }
                        } else {
                            println!("[PASS] Wayland socket owner correct");
                            passes += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("[WARN] Could not stat Wayland socket: {e}");
                }
            }
        } else {
            println!("[WARN] Wayland socket not found (WAYLAND_DISPLAY may not be set)");
        }
    }

    // 3. xdg-user-dirs
    checks += 1;
    match which::which("xdg-user-dir") {
        Ok(_) => {
            println!("[PASS] xdg-user-dir found");
            passes += 1;
        }
        Err(_) => {
            println!("[WARN] xdg-user-dir not found -- install xdg-user-dirs")
        }
    }

    // 4. podbox-guest binary
    checks += 1;
    let guest_paths = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("podbox-guest"))),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("podmgr-guest"))),
        which::which("podbox-guest").ok(),
        which::which("podmgr-guest").ok(),
        std::env::var("PODBOX_GUEST_BIN").map(PathBuf::from).ok(),
        std::env::var("PODMGR_GUEST_BIN").map(PathBuf::from).ok(), // compat
    ];
    let found = guest_paths
        .iter()
        .any(|p| p.as_ref().map(|p| p.exists()).unwrap_or(false));
    if found {
        println!("[PASS] podbox-guest binary found");
        passes += 1;
    } else {
        println!("[FAIL] podbox-guest binary not found");
        println!("       Prebuilt images bundle it: podbox pull <name>");
        println!("       Build: cargo build -p podbox-guest --release --target x86_64-unknown-linux-musl");
        failures += 1;
    }

    // 5. linger
    if config.lifecycle.autostart {
        checks += 1;
        if which::which("loginctl").is_ok() {
            let username = std::env::var("USER").unwrap_or_default();
            if !username.is_empty() {
                if let Ok(output) = podbox::process::run_piped(
                    "loginctl",
                    &[
                        "show-user".into(),
                        username.into(),
                        "--property=Linger".into(),
                    ],
                ) {
                    let out = String::from_utf8_lossy(&output.stdout);
                    if out.contains("yes") {
                        println!("[PASS] loginctl linger enabled");
                        passes += 1;
                    } else {
                        println!(
                            "[WARN] loginctl linger not enabled -- run: loginctl enable-linger $USER"
                        );
                    }
                }
            }
        }
    }

    // 6. Quadlet files
    if config.lifecycle.quadlet {
        checks += 1;
        let qdir = dirs::config_dir()
            .unwrap_or_else(|| podbox::config::expand_tilde("~/.config"))
            .join("containers/systemd");
        let container_file = qdir.join(format!("{}.container", config.container.name));
        if container_file.exists() {
            println!("[PASS] Quadlet files installed");
            passes += 1;
        } else {
            println!("[WARN] Quadlet files not found -- run: podbox enable");
        }
    }

    println!("\n{} / {} checks passed", passes, checks);
    if failures > 0 {
        Err(anyhow::anyhow!("{} check(s) failed", failures))
    } else {
        Ok(())
    }
}

fn fix_wayland_socket_ownership(socket: &std::path::Path) -> Result<()> {
    let runtime_dir = socket
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine runtime directory from socket path"))?;

    let output = std::process::Command::new("podman")
        .args(["unshare", "chown", "0:0"])
        .arg(socket)
        .arg(runtime_dir)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run podman unshare: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman unshare chown failed: {stderr}");
    }
    Ok(())
}
