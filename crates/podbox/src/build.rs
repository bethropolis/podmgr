use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::codegen::containerfile;
use crate::codegen::distros::DistroFamily;
use crate::config::Config;
use crate::env::HostEnv;
use crate::error::PodboxError;
use crate::xdg::ResolvedXdgDirs;

/// Locate the podbox-guest binary.
fn find_guest_binary() -> Result<PathBuf> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join("podbox-guest");
            if candidate.exists() {
                return Ok(candidate);
            }
            // Compat: old name
            let old = dir.join("podmgr-guest");
            if old.exists() {
                return Ok(old);
            }
        }
    }
    if let Ok(path) = which::which("podbox-guest") {
        return Ok(path);
    }
    // Compat: old binary name
    if let Ok(path) = which::which("podmgr-guest") {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("PODBOX_GUEST_BIN") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }
    // Compat: old env var
    if let Ok(path) = std::env::var("PODMGR_GUEST_BIN") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(PodboxError::GuestBinaryNotFound.into())
}

/// SHA-256 hex digest of a string, used for lock-file invalidation.
pub fn checksum(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Build context directory: ~/.local/share/podbox/<name>/
pub fn build_context_dir(name: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("podbox")
        .join(name)
}

/// Run the full build orchestration.
pub fn run(
    config: &Config,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    dry_run: bool,
    rebuild: bool,
) -> Result<()> {
    if config.image.source().is_prebuilt() {
        run_prebuilt(config, dry_run, rebuild)
    } else {
        run_build(config, env, xdg, dry_run, rebuild)
    }
}

// --- Prebuilt image path ----------------------------------------------------

fn run_prebuilt(config: &Config, dry_run: bool, rebuild: bool) -> Result<()> {
    let image_ref = match config.image.source() {
        crate::config::ImageSource::Prebuilt { ref_str } => ref_str,
        _ => config.image.base.clone(),
    };
    let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
    let context_dir = build_context_dir(&config.container.name);
    let lock_path = context_dir.join(".podbox.lock");
    let has_packages = !config.image.packages.install.is_empty();

    // Checksum covers both the image ref and the install list so that
    // changing either triggers a rebuild.
    let definition_toml = toml::to_string(config)
        .with_context(|| "failed to serialize definition config".to_string())?;
    let config_checksum = checksum(&definition_toml);

    if !rebuild {
        if let Some(lock) = crate::lock::read(&lock_path)? {
            if lock.config_checksum == config_checksum && crate::podman::image_exists(&local_tag)?
            {
                println!(
                    "Prebuilt image already present as {}. Skipping pull.",
                    local_tag
                );
                println!("Use --rebuild to re-pull.");
                return Ok(());
            }
        }
    }

    if dry_run {
        println!("Would pull: {}", image_ref);
        if has_packages {
            println!("Would install packages on top: {}", config.image.packages.install.join(", "));
        }
        println!("Would tag as: {}", local_tag);
        println!("Would write lock file at: {}", lock_path.display());
        return Ok(());
    }

    // Warn on version mismatch from labels (best-effort, image may not exist yet)
    if let Ok(labels) = crate::podman::image_labels(&image_ref) {
        if let Some(guest_ver) = labels
            .get("podbox.guest_version")
            .or_else(|| labels.get("podmgr.guest_version"))
        {
            let guest_clean = guest_ver.trim_start_matches('v');
            let host_clean = crate::VERSION.trim_start_matches('v');
            if guest_clean != host_clean {
                eprintln!(
                    "Warning: image guest version (v{}) differs from host (v{}). \
                     Protocol compatibility will be validated at runtime.",
                    guest_clean,
                    host_clean
                );
            }
        }
    }

    println!("Pulling {}...", image_ref);
    let status = std::process::Command::new("podman")
        .args(["pull", &image_ref])
        .status()?;
    if !status.success() {
        return Err(PodboxError::PullFailed(image_ref.clone()).into());
    }

    if has_packages {
        // Layer the config's packages on top of the prebuilt image.
        let distro = resolve_prebuilt_distro(config);
        let install_cmd = distro.install_cmd();
        let clean_cmd = distro.clean_cmd();

        let packages = config.image.packages.install.join(" ");
        let run_line = if clean_cmd.is_empty() {
            format!("RUN {} {}", install_cmd, packages)
        } else {
            format!("RUN {} {} && {}", install_cmd, packages, clean_cmd)
        };

        let containerfile = format!(
            "FROM {}\n{}\n",
            image_ref, run_line
        );

        std::fs::create_dir_all(&context_dir)
            .with_context(|| format!("failed to create context dir '{}'", context_dir.display()))?;

        let containerfile_path = context_dir.join("Containerfile");
        std::fs::write(&containerfile_path, &containerfile).with_context(|| {
            format!(
                "failed to write Containerfile to '{}'",
                containerfile_path.display()
            )
        })?;

        println!("Installing packages on top of prebuilt image...");
        let args: Vec<std::ffi::OsString> = vec![
            "build".into(),
            "-t".into(),
            local_tag.clone().into(),
            "-f".into(),
            containerfile_path.clone().into(),
            context_dir.clone().into(),
        ];
        let output = crate::process::run_piped("podman", &args)
            .with_context(|| format!("failed to build prebuilt overlay for '{}'", image_ref))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PodboxError::BuildFailed(stderr.to_string()).into());
        }
        println!("Image {} ready with packages installed.", local_tag);
    } else {
        println!("Tagging as {}...", local_tag);
        let status = std::process::Command::new("podman")
            .args(["tag", &image_ref, &local_tag])
            .status()?;
        if !status.success() {
            return Err(PodboxError::TagFailed(local_tag.clone()).into());
        }
        println!("Image {} ready.", local_tag);
    }

    std::fs::create_dir_all(&config.container.home).with_context(|| {
        format!(
            "failed to create home dir '{}'",
            config.container.home.display()
        )
    })?;
    let digest = crate::podman::image_digest(&local_tag)?;
    let lock = crate::lock::LockFile {
        config_checksum,
        image_digest: digest,
    };
    crate::lock::write(&lock_path, &lock)?;

    Ok(())
}

