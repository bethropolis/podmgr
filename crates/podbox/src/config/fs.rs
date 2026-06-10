use std::path::PathBuf;

use anyhow::Result;

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("podbox"))
        .unwrap_or_else(|| PathBuf::from("~/.config/podbox"))
}

pub fn find_definition() -> Option<PathBuf> {
    let new_local = PathBuf::from(".podbox.toml");
    if new_local.exists() {
        return Some(new_local);
    }

    let old_local = PathBuf::from(".podmgr.toml");
    if old_local.exists() {
        eprintln!(
            "Warning: '.podmgr.toml' found. Rename it to '.podbox.toml' to silence this warning."
        );
        return Some(old_local);
    }

    let config_dir = config_dir();

    if config_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&config_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "toml")
                    .unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();

        entries.sort();
        if !entries.is_empty() {
            if entries.len() > 1 {
                eprintln!(
                    "Warning: multiple configuration files found in {}. Selecting '{}' alphabetically. Use --config to specify a different file.",
                    config_dir.display(),
                    entries[0].display()
                );
            }
            return Some(entries.remove(0));
        }
    }

    None
}

pub fn list_configs() -> Vec<PathBuf> {
    let config_dir = config_dir();
    if !config_dir.is_dir() {
        return vec![];
    }
    let mut entries: Vec<_> = std::fs::read_dir(&config_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    entries.sort();
    entries
}

pub fn active_context_path() -> PathBuf {
    config_dir().join(".active")
}

pub fn read_active_context() -> Option<String> {
    let path = active_context_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let name = content.trim().to_string();
    if name.is_empty() {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    let config_path = config_dir().join(format!("{}.toml", name));
    if config_path.exists() {
        Some(name)
    } else {
        let _ = std::fs::remove_file(&path);
        None
    }
}

pub fn write_active_context(name: &str) -> Result<()> {
    let path = active_context_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, name)?;
    Ok(())
}

pub fn clear_active_context() -> Result<()> {
    let path = active_context_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/foo"), home.join("foo"));
        assert_eq!(expand_tilde("~"), home.clone());
        assert_eq!(expand_tilde("/foo/bar"), PathBuf::from("/foo/bar"));
    }
}
