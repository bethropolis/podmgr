//! `podbox` — Podman-native container environment manager.
//!
//! Turns a single TOML definition file into a fully integrated,
//! systemd-managed container environment with selective XDG directory
//! sharing, Wayland/audio passthrough, GPU acceleration, and desktop
//! integration (`.desktop` export, binary shims).

pub const VERSION: &str = env!("PODBOX_VERSION");

pub mod build;
pub mod cli;
pub mod codegen;
pub mod config;
pub mod diff;
pub mod editor;
pub mod env;
pub mod error;
pub mod export;
pub mod guest;
pub mod labels;
pub mod lock;
pub mod podman;
pub mod process;
pub mod profiles;
pub mod protocol;
pub mod quadlet_install;
pub mod socket_host;
pub mod wizard;
pub mod xdg;
