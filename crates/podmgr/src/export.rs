use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::Result;

use crate::error::PodmgrError;

/// Export an application as a .desktop file on the host.
pub fn export_app(container_name: &str, app: &str) -> Result<()> {
    // 1. Get .desktop file from container
    let container_path = format!("/usr/share/applications/{}.desktop", app);
    let args: Vec<OsString> = vec![
        "exec".into(),
        container_name.into(),
        "cat".into(),
        container_path.into(),
    ];
    let output = crate::process::run_piped("podman", &args)?;
    if !output.status.success() {
        return Err(
            PodmgrError::ExportFailed(format!("app {} not found in container", app)).into()
        );
    }
    let desktop_content = String::from_utf8_lossy(&output.stdout);

    // 2. Rewrite Name= and Exec= lines
    let rewritten = rewrite_desktop_file(&desktop_content, container_name, app);

    // 3. Write host .desktop file
    let apps_dir = dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".local/share"))
                .unwrap_or_else(|| PathBuf::from("/usr/local/share"))
        })
        .join("applications");
    std::fs::create_dir_all(&apps_dir)?;

    let host_path = apps_dir.join(format!("podmgr-{}-{}.desktop", container_name, app));
    std::fs::write(&host_path, rewritten)?;

    // 4. Try to extract icon
    if let Some(icon_name) = extract_icon_name(&desktop_content) {
        if let Err(e) = copy_icon_from_container(container_name, &icon_name, container_name) {
            eprintln!("Warning: failed to copy icon '{}': {}", icon_name, e);
        }
    }

    // 5. Update desktop database
    if let Err(e) = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .output()
        .map(|_| ())
    {
        eprintln!("Warning: update-desktop-database failed: {}", e);
    }

    println!("Exported app '{}'.desktop -> {}", app, host_path.display());
    Ok(())
}

/// Export a binary shim to ~/.local/bin.
pub fn export_bin(container_name: &str, bin: &str) -> Result<()> {
    let bin_dir = dirs::home_dir()
        .map(|h| h.join(".local/bin"))
        .unwrap_or_else(|| PathBuf::from("/usr/local/bin"));
    std::fs::create_dir_all(&bin_dir)?;

    let shim = format!(
        "#!/bin/sh\nexec podmgr --container \"{}\" exec -- \"{}\" \"$@\"\n",
        container_name.replace('"', "\\\""), bin.replace('"', "\\\"")
    );

    let shim_path = bin_dir.join(bin);
    std::fs::write(&shim_path, shim)?;
    #[allow(clippy::print_literal)]
    {
        let _ = std::fs::set_permissions(&shim_path, std::fs::Permissions::from_mode(0o755));
    }

    println!("Exported bin shim '{}' -> {}", bin, shim_path.display());
    Ok(())
}

/// Remove all exports for a container.
pub fn unexport_all(container_name: &str) -> Result<()> {
    let apps_dir = dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".local/share"))
                .unwrap_or_else(|| PathBuf::from("/usr/local/share"))
        })
        .join("applications");
    let prefix = format!("podmgr-{}", container_name);

    if let Ok(entries) = std::fs::read_dir(&apps_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let icons_dir = dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".local/share"))
                .unwrap_or_else(|| PathBuf::from("/usr/local/share"))
        })
        .join(format!("icons/podmgr/{}", container_name));
    let _ = std::fs::remove_dir_all(&icons_dir);

    let bin_dir = dirs::home_dir()
        .map(|h| h.join(".local/bin"))
        .unwrap_or_else(|| PathBuf::from("/usr/local/bin"));

    // Remove shims that reference this container
    let shim_marker = format!("--container \"{}\"", container_name);
    if let Ok(entries) = std::fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if content.contains(&shim_marker) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    println!("Unexported all apps and bins for '{}'.", container_name);
    Ok(())
}

fn rewrite_desktop_file(content: &str, container_name: &str, _app: &str) -> String {
    let suffix = format!("({})", container_name);
    content
        .lines()
        .map(|line| {
            if let Some(original) = line.strip_prefix("Exec=") {
                format!("Exec=podmgr --container \"{}\" exec -- {}", container_name.replace('"', "\\\""), original)
            } else if let Some((key, val)) = line.split_once('=') {
                if (key == "Name" || key.starts_with("Name[")) && !val.contains(&suffix) {
                    format!("{}={} ({})", key, val, container_name)
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_icon_name(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("Icon=").map(|s| s.to_string())
    })
}

fn copy_icon_from_container(
    container_name: &str,
    icon_name: &str,
    _profile: &str,
) -> Result<()> {
    // Sanitize icon name: refuse path separators to prevent traversal
    if icon_name.contains('/') || icon_name.contains("..") {
        return Err(anyhow::anyhow!(
            "icon name contains path separators, refusing: {}",
            icon_name
        ));
    }

    let icons_dir = dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .map(|h| h.join(".local/share"))
                .unwrap_or_else(|| PathBuf::from("/usr/local/share"))
        })
        .join(format!("icons/podmgr/{}", container_name));
    std::fs::create_dir_all(&icons_dir)?;

    let icon_paths: Vec<String> = vec![
        format!("/usr/share/icons/hicolor/48x48/apps/{}.png", icon_name),
        format!("/usr/share/icons/hicolor/scalable/apps/{}.svg", icon_name),
        format!("/usr/share/icons/hicolor/64x64/apps/{}.png", icon_name),
        format!("/usr/share/icons/hicolor/128x128/apps/{}.png", icon_name),
        format!("/usr/share/icons/hicolor/256x256/apps/{}.png", icon_name),
        format!("/usr/share/icons/hicolor/48x48/apps/{}.svg", icon_name),
    ];

    for path in &icon_paths {
        let ext = std::path::Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy())
            .unwrap_or_default();
        let args: Vec<OsString> = vec![
            "exec".into(),
            container_name.into(),
            "cat".into(),
            path.into(),
        ];
        let output = crate::process::run_piped("podman", &args)?;
        if output.status.success() {
            let dest = icons_dir.join(format!("{}.{}", icon_name, ext));
            std::fs::write(dest, &output.stdout)?;
            break;
        }
    }

    Ok(())
}