/// Resolve the distro family for package installation on a prebuilt image.
/// Respects the explicit `manager` field in the config, falling back to
/// name-based detection via `DistroFamily`.
fn resolve_prebuilt_distro(config: &Config) -> DistroFamily {
    match config.image.packages.manager.trim().to_lowercase().as_str() {
        "apt" => DistroFamily::DebianLike,
        "dnf" => DistroFamily::FedoraLike,
        "pacman" => DistroFamily::ArchLike,
        "apk" => DistroFamily::AlpineLike,
        _ => DistroFamily::from_base_image(&config.image.base),
    }
}

// --- Custom build path ------------------------------------------------------

fn run_build(
    config: &Config,
    _env: &HostEnv,
    _xdg: &ResolvedXdgDirs,
    dry_run: bool,
    rebuild: bool,
) -> Result<()> {
    let name = &config.container.name;
    let context_dir = build_context_dir(name);
    let containerfile_path = context_dir.join("Containerfile");
    let lock_path = context_dir.join(".podbox.lock");

    let guest_bin = find_guest_binary()?;

    let definition_toml = toml::to_string(config)
        .with_context(|| "failed to serialize definition config".to_string())?;
    let config_checksum = checksum(&definition_toml);

    if !rebuild {
        if let Some(lock) = crate::lock::read(&lock_path)? {
            if lock.config_checksum == config_checksum {
                println!("Definition unchanged and image already built. Skipping.");
                println!("Use --rebuild to force.");
                return Ok(());
            }
        }
    }

    let containerfile = containerfile::generate(config, "podbox-guest");

    if dry_run {
        println!("=== Build context: {} ===", context_dir.display());
        println!("=== Containerfile ===");
        println!("{}", containerfile);
        println!();
        println!("=== Files to copy ===");
        println!("{} -> podbox-guest", guest_bin.display());
        println!(
            "podman build -t localhost/podbox-{}:latest {}",
            config.image.name,
            context_dir.display()
        );
        return Ok(());
    }

    std::fs::create_dir_all(&context_dir)
        .map_err(|e| PodboxError::HomeCreateFailed(context_dir.clone(), e))?;
    // Best-effort permission tightening: ignore failure (e.g. on FAT mounts).
    let _ = std::fs::set_permissions(&context_dir, std::fs::Permissions::from_mode(0o700));

    std::fs::write(&containerfile_path, containerfile).with_context(|| {
        format!(
            "failed to write Containerfile to '{}'",
            containerfile_path.display()
        )
    })?;

    let guest_dest = context_dir.join("podbox-guest");
    std::fs::copy(&guest_bin, &guest_dest)
        .with_context(|| format!("failed to copy guest binary to '{}'", guest_dest.display()))?;

    std::fs::create_dir_all(&config.container.home).with_context(|| {
        format!(
            "failed to create home dir '{}'",
            config.container.home.display()
        )
    })?;

    let tag = format!("localhost/podbox-{}:latest", config.image.name);
    let args: Vec<OsString> = vec![
        "build".into(),
        "-t".into(),
        tag.clone().into(),
        context_dir.clone().into(),
    ];

    println!("Building image {}...", tag);
    let output = crate::process::run_piped("podman", &args)
        .with_context(|| format!("failed to execute podman build for image '{}'", tag))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PodboxError::BuildFailed(stderr.to_string()).into());
    }
    println!("Image {} built successfully.", tag);

    let digest = crate::podman::image_digest(&tag)?;
    let lock = crate::lock::LockFile {
        config_checksum,
        image_digest: digest,
    };
    crate::lock::write(&lock_path, &lock)?;

    Ok(())
}
