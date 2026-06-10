use std::path::{Path, PathBuf};

use anyhow::Result;

use podbox::config::Config;
use podbox::xdg::ResolvedXdgDirs;

/// Convert a host path to a container path (or vice versa).
pub fn run_translate_path(
    _config: &Config,
    xdg: &ResolvedXdgDirs,
    to_container: bool,
    to_host: bool,
    path_str: &str,
) -> Result<()> {
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string());
    let home_in_container = if username == "root" {
        "/root".to_string()
    } else {
        format!("/home/{}", username)
    };
    let host_path = if Path::new(path_str).is_relative() {
        std::env::current_dir()
            .map(|d| d.join(path_str))
            .unwrap_or_else(|_| PathBuf::from(path_str))
    } else {
        PathBuf::from(path_str)
    };

    if to_container {
        let host_to_container: Vec<(&str, &PathBuf)> = vec![
            ("Documents", &xdg.documents),
            ("Downloads", &xdg.downloads),
            ("Pictures", &xdg.pictures),
            ("Music", &xdg.music),
            ("Videos", &xdg.videos),
            ("Desktop", &xdg.desktop),
            ("Projects", &xdg.projects),
        ]
        .into_iter()
        .filter_map(|(name, opt)| opt.as_ref().map(|r| (name, &r.path)))
        .collect();

        for (dir_name, host_dir) in &host_to_container {
            if let Ok(relative) = host_path.strip_prefix(host_dir) {
                let container_path =
                    format!("{}/{}/{}", home_in_container, dir_name, relative.display());
                println!("{container_path}");
                return Ok(());
            }
        }

        let host_home = dirs::home_dir().unwrap_or_default();
        if let Ok(relative) = host_path.strip_prefix(&host_home) {
            let container_path = format!("{}/{}", home_in_container, relative.display());
            println!("{container_path}");
            return Ok(());
        }

        println!("{path_str}");
    }

    if to_host {
        for (dir_name, host_dir) in [
            ("Documents", &xdg.documents),
            ("Downloads", &xdg.downloads),
            ("Pictures", &xdg.pictures),
            ("Music", &xdg.music),
            ("Videos", &xdg.videos),
            ("Desktop", &xdg.desktop),
        ] {
            if let Some(ref resolved) = host_dir {
                let container_prefix = format!("{}/{}/", home_in_container, dir_name);
                if path_str.starts_with(&container_prefix) {
                    let relative = path_str.strip_prefix(&container_prefix).unwrap_or("");
                    let host_path = resolved.path.join(relative);
                    println!("{}", host_path.display());
                    return Ok(());
                }
            }
        }

        println!("{path_str}");
    }

    Ok(())
}
