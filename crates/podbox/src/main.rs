use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use podbox::cli::{Cli, Command};
use podbox::config::{self, Config};
use podbox::error::PodboxError;

mod commands;

pub const VERSION: &str = env!("PODBOX_VERSION");

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
        | Command::Shell { name }
        | Command::Enter { name }
        | Command::Status { name }
        | Command::Remove { name, .. }
        | Command::Logs { name, .. }
        | Command::Update { name, .. }
        | Command::Diff { name, .. }
        | Command::Snapshot { name, .. }
        | Command::Restore { name, .. }
        | Command::Inspect { name, .. }
        | Command::FindDefinition { name } => name.clone(),
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
            return commands::config::run_completions((*shell).into());
        }

        Command::Init {
            image,
            name,
            interactive,
            profile,
        } => {
            return commands::config::run_init(
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
        } => {
            return commands::config::run_create(
                cli.dry_run,
                image,
                name.as_deref(),
                packages.as_deref(),
                *no_start,
            );
        }

        Command::List => {
            return commands::config::run_list();
        }

        Command::Clone {
            src,
            dst,
            copy_home,
        } => {
            return commands::config::run_clone(src, dst, *copy_home, cli.dry_run);
        }

        Command::Use { name, clear } => {
            return commands::config::run_use(name.clone(), *clear, cli.dry_run);
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
        return commands::config::run_find_definition(lookup.as_deref());
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
            && nix::unistd::isatty(0).unwrap_or(false)
        {
            eprintln!("Welcome to podbox! It looks like you don't have any containers set up yet.");
            let launch =
                dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Would you like to run the interactive setup wizard?")
                    .default(true)
                    .interact()
                    .unwrap_or(false);
            if launch {
                commands::config::run_init(cli.dry_run, None, None, true, None)?;
                return Ok(());
            }
        }

        if config_list.len() > 1 && nix::unistd::isatty(0).unwrap_or(false) {
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

    // Apply image label defaults — only for commands that generate config from labels
    if needs_image_labels(&cli.command) {
        let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
        if let Ok(true) = podbox::podman::image_exists(&local_tag) {
            if let Ok(labels) = podbox::labels::fetch(&local_tag) {
                podbox::labels::apply_defaults(&mut config, &labels);
            }
        }
    }

    let env = podbox::env::resolve()?;
    let xdg = podbox::xdg::resolve(&config.integration.xdg_dirs)?;

    match &cli.command {
        Command::Build {
            name: _,
            rebuild,
            no_diff,
        } => {
            commands::lifecycle::run_build(&config, &env, &xdg, cli.dry_run, *rebuild, *no_diff)?;
        }

        Command::Enable { name: _ } => {
            commands::lifecycle::run_enable(&config, &env, &xdg, cli.dry_run)?;
        }

        Command::Disable { name: _, .. } => {
            commands::lifecycle::run_disable(&name)?;
        }

        Command::Start { name: _, timeout } => {
            commands::lifecycle::run_start(&config, &env, &xdg, &name, cli.dry_run, *timeout)?;
        }

        Command::Stop { name: _ } => {
            commands::lifecycle::run_stop(&config, &name, cli.dry_run)?;
        }

        Command::Shell { name: _ } | Command::Enter { name: _ } => {
            commands::runtime::run_shell_enter(&config, &name, cli.dry_run)?;
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
            commands::config::run_inspect(
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
            commands::config::run_export(&name, Some(&config), export_cmd)?;
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
            commands::config::run_serve(cli.config.as_ref(), serve_name, cli.dry_run)?;
        }

        Command::Update { no_restart, .. } => {
            commands::lifecycle::run_update(&config, &env, &xdg, &name, cli.dry_run, *no_restart)?;
        }

        Command::Pull { image } => {
            commands::config::run_pull(&config, image, cli.dry_run)?;
        }

        Command::Doctor { fix } => {
            commands::runtime::run_doctor(&config, &env, *fix)?;
        }

        Command::TranslatePath {
            to_container,
            to_host,
            path,
        } => {
            commands::config::run_translate_path(&config, &xdg, *to_container, *to_host, path)?;
        }

        Command::FindDefinition { .. }
        | Command::Completions { .. }
        | Command::Init { .. }
        | Command::Create { .. }
        | Command::Clone { .. }
        | Command::List
        | Command::Use { .. } => unreachable!(),
    }

    Ok(())
}
