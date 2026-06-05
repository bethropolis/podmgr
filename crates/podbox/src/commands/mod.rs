use anyhow::Result;

use podbox::podman::{query_state, ContainerState};

pub mod lifecycle;
pub mod runtime;

/// Start a container if it isn't already running.
///
/// Tries `systemctl --user start` when systemd is available and falls
/// back to `podman start`.  Used by `start`, `shell`, `exec`, and `run`.
pub fn ensure_running(name: &str, dry_run: bool) -> Result<()> {
    match query_state(name)? {
        ContainerState::Running => Ok(()),
        ContainerState::Stopped | ContainerState::Missing => {
            if dry_run {
                println!("podman start {}", name);
                return Ok(());
            }
            if which::which("systemctl").is_ok() {
                let args = podbox::process::args(&["--user", "start", name]);
                podbox::process::spawn_interactive("systemctl", &args)?;
            } else {
                let args = podbox::process::args(&["start", name]);
                podbox::process::spawn_interactive("podman", &args)?;
            }
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
