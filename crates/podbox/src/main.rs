use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use podbox::cli::{Cli, Command};
use podbox::config::{self, Config};
use podbox::error::PodboxError;

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
            return commands::config::run_find_definition();
        }

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
            no_start,
        } => {
            return commands::config::run_create(cli.dry_run, image, name.as_deref(), *no_start);
        }

        Command::List => {
            return commands::config::run_list();
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
        let config_list = config::list_configs();
        if config_list.len() > 1 && nix::unistd::isatty(0).unwrap_or(false) {
            let items: Vec<String> = config_list
                .iter()
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                .collect();
            let selection = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("Multiple containers found")
                .items(&items)
                .default(0)
                .interact()
                .map_err(|e| anyhow::anyhow!("selection failed: {}", e))?;
            Config::load(&config_list[selection])?
        } else {
            match config::find_definition() {
                Some(path) => Config::load(&path)?,
                None => {
                    eprintln!("No definition file found, using embedded default. Create .podbox.toml to customize.");
                    Config::embedded()
                }
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
        Command::Build { name: _, rebuild, no_diff } => {
            commands::lifecycle::run_build(&config, &env, &xdg, cli.dry_run, *rebuild, *no_diff)?;
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

        Command::Diff { apply } => {
            commands::diff::run_diff(&config, &name, &env.username, *apply)?;
        }

        Command::Export { export_cmd } => {
            commands::config::run_export(&name, export_cmd)?;
        }

        Command::Remove { all, force } => {
            commands::lifecycle::run_remove(&config, &name, cli.dry_run, *all, *force)?;
        }

        Command::Serve { name: serve_name } => {
            commands::config::run_serve(cli.config.as_ref(), serve_name, cli.dry_run)?;
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

        Command::FindDefinition
        | Command::Completions { .. }
        | Command::Init { .. }
        | Command::Create { .. }
        | Command::List => unreachable!(),
    }

    Ok(())
}
