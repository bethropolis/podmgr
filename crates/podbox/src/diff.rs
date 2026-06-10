use std::collections::BTreeSet;
use std::process::Command;

use anyhow::Result;

use crate::codegen::distros::DistroFamily;
use crate::config::Config;
use crate::config::PackageManager;

#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Packages listed in `config.image.packages.install`.
    pub config_install: Vec<String>,
    /// All packages actually present in the container (as reported by the
    /// native package-manager query).
    pub container_packages: Vec<String>,
    /// Packages from `config_install` that are **not** installed.
    pub missing: Vec<String>,
    /// Packages present in the container that are not in the base set and
    /// not in `config_install` — potential drift additions.
    pub unexpected: Vec<String>,
    /// Whether any drift was detected.
    pub has_drift: bool,
}

/// Compare the packages declared in `config` against what is actually
/// installed in the running container `name`.
///
/// The container must be running — the function runs
/// `podman exec` internally to query package state.
pub fn compute(config: &Config, name: &str, username: &str) -> Result<DiffResult> {
    let manager = resolve_manager(config);
    let raw = query_packages(name, username, manager)?;
    let container_packages = parse_package_list(&raw, manager);

    let config_set: BTreeSet<String> = config.image.packages.install.iter().cloned().collect();
    let container_set: BTreeSet<String> = container_packages.iter().cloned().collect();

    let missing: Vec<String> = config_set.difference(&container_set).cloned().collect();

    // For prebuilt images only report missing packages — the unexpected
    // list is always noisy because prebuilt images ship hundreds of
    // packages that aren't in our base-package reference.
    let unexpected = if config.image.source().is_prebuilt() {
        vec![]
    } else {
        compute_unexpected(&container_set, &config_set, manager)
    };

    let has_drift = !missing.is_empty() || !unexpected.is_empty();

    Ok(DiffResult {
        config_install: config.image.packages.install.clone(),
        container_packages,
        missing,
        unexpected,
        has_drift,
    })
}

/// Map the explicit `manager` field or fall back to distro-based detection.
fn resolve_manager(config: &Config) -> PackageManager {
    if config.image.packages.manager == PackageManager::Dnf
        || config.image.packages.manager == PackageManager::Apt
        || config.image.packages.manager == PackageManager::Pacman
        || config.image.packages.manager == PackageManager::Apk
    {
        config.image.packages.manager
    } else {
        DistroFamily::from_base_image(&config.image.base).manager()
    }
    // Zypper is valid but doesn't have a query command yet, so it falls
    // through to the dnf/rpm default below.
}

