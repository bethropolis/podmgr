use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use podbox::cli::{Cli, Command};
use podbox::config::{self, Config};
use podbox::editor;
use podbox::error::PodboxError;

mod commands;

/// Commands that need image label defaults applied to the config.
/// These generate Quadlet files or build the image — the rest can skip
/// the ~100ms `podman inspect` fork.
fn needs_image_labels(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Build { .. } | Command::Enable { .. } | Command::Update { .. }
    )
}

fn extract_positional_name(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Build { name, .. }
        | Command::Enable { name }
        | Command::Disable { name, .. }
        | Command::Start { name, .. }
        | Command::Stop { name }
        | Command::Shell { name, edit: _ }
        | Command::Enter { name }
        | Command::Status { name }
        | Command::Remove { name, .. }
        | Command::Logs { name, .. }
        | Command::Update { name, .. }
        | Command::Diff { name, .. }
        | Command::Snapshot { name, .. }
        | Command::Restore { name, .. }
        | Command::Inspect { name, .. }
        | Command::FindDefinition { name }
        | Command::Edit { name, .. } => name.clone(),
        _ => None,
    }
}

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
            PodboxError::PodmanNotFound => ExitCode::from(5),
            PodboxError::PullFailed(..) | PodboxError::TagFailed(..) => ExitCode::from(6),
            _ => ExitCode::FAILURE,
        }
    } else {
        ExitCode::FAILURE
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Exit early if podman is not installed — clean error instead of a cryptic
    // spawn failure deep in the stack.
    if !matches!(&cli.command, Command::Completions { .. }) && which::which("podman").is_err() {
        return Err(PodboxError::PodmanNotFound.into());
    }

    match &cli.command {
        Command::Completions { shell } => {
            return commands::definition::run_completions((*shell).into());
        }

        Command::Init {
            image,
            name,
            interactive,
            profile,
        } => {
            return commands::create::run_init(
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
            packages,
            no_start,
            edit,
        } => {
            return commands::create::run_create(
                cli.dry_run,
                image,
                name.as_deref(),
                packages.as_deref(),
                *no_start,
                *edit,
            );
        }

        Command::List => {
            return commands::definition::run_list();
        }

        Command::Clone {
            src,
            dst,
            copy_home,
        } => {
            return commands::clone::run_clone(src, dst, *copy_home, cli.dry_run);
        }

        Command::Use { name, clear } => {
            return commands::context::run_use(name.clone(), *clear, cli.dry_run);
        }

        Command::Edit { name, rebuild } => {
            let container_name = name
                .clone()
                .or_else(|| cli.container.clone())
                .or_else(|| std::env::var("PODBOX_CONTAINER").ok())
                .or_else(config::read_active_context);
            return run_edit(cli.dry_run, container_name.as_deref(), *rebuild);
        }

        _ => {}
    }

    // Resolution chain: positional -> -C -> PODBOX_CONTAINER env -> .active
    let cmd_name = extract_positional_name(&cli.command);
    let target_name = cmd_name
        .or_else(|| cli.container.clone())
        .or_else(|| std::env::var("PODBOX_CONTAINER").ok())
        .or_else(config::read_active_context);

    // Short-circuit for commands that don't need a full config load
    if let Command::FindDefinition { name } = &cli.command {
        let lookup = name.clone().or_else(|| target_name.clone());
        return commands::definition::run_find_definition(lookup.as_deref());
    }

    if let Command::Disable { force: true, .. } = &cli.command {
        let n = target_name.clone()
            .context("--force requires a container name (positional, -C, PODBOX_CONTAINER env, or active context)")?;
        return commands::lifecycle::run_disable(&n);
    }

    if let Command::Remove {
        stale: true, force, ..
    } = &cli.command
    {
        return commands::lifecycle::run_remove_stale(cli.dry_run, *force);
    }

    let (config, name) = resolve_config(&cli, target_name)?;

    let env = podbox::env::resolve()?;
    let xdg = podbox::xdg::resolve(&config.integration.xdg_dirs)?;

    match &cli.command {
        Command::Build {
            name: _,
            rebuild,
            no_diff,
            edit,
        } => {
            if *edit {
                let config_path = resolve_config_path(cli.container.as_deref())?;
                let ed = editor::resolve()?;
                editor::open(&ed, &config_path)?;
            }
            commands::lifecycle::run_build(&config, &env, &xdg, cli.dry_run, *rebuild, *no_diff)?;
        }

        Command::Enable { name: _ } => {
            commands::lifecycle::run_enable(&config, &env, &xdg, cli.dry_run)?;
        }

        Command::Disable { name: _, .. } => {
            commands::lifecycle::run_disable(&name)?;
        }

        Command::Start {
            name: _,
            timeout,
            edit,
        } => {
            if *edit {
                let config_path = resolve_config_path(cli.container.as_deref())?;
                let ed = editor::resolve()?;
                editor::open(&ed, &config_path)?;
            }
            commands::lifecycle::run_start(&config, &env, &xdg, &name, cli.dry_run, *timeout)?;
        }

        Command::Stop { name: _ } => {
            commands::lifecycle::run_stop(&config, &name, cli.dry_run)?;
        }

        Command::Shell { name: _, edit } => {
            if *edit {
                let config_path = resolve_config_path(cli.container.as_deref())?;
                let ed = editor::resolve()?;
                editor::open(&ed, &config_path)?;
            }
            commands::runtime::run_shell_enter(&env, &config, &name, cli.dry_run)?;
        }

        Command::Enter { name: _ } => {
            commands::runtime::run_shell_enter(&env, &config, &name, cli.dry_run)?;
        }

        Command::Exec {
            args: cmd_args,
            root,
        } => {
            commands::runtime::run_exec(&env, &name, cmd_args, cli.dry_run, *root)?;
        }

        Command::Run { app, app_args } => {
            commands::runtime::run_run(&env, &name, app, app_args, cli.dry_run)?;
        }

        Command::Status { name: _ } => {
            commands::runtime::run_status(&name, cli.dry_run)?;
        }

        Command::Logs {
            name: _,
            follow,
            tail,
            since,
        } => {
            commands::runtime::run_logs(&name, *follow, *tail, since.clone(), cli.dry_run)?;
        }

        Command::Diff { apply, .. } => {
            commands::diff::run_diff(&config, &name, &env.username, *apply)?;
        }

        Command::Snapshot { tag, .. } => {
            commands::lifecycle::run_snapshot(&config, &name, tag.as_deref())?;
        }

        Command::Restore { tag, .. } => {
            commands::lifecycle::run_restore(&config, &name, tag)?;
        }

        Command::Inspect {
            config: show_config,
            quadlet: show_quadlet,
            env: show_env,
            ..
        } => {
            commands::inspect::run_inspect(
                &config,
                &name,
                &env,
                &xdg,
                *show_config,
                *show_quadlet,
                *show_env,
            )?;
        }

        Command::Export { export_cmd } => {
            commands::export::run_export(&name, Some(&config), export_cmd)?;
        }

        Command::Remove {
            name: _,
            all,
            force,
            ..
        } => {
            commands::lifecycle::run_remove(&config, &name, cli.dry_run, *all, *force)?;
        }

        Command::Serve { name: serve_name } => {
            commands::serve::run_serve(cli.config.as_ref(), serve_name, cli.dry_run)?;
        }

        Command::Update { no_restart, .. } => {
            commands::lifecycle::run_update(&config, &env, &xdg, &name, cli.dry_run, *no_restart)?;
        }

        Command::Pull { image } => {
            commands::pull::run_pull(&config, image, cli.dry_run)?;
        }

        Command::Doctor { fix } => {
            commands::runtime::run_doctor(&config, &env, *fix)?;
        }

        Command::TranslatePath {
            to_container,
            to_host,
            path,
        } => {
            commands::translate::run_translate_path(&config, &xdg, *to_container, *to_host, path)?;
        }

        Command::FindDefinition { .. }
        | Command::Completions { .. }
        | Command::Init { .. }
        | Command::Create { .. }
        | Command::Clone { .. }
        | Command::List
        | Command::Use { .. }
        | Command::Edit { .. } => unreachable!(),
    }

    Ok(())
}

fn resolve_config(cli: &Cli, target_name: Option<String>) -> Result<(Config, String)> {
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
    } else if let Some(ref container_name) = target_name {
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
        let config_list = config::list_configs();

        // No configs at all — welcome the user and offer the wizard
        if config_list.is_empty()
            && config::find_definition().is_none()
            && podbox::codegen::distros::is_tty()
        {
            eprintln!("Welcome to podbox! It looks like you don't have any containers set up yet.");
            let launch =
                dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Would you like to run the interactive setup wizard?")
                    .default(true)
                    .interact()
                    .unwrap_or(false);
            if launch {
                commands::create::run_init(cli.dry_run, None, None, true, None)?;
                return Ok((Config::embedded(), String::new()));
            }
        }

        if config_list.len() > 1 && podbox::codegen::distros::is_tty() {
            let items: Vec<String> = config_list
                .iter()
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                .collect();
            let selection =
                dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Multiple containers found")
                    .items(&items)
                    .default(0)
                    .interact()
                    .map_err(|e| anyhow::anyhow!("selection failed: {}", e))?;
            Config::load(&config_list[selection])?
        } else if config_list.len() == 1 {
            Config::load(&config_list[0])?
        } else {
            match config::find_definition() {
                Some(path) => Config::load(&path)?,
                None => {
                    anyhow::bail!(
                        "No container configs found. Create one with `podbox init --interactive` \
                         or specify a config with `--config <PATH>` / `-C <NAME>`."
                    );
                }
            }
        }
    };

    let name = config.container.name.clone();

    if needs_image_labels(&cli.command) {
        let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
        if let Ok(true) = podbox::podman::image_exists(&local_tag) {
            if let Ok(labels) = podbox::labels::fetch(&local_tag) {
                podbox::labels::apply_defaults(&mut config, &labels);
            }
        }
    }

    Ok((config, name))
}

