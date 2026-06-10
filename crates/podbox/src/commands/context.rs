use anyhow::Result;

use podbox::config;

/// Set or show the active context.
pub fn run_use(name: Option<String>, clear: bool, dry_run: bool) -> Result<()> {
    if clear {
        if dry_run {
            println!("Would clear active context.");
            return Ok(());
        }
        config::clear_active_context()?;
        println!("Active context cleared.");
        return Ok(());
    }

    match name {
        Some(n) => {
            let config_path = config::config_dir().join(format!("{}.toml", n));
            if !config_path.exists() {
                anyhow::bail!("Config '{}' not found at {}", n, config_path.display());
            }
            if dry_run {
                println!("Would set active context to '{}'.", n);
                return Ok(());
            }
            config::write_active_context(&n)?;
            println!("Active context set to '{}'.", n);
        }
        None => match config::read_active_context() {
            Some(n) => println!("{}", n),
            None => println!("No active context set."),
        },
    }
    Ok(())
}
