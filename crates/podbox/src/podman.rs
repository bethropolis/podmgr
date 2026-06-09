use std::process::Command;
use std::sync::OnceLock;

use crate::error::PodboxError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodmanVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl PodmanVersion {
    pub fn at_least(&self, major: u32, minor: u32) -> bool {
        (self.major, self.minor) >= (major, minor)
    }
}

static PODMAN_VERSION: OnceLock<anyhow::Result<PodmanVersion>> = OnceLock::new();

fn parse_version_string(s: &str) -> PodmanVersion {
    let s = s.trim();
    // Strip anything after a space (e.g. "5.3.0 (ok)" -> "5.3.0")
    let s = s.split_whitespace().next().unwrap_or(s);
    let mut parts = s.splitn(3, '.');
    PodmanVersion {
        major: parts.next().and_then(|p| p.parse().ok()).unwrap_or(0),
        minor: parts.next().and_then(|p| p.parse().ok()).unwrap_or(0),
        patch: parts.next().and_then(|p| p.parse().ok()).unwrap_or(0),
    }
}

pub fn podman_version() -> anyhow::Result<&'static PodmanVersion> {
    let res = PODMAN_VERSION.get_or_init(|| {
        // Prefer structured output from `podman version -f` (more reliable
        // across distro packaging, e.g. Debian epoch suffixes).
        let structured = Command::new("podman")
            .args(["version", "-f", "{{.Client.Version}}"])
            .output()
            .ok()
            .filter(|o| o.status.success());
        let version_str = match structured {
            Some(ref output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
            None => {
                // Fallback: parse `podman --version` (e.g. "podman version 5.3.0")
                let output = Command::new("podman").args(["--version"]).output()?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.split_whitespace().last().unwrap_or("").to_string()
            }
        };
        Ok(parse_version_string(&version_str))
    });
    res.as_ref().map_err(|e| anyhow::anyhow!("{}", e))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn set_test_version(ver: PodmanVersion) {
    PODMAN_VERSION.set(Ok(ver)).ok();
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ContainerState {
    Running,
    Stopped,
    Missing,
}

/// Check whether a local image tag exists.
pub fn image_exists(tag: &str) -> anyhow::Result<bool> {
    let output = std::process::Command::new("podman")
        .args(["image", "exists", tag])
        .output()?;
    Ok(output.status.success())
}

/// Fetch OCI labels for a local image.
pub fn image_labels(tag: &str) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let output = std::process::Command::new("podman")
        .args([
            "inspect",
            "--type",
            "image",
            "--format",
            "{{json .Labels}}",
            tag,
        ])
        .output()?;
    if !output.status.success() {
        return Ok(std::collections::HashMap::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let map: std::collections::HashMap<String, String> =
        serde_json::from_str(stdout.trim()).unwrap_or_default();
    Ok(map)
}

/// Query the state of a container.
///
/// Checks `podman inspect` first.  If the container is unknown to podman,
/// falls back to `systemctl --user is-active` (quadlet-managed containers)
/// and returns `Stopped` when the unit exists but is inactive.
pub fn query_state(name: &str) -> anyhow::Result<ContainerState> {
    let output = Command::new("podman")
        .args([
            "inspect",
            "--type",
            "container",
            "--format",
            "{{.State.Status}}",
            name,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no such container") || stderr.contains("no such object") {
            // Fallback: check systemd for Quadlet-managed containers
            if let Ok(sys) = Command::new("systemctl")
                .args(["--user", "is-active", &format!("{}.service", name)])
                .output()
            {
                let state = String::from_utf8_lossy(&sys.stdout).trim().to_string();
                match state.as_str() {
                    "active" => return Ok(ContainerState::Running),
                    "inactive" | "failed" => return Ok(ContainerState::Stopped),
                    _ => {
                        // "unknown" — unit not loaded.  Check whether quadlet
                        // files exist; if so the container was built but is
                        // stopped (OnStop=remove may have removed the podman
                        // object while leaving the quadlet infrastructure).
                        let qdir = dirs::config_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
                            .join("containers/systemd");
                        if qdir.join(format!("{}.container", name)).exists() {
                            return Ok(ContainerState::Stopped);
                        }
                        return Ok(ContainerState::Missing);
                    }
                };
            }
            // systemctl not available — check quadlet files as last resort
            let qdir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
                .join("containers/systemd");
            if qdir.join(format!("{}.container", name)).exists() {
                return Ok(ContainerState::Stopped);
            }
            return Ok(ContainerState::Missing);
        }
        return Err(PodboxError::PodmanInspectFailed {
            name: name.into(),
            stderr: stderr.to_string(),
        }
        .into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    match stdout.as_str() {
        "running" => Ok(ContainerState::Running),
        "stopped" | "exited" => Ok(ContainerState::Stopped),
        _ => Ok(ContainerState::Stopped),
    }
}

/// Get the digest of a built image.
pub fn image_digest(tag: &str) -> anyhow::Result<String> {
    let output = Command::new("podman")
        .args(["inspect", "--format", "{{.Digest}}", tag])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PodboxError::PodmanInspectFailed {
            name: tag.into(),
            stderr: stderr.to_string(),
        }
        .into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
