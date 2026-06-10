use anyhow::Result;

use crate::config::Config;
use crate::error::PodboxError;

impl Config {
    pub fn validate(&self) -> Result<()> {
        let mut errors: Vec<String> = Vec::new();

        if self.image.base.trim().is_empty() {
            errors.push("image.base: must not be empty".into());
        }
        if self.image.name.trim().is_empty() {
            errors.push("image.name: must not be empty".into());
        } else if !is_valid_name(&self.image.name) {
            errors.push(format!(
                "image.name: '{}' contains invalid characters (use letters, digits, hyphens, underscores, dots)",
                self.image.name
            ));
        }
        if let Some(ref r) = self.image.image_ref {
            if r.trim().is_empty() {
                errors.push("image.image: must not be empty when set".into());
            } else if !r.contains(':') && !r.contains('/') {
                errors.push(format!(
                    "image.image: '{}' does not look like a valid image reference (missing ':' or '/')",
                    r
                ));
            }
        }

        if self.container.name.trim().is_empty() {
            errors.push("container.name: must not be empty".into());
        } else if !is_valid_name(&self.container.name) {
            errors.push(format!(
                "container.name: '{}' contains invalid characters (use letters, digits, hyphens, underscores, dots)",
                self.container.name
            ));
        }
        if self.container.home.as_os_str().is_empty() {
            errors.push("container.home: must not be empty".into());
        }
        if self.container.shell.trim().is_empty() {
            errors.push("container.shell: must not be empty".into());
        }
        if let Some(ref mem) = self.container.memory {
            if !is_valid_memory(mem) {
                errors.push(format!(
                    "container.memory: '{}' is not a valid memory limit (e.g. '2g', '512m')",
                    mem
                ));
            }
        }
        for (i, mount) in self.container.mounts.extra.iter().enumerate() {
            if !mount.contains(':') {
                errors.push(format!(
                    "container.mounts.extra[{}]: '{}' missing ':' separator (expected host:container[:options])",
                    i, mount
                ));
            }
        }
        for (key, val) in &self.container.env {
            if key.contains('\n') {
                errors.push(format!("container.env: key {:?} contains newline", key));
            }
            if val.contains('\n') {
                errors.push(format!(
                    "container.env: value for {:?} contains newline",
                    key
                ));
            }
        }

        if let Some(ref map) = self.integration.host_exec.allowlist {
            for (alias, path) in map {
                if !is_absolute_path(path) {
                    errors.push(format!(
                        "integration.host_exec.allowlist.{}: path '{}' is not absolute (must start with '/')",
                        alias, path
                    ));
                }
            }
        }

        if self.integration.host_exec.enabled {
            let has_allowlist = self
                .integration
                .host_exec
                .allowlist
                .as_ref()
                .is_some_and(|m| !m.is_empty());
            if !has_allowlist {
                errors.push(
                    "integration.host_exec: 'enabled' is true, but 'allowlist' is missing or empty. \
                     For security, legacy open execution is blocked; you must explicitly define \
                     allowed host commands."
                        .into(),
                );
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(PodboxError::ConfigValidationFailed(errors.join("\n  - ")).into())
        }
    }
}

fn is_valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn is_absolute_path(s: &str) -> bool {
    s.starts_with('/')
}

fn is_valid_memory(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let digits: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let suffix: String = s.chars().skip(digits.len()).collect();
    if digits.is_empty() || digits == "." {
        return false;
    }
    if digits.starts_with('.') || (digits.chars().filter(|&c| c == '.').count() > 1) {
        return false;
    }
    suffix.is_empty()
        || matches!(
            suffix.as_str(),
            "k" | "K" | "m" | "M" | "g" | "G" | "t" | "T"
        )
}
