use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "podbox")]
#[command(version = env!("PODBOX_VERSION"))]
#[command(about = "Podman-native container environment manager")]
pub struct Cli {
    /// Path to the definition TOML file.
    #[arg(long, short)]
    pub config: Option<PathBuf>,

    /// Print what would happen without executing.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Container name to use for commands (overrides config file detection)
    #[arg(long, short = 'C', global = true)]
    pub container: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Build the container image from the definition.
    Build {
        /// Container name to build (overrides auto-detection).
        name: Option<String>,
        /// Force rebuild even if definition hasn't changed.
        #[arg(long)]
        rebuild: bool,
    },

    /// Install Quadlet systemd files and enable the container.
    Enable,

    /// Disable and remove Quadlet systemd files.
    Disable,

    /// Start the container.
    Start,

    /// Stop the container.
    Stop,

    /// Open an interactive shell in the container.
    Shell,

    /// Execute a command interactively in the container.
    Exec {
        /// Command and arguments to execute.
        #[arg(required = true, trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Run a GUI application in the container (detached).
    Run {
        /// Application to run.
        app: String,
        /// Additional arguments for the application.
        #[arg(trailing_var_arg = true)]
        app_args: Vec<String>,
    },

    /// Show container status.
    Status,

    /// Show container logs.
    Logs {
        /// Follow log output.
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show from the end.
        #[arg(short, long)]
        tail: Option<u32>,
    },

    /// Export a .desktop app or binary shim to the host.
    Export {
        #[command(subcommand)]
        export_cmd: ExportCommand,
    },

    /// Remove the container.
    Remove {
        /// Also remove the home directory.
        #[arg(long)]
        all: bool,
        /// Skip confirmation prompt.
        #[arg(long)]
        force: bool,
    },

    /// Run the host socket server (socket-activated by systemd).
    Serve {
        /// Container name to serve.
        name: String,
    },

    /// Enter a container by name (shortcut for --container <name> shell).
    Enter {
        /// Container name to enter.
        name: String,
    },

    /// Create and start a container from a profile or image in one step.
    Create {
        /// Profile name (fedora, cachy) or full image reference.
        image: String,
        /// Override the container name.
        #[arg(long, short)]
        name: Option<String>,
        /// Skip starting the container after setup.
        #[arg(long)]
        no_start: bool,
    },

    /// List all managed containers.
    List,

    /// Initialize a new container config.
    Init {
        /// Base image reference (e.g. "fedora:44") for a non-prebuilt container.
        /// If omitted, defaults to "fedora:44".
        image: Option<String>,
        /// Container name (defaults to the image name).
        #[arg(long)]
        name: Option<String>,
        /// Run an interactive wizard to build the config.
        #[arg(long, short = 'i', conflicts_with = "profile")]
        interactive: bool,
        /// Use a named profile (cachy, fedora, gaming) as template.
        #[arg(long)]
        profile: Option<String>,
    },

    /// Pull a prebuilt image without building.
    Pull {
        /// Distro shorthand or full image reference.
        image: Option<String>,
    },

    /// Run diagnostic checks.
    Doctor {
        /// Auto-fix common issues (e.g. corrupted Wayland socket ownership).
        #[arg(long)]
        fix: bool,
    },

    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },

    /// Find the definition file that would be used.
    FindDefinition,

    /// Translate a path between host and container.
    #[command(group(
        clap::ArgGroup::new("direction")
            .args(["to_container", "to_host"])
            .required(true)
            .multiple(false)
    ))]
    TranslatePath {
        /// Direction of translation.
        #[arg(long)]
        to_container: bool,
        /// Direction of translation.
        #[arg(long)]
        to_host: bool,
        /// Path to translate.
        path: String,
    },
}

#[derive(Subcommand)]
pub enum ExportCommand {
    /// Export a .desktop application.
    App { name: String },
    /// Export a binary shim.
    Bin { name: String },
    /// Remove all exports for the container.
    Clean,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl From<Shell> for clap_complete::shells::Shell {
    fn from(s: Shell) -> Self {
        match s {
            Shell::Bash => clap_complete::shells::Shell::Bash,
            Shell::Zsh => clap_complete::shells::Shell::Zsh,
            Shell::Fish => clap_complete::shells::Shell::Fish,
        }
    }
}
