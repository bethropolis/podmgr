use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;

use crate::config::XdgDirConfig;

pub struct ResolvedXdgDirs {
    pub documents: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub pictures: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub videos: Option<PathBuf>,
    pub desktop: Option<PathBuf>,
    pub projects: Option<PathBuf>,
}

/// Resolve XDG user directories from the host.
///
/// For each enabled dir, tries in order:
/// 1. Parse `~/.config/user-dirs.dirs`
/// 2. Call `xdg-user-dir <NAME>`
/// 3. Fall back to `~/DirName`
/// 4. If path doesn't exist on disk: `None`
pub fn resolve(config: &XdgDirConfig) -> Result<ResolvedXdgDirs> {
    Ok(ResolvedXdgDirs {
        documents: resolve_dir(config.documents, "DOCUMENTS", "Documents"),
        downloads: resolve_dir(config.downloads, "DOWNLOADS", "Downloads"),
        pictures: resolve_dir(config.pictures, "PICTURES", "Pictures"),
        music: resolve_dir(config.music, "MUSIC", "Music"),
        videos: resolve_dir(config.videos, "VIDEOS", "Videos"),
        desktop: resolve_dir(config.desktop, "DESKTOP", "Desktop"),
        projects: resolve_dir(config.projects, "PROJECTS", "Projects"),
    })
}

fn resolve_dir(enabled: bool, xdg_name: &str, fallback_name: &str) -> Option<PathBuf> {
    if !enabled {
        return None;
    }

    // Try xdg-user-dir
    if let Ok(output) = Command::new("xdg-user-dir").arg(xdg_name).output() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() && path != home_dir_str() {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    // Fallback to ~/DirName
    if let Some(home) = dirs::home_dir() {
        let p = home.join(fallback_name);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

fn home_dir_str() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}
