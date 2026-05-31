use std::env;
use std::path::{Path, PathBuf};

use anyhow::Result;
use nix::unistd::getuid;

/// Resolved host environment for socket and path detection.
pub struct HostEnv {
    pub uid: u32,
    pub username: String,
    pub xdg_runtime_dir: PathBuf,
    pub wayland_display: Option<String>,
    pub wayland_socket: Option<PathBuf>,
    pub pipewire_socket: Option<PathBuf>,
    pub pulse_dir: Option<PathBuf>,
    pub dbus_socket: Option<PathBuf>,
    pub gpu_has_dri: bool,
    pub gpu_has_nvidia: bool,
    pub gpu_has_nvidia_uvm: bool,
}

/// Resolve the host environment.
///
/// Reads `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` from the environment.
/// Detects presence of Wayland, PipeWire, PulseAudio, and D-Bus sockets.
pub fn resolve() -> Result<HostEnv> {
    let uid = getuid().as_raw();
    let username = env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".into());

    let xdg_runtime_dir: PathBuf = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid)));

    let wayland_display = env::var("WAYLAND_DISPLAY").ok();
    let wayland_socket = wayland_display
        .as_ref()
        .map(|d| xdg_runtime_dir.join(d))
        .filter(|p| p.exists());

    let pipewire_socket = {
        let p = xdg_runtime_dir.join("pipewire-0");
        if p.exists() { Some(p) } else { None }
    };

    let pulse_dir = {
        let p = xdg_runtime_dir.join("pulse");
        if p.join("native").exists() { Some(p) } else { None }
    };

    let dbus_socket = {
        let p = xdg_runtime_dir.join("bus");
        if p.exists() { Some(p) } else { None }
    };

    let gpu_has_dri = Path::new("/dev/dri").exists();
    let gpu_has_nvidia = Path::new("/dev/nvidiactl").exists();
    let gpu_has_nvidia_uvm = Path::new("/dev/nvidia-uvm").exists();

    Ok(HostEnv {
        uid,
        username,
        xdg_runtime_dir,
        wayland_display,
        wayland_socket,
        pipewire_socket,
        pulse_dir,
        dbus_socket,
        gpu_has_dri,
        gpu_has_nvidia,
        gpu_has_nvidia_uvm,
    })
}
