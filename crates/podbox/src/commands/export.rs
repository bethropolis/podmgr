use anyhow::Result;

use podbox::cli::ExportCommand;
use podbox::config::Config;

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
