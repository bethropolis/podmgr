use anyhow::Result;

use podbox::config;

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
    let mut cmd = <podbox::cli::Cli as clap::CommandFactory>::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
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
            .map_err(|_| podbox::error::PodboxError::PodmanNotFound)?;
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
            .map_err(|_| podbox::error::PodboxError::PodmanNotFound)?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}