/// Resolve the config file path for the given container name (or auto-detect).
fn resolve_config_path(container: Option<&str>) -> Result<PathBuf> {
    if let Some(name) = container {
        let path = config::config_dir().join(format!("{}.toml", name));
        if !path.exists() {
            anyhow::bail!(
                "no config found for container '{}' at '{}'",
                name,
                path.display()
            );
        }
        return Ok(path);
    }

    let configs = config::list_configs();
    match configs.len() {
        0 => {
            let local = config::find_definition();
            match local {
                Some(p) => Ok(p),
                None => anyhow::bail!("no config found. Run `podbox init` to create one."),
            }
        }
        1 => Ok(configs.into_iter().next().unwrap()),
        _ => {
            if podbox::codegen::distros::is_tty() {
                let items: Vec<String> = configs
                    .iter()
                    .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                    .collect();
                let idx =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Select container")
                        .items(&items)
                        .default(0)
                        .interact()?;
                Ok(configs[idx].clone())
            } else {
                anyhow::bail!("multiple configs found — specify one with --container <name>")
            }
        }
    }
}

/// Hash the `[image]` section of a config file — used to detect changes.
fn hash_image_section(path: &std::path::Path) -> Result<String> {
    let raw = std::fs::read_to_string(path)?;
    let table: toml::Value = raw.parse()?;
    let image_str = table
        .get("image")
        .map(|v| v.to_string())
        .unwrap_or_default();
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(image_str.as_bytes())))
}

