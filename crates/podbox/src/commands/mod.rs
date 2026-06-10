use anyhow::Result;
use std::time::{Duration, Instant};

use podbox::podman::{query_state, ContainerState};

pub mod clone;
pub mod context;
pub mod create;
pub mod definition;
pub mod diff;
pub mod export;
pub mod inspect;
pub mod lifecycle;
pub mod pull;
pub mod runtime;
pub mod serve;
pub mod translate;

pub const DEFAULT_START_TIMEOUT_SECS: u64 = 30;
const POLL_INTERVAL_MS: u64 = 300;

/// Start a container if it isn't already running.
///
/// Tries `systemctl --user start` when systemd is available and falls
/// back to `podman start`.  Used by `start`, `shell`, `exec`, and `run`.
///
/// `timeout_secs` controls how long to wait for the container to reach
/// `Running` state after dispatching the start command.
pub fn ensure_running(name: &str, dry_run: bool, timeout_secs: u64) -> Result<()> {
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

            let deadline = Instant::now() + Duration::from_secs(timeout_secs);
            loop {
                match query_state(name)? {
                    ContainerState::Running => return Ok(()),
                    _ if Instant::now() >= deadline => {
                        let state = query_state(name)?;
                        return Err(anyhow::anyhow!(
                            "Container '{}' did not become ready within {}s \
                             (final state: {:?})",
                            name,
                            timeout_secs,
                            state,
                        ));
                    }
                    _ => {
                        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
                    }
                }
            }
        }
    }
}
