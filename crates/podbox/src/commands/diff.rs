use anyhow::Result;

use podbox::config::Config;
use podbox::diff;

/// Run a package-drift comparison between the config and the running
/// container.
///
/// When `apply` is true the definition file on disk is updated:
/// - Removed packages that are no longer in the container are dropped from
///   the `install` list.
/// - Unexpected packages found in the container are added to the `install`
///   list.
pub fn run_diff(config: &Config, name: &str, username: &str, apply: bool) -> Result<()> {
    let result = diff::compute(config, name, username)?;

    if !result.has_drift {
        println!("✓ No drift detected — all declared packages are installed.");
        return Ok(());
    }

    println!("{}", diff::format_report(&result));

    if apply {
        let definition_path = podbox::config::find_definition()
            .ok_or_else(|| anyhow::anyhow!("No definition file found to patch"))?;
        let original = std::fs::read_to_string(&definition_path)?;

        // New install list: keep config packages that exist in the
        // container, plus add unexpected packages.
        let mut reconciled: Vec<String> = result
            .config_install
            .iter()
            .filter(|p| !result.missing.contains(p))
            .cloned()
            .collect();
        for pkg in &result.unexpected {
            if !reconciled.contains(pkg) {
                reconciled.push(pkg.clone());
            }
        }
        reconciled.sort();

        let patched = diff::patch_toml(&original, &reconciled)?;
        std::fs::write(&definition_path, patched)?;
        println!(
            "\n✓ Updated {} with reconciled package list.",
            definition_path.display()
        );
    }

    Ok(())
}
