use std::path::PathBuf;

/// A named, ready-to-use configuration template.
pub struct Profile {
    pub name: String,
    pub label: String,
    pub description: String,
    pub toml: String,
}

/// List all available profiles (bundled + user-defined).
pub fn all() -> Vec<Profile> {
    let mut profiles = vec![cachy(), fedora(), gaming()];
    profiles.extend(user_profiles());
    profiles
}

/// Find a profile by name (case-insensitive).
pub fn find(name: &str) -> Option<Profile> {
    let lower = name.to_lowercase();
    // Check bundled first, then user-defined
    for p in [cachy(), fedora(), gaming()] {
        if p.name == lower {
            return Some(p);
        }
    }
    user_profiles().into_iter().find(|p| p.name == lower)
}

fn user_profiles_dir() -> PathBuf {
    crate::config::config_dir().join("profiles")
}

fn user_profiles() -> Vec<Profile> {
    let dir = user_profiles_dir();
    if !dir.is_dir() {
        return vec![];
    }
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "toml").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let name = path.file_stem()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    profiles.push(Profile {
                        label: name.clone(),
                        description: format!("User-defined profile ({})", name),
                        toml: content,
                        name,
                    });
                }
            }
        }
    }
    profiles
}

fn cachy() -> Profile {
    Profile {
        name: "cachy".into(),
        label: "CachyOS".into(),
        description: "CachyOS-based environment optimized for gaming".into(),
        toml: include_str!("profiles/cachy.toml").into(),
    }
}

fn fedora() -> Profile {
    Profile {
        name: "fedora".into(),
        label: "Fedora".into(),
        description: "Fedora-based general-purpose environment".into(),
        toml: include_str!("profiles/fedora.toml").into(),
    }
}

fn gaming() -> Profile {
    Profile {
        name: "gaming".into(),
        label: "Gaming".into(),
        description: "Generic gaming environment (distro-agnostic)".into(),
        toml: include_str!("profiles/gaming.toml").into(),
    }
}

/// List profile names for tab completion / CLI hints.
pub fn list_names() -> Vec<String> {
    all().into_iter().map(|p| p.name).collect()
}