/// Returns the command + arguments used to query *all* installed packages
/// inside a running container for the given package manager.
fn query_cmd(manager: PackageManager) -> (&'static str, &'static [&'static str]) {
    match manager {
        PackageManager::Apt => ("dpkg-query", &["-W", "-f", "${Package}\n"]),
        PackageManager::Pacman => ("pacman", &["-Qqn"]),
        PackageManager::Apk => ("apk", &["list", "-I"]),
        // dnf / rpm default (also used for Zypper)
        PackageManager::Dnf | PackageManager::Zypper => {
            ("rpm", &["-qa", "--queryformat", "%{NAME}\n"])
        }
    }
}

fn query_packages(name: &str, username: &str, manager: PackageManager) -> Result<String> {
    let (cmd, args) = query_cmd(manager);
    let output = Command::new("podman")
        .arg("exec")
        .arg("-u")
        .arg(username)
        .arg(name)
        .arg(cmd)
        .args(args)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman exec {} {} failed: {}", name, cmd, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse a raw package-manager output into a sorted, deduplicated list of
/// package names.
fn parse_package_list(raw: &str, manager: PackageManager) -> Vec<String> {
    let mut pkgs: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| normalize_package_name(l, manager))
        .collect();
    pkgs.sort();
    pkgs.dedup();
    pkgs
}

/// Normalize a single line of package-manager output to a plain package
/// name.  Returns `None` for lines that should be skipped.
fn normalize_package_name(line: &str, manager: PackageManager) -> Option<String> {
    match manager {
        // apk list -I:  "zlib-1.3.1-r0 x86_64 {zlib}"  →  "zlib"
        PackageManager::Apk => {
            if let Some(start) = line.find('{') {
                let end = line.find('}')?;
                let name = line[start + 1..end].trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
            let first = line.split_whitespace().next()?;
            let name = first.rsplit_once('-').map(|(n, _)| n).unwrap_or(first);
            Some(name.to_string())
        }
        // dpkg-query -W / rpm -qa / pacman -Qqn all give plain names
        _ => {
            let name = line.trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        }
    }
}

/// Determine which container packages are "unexpected" — i.e., not part
/// of the base image and not declared in `config_install`.
///
/// Uses the distro's known base-package set as the reference.
fn compute_unexpected(
    container_set: &BTreeSet<String>,
    config_set: &BTreeSet<String>,
    manager: PackageManager,
) -> Vec<String> {
    let distro = match manager {
        PackageManager::Apt => DistroFamily::DebianLike,
        PackageManager::Pacman => DistroFamily::ArchLike,
        PackageManager::Apk => DistroFamily::AlpineLike,
        PackageManager::Dnf | PackageManager::Zypper => DistroFamily::FedoraLike,
    };
    let base_set: BTreeSet<String> = distro.base_packages(None).into_iter().collect();

    container_set
        .difference(config_set)
        .filter(|pkg| !base_set.contains(pkg.as_str()))
        .cloned()
        .collect()
}

/// Format a diff report for display.
pub fn format_report(result: &DiffResult) -> String {
    let mut lines = Vec::new();

    if !result.has_drift {
        lines.push("✓ All declared packages are installed — no drift detected.".to_string());
        return lines.join("\n");
    }

    if !result.missing.is_empty() {
        lines.push("── Declared packages NOT installed ──".to_string());
        for pkg in &result.missing {
            lines.push(format!("  - {}", pkg));
        }
        lines.push(String::new());
    }

    if !result.unexpected.is_empty() {
        lines.push("── Unexpected packages found ──".to_string());
        let show: Vec<&String> = result.unexpected.iter().take(30).collect();
        for pkg in &show {
            lines.push(format!("  + {}", pkg));
        }
        if result.unexpected.len() > 30 {
            lines.push(format!("  … and {} more", result.unexpected.len() - 30));
        }
        lines.push(String::new());
    }

    lines.push(format!(
        "{} missing, {} unexpected — drift detected.",
        result.missing.len(),
        result.unexpected.len(),
    ));

    lines.join("\n")
}

/// Patch the `install` array in a TOML definition to match the given
/// package list.  Returns the patched TOML string.
///
/// Uses `toml_edit` to preserve comments and formatting while safely
/// updating or inserting the `[image.packages].install` key, even when
/// the array is formatted across multiple lines.
pub fn patch_toml(original: &str, install: &[String]) -> Result<String> {
    let mut doc = original
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("Failed to parse TOML for editing: {}", e))?;

    let mut arr = toml_edit::Array::new();
    for pkg in install {
        arr.push(pkg);
    }

    doc["image"]["packages"]["install"] = toml_edit::value(arr);
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_package_list ----

    #[test]
    fn parse_rpm_output() {
        let raw = "bash\ncoreutils\nsudo\nzlib\nbash\n";
        let pkgs = parse_package_list(raw, PackageManager::Dnf);
        assert_eq!(pkgs, vec!["bash", "coreutils", "sudo", "zlib"]);
    }

    #[test]
    fn parse_dpkg_output() {
        let raw = "bash\ncoreutils\nsudo\nzlib\n";
        let pkgs = parse_package_list(raw, PackageManager::Apt);
        assert_eq!(pkgs, vec!["bash", "coreutils", "sudo", "zlib"]);
    }

    #[test]
    fn parse_pacman_output() {
        let raw = "bash\ncoreutils\nsudo\nzlib\n";
        let pkgs = parse_package_list(raw, PackageManager::Pacman);
        assert_eq!(pkgs, vec!["bash", "coreutils", "sudo", "zlib"]);
    }

    #[test]
    fn parse_apk_output() {
        let raw = "zlib-1.3.1-r0 x86_64 {zlib}\nalpine-base-3.20.0 x86_64 {alpine-base}\n";
        let pkgs = parse_package_list(raw, PackageManager::Apk);
        assert_eq!(pkgs, vec!["alpine-base", "zlib"]);
    }

    #[test]
    fn parse_empty_output() {
        let pkgs = parse_package_list("", PackageManager::Dnf);
        assert!(pkgs.is_empty());
    }

    #[test]
    fn parse_whitespace_only() {
        let pkgs = parse_package_list("  \n  \n", PackageManager::Dnf);
        assert!(pkgs.is_empty());
    }

    // ---- compute_unexpected ----

    #[test]
    fn unexpected_excludes_base_packages() {
        let container: BTreeSet<String> = ["sudo", "curl", "bash", "unrecognized-tool"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let config: BTreeSet<String> = ["bash"].iter().map(|s| s.to_string()).collect();
        // sudo and curl are base packages, should NOT be unexpected.
        // unrecognized-tool is NOT a base package → unexpected.
        let unexpected = compute_unexpected(&container, &config, PackageManager::Dnf);
        assert_eq!(unexpected, vec!["unrecognized-tool"]);
    }

    // ---- format_report ----

    #[test]
    fn report_no_drift() {
        let result = DiffResult {
            config_install: vec!["git".into()],
            container_packages: vec!["git".into()],
            missing: vec![],
            unexpected: vec![],
            has_drift: false,
        };
        let report = format_report(&result);
        assert!(report.contains("no drift detected"));
    }

    #[test]
    fn report_with_missing() {
        let result = DiffResult {
            config_install: vec!["git".into(), "htop".into()],
            container_packages: vec!["git".into()],
            missing: vec!["htop".into()],
            unexpected: vec![],
            has_drift: true,
        };
        let report = format_report(&result);
        assert!(report.contains("htop"));
        assert!(report.contains("missing"));
    }

    // ---- patch_toml ----

    #[test]
    fn patch_toml_replaces_existing_install() {
        let original = r#"[image]
base = "fedora:41"
name = "myenv"

[image.packages]
install = ["git", "gcc"]
remove = []
"#;
        let patched = patch_toml(original, &["git".into(), "htop".into()]).unwrap();
        assert!(patched.contains(r#"install = ["git", "htop"]"#));
        // Should not duplicate the key
        assert_eq!(patched.matches("install =").count(), 1);
    }

    #[test]
    fn patch_toml_adds_install_when_missing() {
        let original = r#"[image]
base = "fedora:41"
name = "myenv"

[image.packages]
remove = []
"#;
        let patched = patch_toml(original, &["git".into()]).unwrap();
        assert!(patched.contains(r#"install = ["git"]"#));
    }

    #[test]
    fn patch_toml_creates_section_when_absent() {
        let original = r#"[image]
base = "fedora:41"
name = "myenv"
"#;
        let patched = patch_toml(original, &["git".into()]).unwrap();
        assert!(patched.contains(r#"install = ["git"]"#));
    }

    #[test]
    fn patch_toml_empty_install() {
        let original = r#"[image]
base = "fedora:41"
name = "myenv"

[image.packages]
install = ["git"]
"#;
        let patched = patch_toml(original, &[] as &[String]).unwrap();
        assert!(patched.contains(r#"install = []"#));
    }
}
