use anyhow::Result;

use podbox::codegen::quadlet;
use podbox::config::{Config, ImageSource};
use podbox::env::HostEnv;
use podbox::xdg::ResolvedXdgDirs;

/// Inspect container config, Quadlet, or computed env.
pub fn run_inspect(
    config: &Config,
    _name: &str,
    env: &HostEnv,
    xdg: &ResolvedXdgDirs,
    show_config: bool,
    show_quadlet: bool,
    show_env: bool,
) -> Result<()> {
    let all = !show_config && !show_quadlet && !show_env;

    if all || show_config {
        println!("--- Config ---");
        let toml_str = toml::to_string_pretty(config)?;
        println!("{}", toml_str);
    }

    if all || show_quadlet {
        println!("--- Quadlet (.container) ---");
        let q = quadlet::generate_container(config, env, xdg);
        println!("{}", q);
        println!();
        println!("--- Quadlet (.socket) ---");
        let s = quadlet::generate_socket(config);
        println!("{}", s);
    }

    if all || show_env {
        println!("--- Environment ---");
        println!("Container name:  {}", config.container.name);
        let image_ref = match config.image.source() {
            ImageSource::Build { base } => format!("build:{}", base),
            ImageSource::Prebuilt { ref_str } => ref_str.clone(),
        };
        println!("Image ref:       {}", image_ref);
        println!("Image source:    {:?}", config.image.source());
        println!("Quadlet:         {}", config.lifecycle.quadlet);
        println!("Auto-start:      {}", config.lifecycle.autostart);
        println!("Auto-update:     {}", config.lifecycle.auto_update);
        println!();
        println!("XDG_RUNTIME_DIR: {}", env.xdg_runtime_dir.display());
        if let Some(ref w) = env.wayland_display {
            println!("WAYLAND_DISPLAY: {}", w);
        }
        if env.gpu_has_dri {
            println!("GPU (DRI):       yes");
        }
        if env.gpu_has_nvidia {
            println!("GPU (NVIDIA):    yes");
        }
        if let Some(ref dbus) = env.dbus_socket {
            println!("D-Bus socket:    {}", dbus.display());
        }
        if env.gpg_agent_socket.is_some() {
            println!("GPG agent:       available");
        }
        if let Some(ref shell) = env.host_shell {
            println!("Host shell:      {}", shell);
        }
        if let Some(ref locale) = env.host_locale {
            println!("Host locale:     {}", locale);
        }
    }

    Ok(())
}
