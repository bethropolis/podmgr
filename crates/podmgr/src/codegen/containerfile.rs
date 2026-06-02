use crate::config::Config;

/// Generate a Containerfile string from the config.
pub fn generate(config: &Config, _guest_binary_name: &str) -> String {
    if config.image.prebuilt {
        return generate_prebuilt(config);
    }
    generate_custom(config)
}

fn generate_prebuilt(config: &Config) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("FROM {}", config.image.base));
    lines.push(String::new());

    lines.extend(generate_package_install_lines(config));
    lines.extend(generate_package_remove_lines(config));
    lines.extend(generate_run_command_lines(config));

    lines.push(format!(
        "ENV PODMGR_CONTAINER={}",
        config.container.name
    ));
    lines.push(String::new());

    lines.push("ENTRYPOINT [\"/usr/local/bin/podmgr-guest\", \"--entry\"]".into());
    lines.push(format!(
        "CMD [\"{}\"]",
        config.container.shell
    ));

    lines.join("\n")
}

fn generate_custom(config: &Config) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("FROM {}", config.image.base));
    lines.push(String::new());

    lines.extend(generate_package_install_lines(config));
    lines.extend(generate_package_remove_lines(config));
    lines.extend(generate_run_command_lines(config));

    // Integration layer
    lines.push("COPY podmgr-guest /usr/local/bin/podmgr-guest".into());
    lines.push(
        "RUN chmod +x /usr/local/bin/podmgr-guest".into(),
    );
    lines.push(String::new());

    lines.push(format!(
        "ENV PODMGR_CONTAINER={}",
        config.container.name
    ));
    lines.push(String::new());

    lines.push("ENTRYPOINT [\"/usr/local/bin/podmgr-guest\", \"--entry\"]".into());
    lines.push(format!(
        "CMD [\"{}\"]",
        config.container.shell
    ));

    lines.join("\n")
}

fn generate_package_install_lines(config: &Config) -> Vec<String> {
    let mut lines = Vec::new();
    let manager = config.image.packages.manager.as_str();
    if !config.image.packages.install.is_empty() {
        let pkgs = config.image.packages.install.join(" ");
        let cmd = match manager {
            "apt" | "apt-get" => format!("apt-get update && apt-get install -y {} && rm -rf /var/lib/apt/lists/*", pkgs),
            "apk" => format!("apk add --no-cache {}", pkgs),
            "pacman" => format!("pacman -Syu --noconfirm {} && pacman -Scc --noconfirm", pkgs),
            _ => format!("dnf install -y {} && dnf clean all", pkgs),
        };
        lines.push(format!("RUN {}", cmd));
        lines.push(String::new());
    }
    lines
}

fn generate_package_remove_lines(config: &Config) -> Vec<String> {
    let mut lines = Vec::new();
    let manager = config.image.packages.manager.as_str();
    if !config.image.packages.remove.is_empty() {
        let pkgs = config.image.packages.remove.join(" ");
        let cmd = match manager {
            "apt" | "apt-get" => format!("apt-get purge -y {} && apt-get autoremove -y && rm -rf /var/lib/apt/lists/*", pkgs),
            "apk" => format!("apk del {}", pkgs),
            "pacman" => format!("pacman -Rns --noconfirm {}", pkgs),
            _ => format!("dnf remove -y {} && dnf clean all", pkgs),
        };
        lines.push(format!("RUN {}", cmd));
        lines.push(String::new());
    }
    lines
}

fn generate_run_command_lines(config: &Config) -> Vec<String> {
    let mut lines = Vec::new();
    for cmd in &config.image.run.commands {
        lines.push(format!("RUN {}", cmd));
    }
    if !config.image.run.commands.is_empty() {
        lines.push(String::new());
    }
    lines
}
