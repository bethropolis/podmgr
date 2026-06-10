use anyhow::Result;

use podbox::config::{Config, ImageSource};
use podbox::error::PodboxError;

/// Pull a container image and tag it for podbox use.
pub fn run_pull(config: &Config, image: &Option<String>, dry_run: bool) -> Result<()> {
    let image_ref = match image {
        Some(ref img) => img.clone(),
        None => match config.image.source() {
            ImageSource::Prebuilt { ref_str } => ref_str,
            ImageSource::Build { base } => base,
        },
    };
    if dry_run {
        println!("podman pull {}", image_ref);
        println!(
            "podman tag {} localhost/podbox-{}:latest",
            image_ref, config.image.name
        );
        return Ok(());
    }
    let local_tag = format!("localhost/podbox-{}:latest", config.image.name);
    println!("Pulling {}...", image_ref);
    let status = std::process::Command::new("podman")
        .args(["pull", &image_ref])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|_| PodboxError::PullFailed(image_ref.clone()))?;
    if !status.success() {
        return Err(PodboxError::PullFailed(image_ref.clone()).into());
    }
    println!("Tagging {} as {}...", image_ref, local_tag);
    let tag_status = std::process::Command::new("podman")
        .args(["tag", &image_ref, &local_tag])
        .status()
        .map_err(|_| PodboxError::TagFailed(image_ref.clone()))?;
    if !tag_status.success() {
        return Err(PodboxError::TagFailed(image_ref.clone()).into());
    }
    let context_dir = podbox::build::build_context_dir(&config.image.name);
    std::fs::create_dir_all(&context_dir)?;
    let digest = podbox::podman::image_digest(&local_tag)?;
    let lock = podbox::lock::LockFile {
        config_checksum: podbox::build::checksum(&image_ref),
        image_digest: digest,
    };
    let lock_path = context_dir.join(".podbox.lock");
    podbox::lock::write(&lock_path, &lock)?;
    println!("Lock file written to {}", lock_path.display());
    Ok(())
}