/// Open the config in the user's editor, detect [image] changes, and offer to rebuild.
fn run_edit(dry_run: bool, container: Option<&str>, rebuild_after: bool) -> Result<()> {
    let config_path = resolve_config_path(container)?;

    if dry_run {
        println!("Would open: {}", config_path.display());
        return Ok(());
    }

    let pre_hash = hash_image_section(&config_path)?;

    let ed = editor::resolve()?;
    editor::open(&ed, &config_path)?;

    let post_hash = hash_image_section(&config_path)?;
    let image_changed = pre_hash != post_hash;

    if image_changed {
        println!("Image config changed.");
        if rebuild_after {
            let config = Config::load(&config_path)?;
            let env = podbox::env::resolve()?;
            let xdg = podbox::xdg::resolve(&config.integration.xdg_dirs)?;
            commands::lifecycle::run_build(&config, &env, &xdg, false, false, false)?;
        } else if podbox::codegen::distros::is_tty() {
            let yes = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Rebuild now?")
                .default(true)
                .interact()?;
            if yes {
                let config = Config::load(&config_path)?;
                let env = podbox::env::resolve()?;
                let xdg = podbox::xdg::resolve(&config.integration.xdg_dirs)?;
                commands::lifecycle::run_build(&config, &env, &xdg, false, false, false)?;
            }
        } else {
            eprintln!("Run `podbox build` to apply changes.");
        }
    }

    Ok(())
}
