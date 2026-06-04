use std::io;
use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum PodboxError {
    #[error("container '{0}' not found -- run `podbox build` and `podbox enable` first")]
    ContainerMissing(String),

    #[error("definition file not found at {0}")]
    DefinitionNotFound(PathBuf),

    #[error("failed to read definition file: {0}")]
    DefinitionReadFailed(#[from] io::Error),

    #[error("failed to parse definition file: {0}")]
    DefinitionParseFailed(#[from] toml::de::Error),

    #[error("podman not found in PATH")]
    PodmanNotFound,

    #[error("podbox-guest binary not found -- use prebuilt images (podbox pull) or build manually: cargo build -p podbox-guest --release --target x86_64-unknown-linux-musl")]
    GuestBinaryNotFound,

    #[error("home directory '{0}' could not be created: {1}")]
    HomeCreateFailed(PathBuf, io::Error),

    #[error("container '{0}' remove failed: {1}")]
    ContainerRemoveFailed(String, String),

    #[error("wayland socket not found at {0}")]
    WaylandSocketNotFound(PathBuf),

    #[error("lock file error: {0}")]
    LockFileError(String),

    #[error("podman inspect failed for '{name}': {stderr}")]
    PodmanInspectFailed { name: String, stderr: String },

    #[error("build failed: {0}")]
    BuildFailed(String),

    #[error("quadlet install failed: {0}")]
    QuadletInstallFailed(String),

    #[error("export failed: {0}")]
    ExportFailed(String),

    #[error("xdg-user-dir not found in PATH -- install xdg-user-dirs")]
    XdgUserDirNotFound,

    #[error("failed to pull image '{0}'")]
    PullFailed(String),

    #[error("failed to tag image as '{0}'")]
    TagFailed(String),

    #[error("protocol version mismatch: host speaks v{expected}, guest speaks v{got}")]
    ProtocolMismatch { expected: u32, got: u32 },
}
