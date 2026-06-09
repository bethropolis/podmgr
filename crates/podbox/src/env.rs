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
    pub host_has_localtime: bool,
    pub host_has_timezone_file: bool,
    pub host_has_local_share_themes: bool,
    pub host_has_local_share_icons: bool,
    pub host_has_local_share_fonts: bool,
    pub host_shell: Option<String>,
    pub host_locale: Option<String>,
    pub gpg_agent_socket: Option<PathBuf>,
    pub gpg_home: Option<PathBuf>,
}

/// Resolve the host environment.
///
/// Reads `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` from the environment.
/// Detects presence of Wayland, PipeWire, PulseAudio, and D-Bus sockets.
/// Detects timezone files and modern XDG theme/icon/font directories.
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

    let pipewire_socket = Some(xdg_runtime_dir.join("pipewire-0")).filter(|p| p.exists());

    let pulse_dir = Some(xdg_runtime_dir.join("pulse")).filter(|p| p.join("native").exists());

    let dbus_socket = Some(xdg_runtime_dir.join("bus")).filter(|p| p.exists());

    let gpu_has_dri = Path::new("/dev/dri").exists();
    let gpu_has_nvidia = Path::new("/dev/nvidiactl").exists();
    let gpu_has_nvidia_uvm = Path::new("/dev/nvidia-uvm").exists();

    let host_has_localtime = Path::new("/etc/localtime").exists();
    let host_has_timezone_file = Path::new("/etc/timezone").exists();

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
    let host_has_local_share_themes = home.join(".local/share/themes").exists();
    let host_has_local_share_icons = home.join(".local/share/icons").exists();
    let host_has_local_share_fonts = home.join(".local/share/fonts").exists();

    let host_shell = env::var("SHELL").ok().filter(|s| !s.is_empty());
    let host_locale = env::var("LANG")
        .ok()
        .or_else(|| env::var("LC_ALL").ok())
        .or_else(|| env::var("LC_CTYPE").ok())
        .filter(|s| !s.is_empty());

    let gpg_home = env::var("GNUPGHOME").ok().map(PathBuf::from).or_else(|| {
        let fallback = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root")).join(".gnupg");
        if fallback.exists() { Some(fallback) } else { None }
    });
    let gpg_agent_socket = gpg_home.as_ref().and_then(|gpg| {
        let sock = gpg.join("S.gpg-agent");
        if sock.exists() { Some(sock) } else { None }
    });

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
        host_has_localtime,
        host_has_timezone_file,
        host_has_local_share_themes,
        host_has_local_share_icons,
        host_has_local_share_fonts,
        host_shell,
        host_locale,
        gpg_agent_socket,
        gpg_home,
    })
}
