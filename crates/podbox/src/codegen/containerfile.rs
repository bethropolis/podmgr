use crate::codegen::distros::{detect_host_locale, detect_host_shell, DistroFamily};
use crate::config::Config;

pub const VERSION: &str = env!("PODBOX_VERSION");

pub fn generate(config: &Config, _guest_binary_name: &str) -> String {
    if config.image.source().is_prebuilt() {
        return generate_prebuilt(config);
    }
    generate_custom(config)
}

fn generate_prebuilt(config: &Config) -> String {
    let builder = ContainerfileBuilder::new(&config.image.base, &config.container.name);
    builder
        .add_user_packages(config.image.packages.install.clone())
        .add_run_commands(config.image.run.commands.clone())
        .set_shell(&config.container.shell)
        .build()
}

fn generate_custom(config: &Config) -> String {
    let distro = DistroFamily::from_base_image(&config.image.base);
    let host_shell = detect_host_shell();
    let host_locale = detect_host_locale();

    let builder = ContainerfileBuilder::new(&config.image.base, &config.container.name);
    builder
        .add_base_packages(distro, host_shell.as_deref(), host_locale.as_deref())
        .add_user_packages(config.image.packages.install.clone())
        .add_run_commands(config.image.run.commands.clone())
        .add_guest_binary()
        .build()
}

struct ContainerfileBuilder {
    base_image: String,
    container_name: String,
    packages: Vec<String>,
    run_commands: Vec<String>,
    has_guest_binary: bool,
    env_vars: Vec<(String, String)>,
    forced_shell: Option<String>,
}

impl ContainerfileBuilder {
    fn new(base_image: &str, container_name: &str) -> Self {
        Self {
            base_image: base_image.to_string(),
            container_name: container_name.to_string(),
            packages: Vec::new(),
            run_commands: Vec::new(),
            has_guest_binary: false,
            env_vars: Vec::new(),
            forced_shell: None,
        }
    }

    fn add_base_packages(
        mut self,
        distro: DistroFamily,
        host_shell: Option<&str>,
        host_locale: Option<&str>,
    ) -> Self {
        let mut pkgs = distro.base_packages(host_shell);
        let locale_pkgs = distro.locale_packages();
        for pkg in locale_pkgs {
            if !pkgs.contains(&pkg) {
                pkgs.push(pkg);
            }
        }
        self.packages = pkgs;
        if let Some(locale) = host_locale {
            self.env_vars.push(("LANG".into(), locale.to_string()));
            self.env_vars.push(("LC_ALL".into(), locale.to_string()));
            self.env_vars.push(("LC_CTYPE".into(), locale.to_string()));
        }
        self
    }

    fn add_user_packages(mut self, pkgs: Vec<String>) -> Self {
        for pkg in pkgs {
            if !self.packages.contains(&pkg) {
                self.packages.push(pkg);
            }
        }
        self
    }

    fn add_run_commands(mut self, cmds: Vec<String>) -> Self {
        self.run_commands = cmds;
        self
    }

    fn add_guest_binary(mut self) -> Self {
        self.has_guest_binary = true;
        self
    }

    fn set_shell(mut self, shell: &str) -> Self {
        self.forced_shell = Some(shell.to_string());
        self
    }

    fn build(self) -> String {
        let distro = DistroFamily::from_base_image(&self.base_image);
        let mut lines = Vec::new();

        lines.push(format!("FROM {}", self.base_image));
        lines.push(String::new());

        if !self.packages.is_empty() {
            let pkgs = self.packages.join(" ");
            let cmd = format!(
                "{} {} && {}",
                distro.install_cmd(),
                pkgs,
                distro.clean_cmd()
            );
            lines.push(format!("RUN {}", cmd));
            lines.push(String::new());
        }

        for cmd in &self.run_commands {
            lines.push(format!("RUN {}", cmd));
        }
        if !self.run_commands.is_empty() {
            lines.push(String::new());
        }

        if let Some(locale) = self
            .env_vars
            .iter()
            .find(|(k, _)| k == "LANG")
            .map(|(_, v)| v.as_str())
        {
            match distro {
                DistroFamily::DebianLike | DistroFamily::ArchLike => {
                    let (name, charset) = locale.split_once('.').unwrap_or((locale, "UTF-8"));
                    lines.push(format!(
                        "RUN localedef -i {} -f {} {} || true",
                        name, charset, locale
                    ));
                    lines.push(String::new());
                }
                DistroFamily::FedoraLike => {
                    // glibc-all-langpacks includes pre-generated locales, no localedef needed
                }
                DistroFamily::AlpineLike | DistroFamily::Unknown => {}
            }
        }

        if self.has_guest_binary {
            lines.push("COPY podbox-guest /usr/local/bin/podbox-guest".into());
            lines.push("RUN chmod +x /usr/local/bin/podbox-guest".into());
            lines.push(String::new());
        }

        for (key, value) in &self.env_vars {
            lines.push(format!("ENV {}={}", key, value));
        }

        lines.push(format!("ENV PODBOX_CONTAINER={}", self.container_name));
        lines.push(format!("ENV PODBOX_HOST_VERSION={}", VERSION));
        lines.push(String::new());

        lines.push("ENTRYPOINT [\"/usr/local/bin/podbox-guest\", \"--entry\"]".into());
        lines.push(format!("CMD [\"{}\"]", self.default_shell()));
        lines.push(String::new());

        lines.join("\n")
    }

    fn default_shell(&self) -> &str {
        if let Some(ref shell) = self.forced_shell {
            return shell;
        }
        self.packages
            .iter()
            .find_map(|p| match p.as_str() {
                "fish" => Some("fish"),
                "zsh" => Some("zsh"),
                "bash" => Some("bash"),
                _ => None,
            })
            .unwrap_or("fish")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::distros::DistroFamily;

    #[test]
    fn test_builder_debian() {
        let builder = ContainerfileBuilder::new("debian:12", "test").add_base_packages(
            DistroFamily::DebianLike,
            Some("/usr/bin/fish"),
            Some("en_US.UTF-8"),
        );
        let cf = builder.build();
        assert!(cf.contains("apt-get update"));
        assert!(cf.contains("sudo"));
        assert!(cf.contains("fish"));
        assert!(cf.contains("locales"));
        assert!(cf.contains("ENV LANG=en_US.UTF-8"));
        assert!(cf.contains("localedef -i en_US -f UTF-8 en_US.UTF-8"));
        assert!(cf.contains("ENV PODBOX_CONTAINER=test"));
    }

    #[test]
    fn test_builder_fedora() {
        let builder = ContainerfileBuilder::new("fedora:41", "test").add_base_packages(
            DistroFamily::FedoraLike,
            Some("/usr/bin/zsh"),
            None,
        );
        let cf = builder.build();
        assert!(cf.contains("dnf install -y"));
        assert!(cf.contains("sudo"));
        assert!(cf.contains("zsh"));
        assert!(cf.contains("ENV PODBOX_CONTAINER=test"));
        // Fedora uses glibc-all-langpacks (no localedef needed)
        assert!(!cf.contains("localedef"));
    }

    #[test]
    fn test_builder_arch() {
        let builder = ContainerfileBuilder::new("archlinux:latest", "test").add_base_packages(
            DistroFamily::ArchLike,
            Some("/bin/bash"),
            None,
        );
        let cf = builder.build();
        assert!(cf.contains("pacman -Syu --noconfirm"));
        assert!(cf.contains("bash"));
        assert!(cf.contains("bash-completion"));
        assert!(cf.contains("ENV PODBOX_CONTAINER=test"));
        // No locale requested, so no localedef
        assert!(!cf.contains("localedef"));
    }

    #[test]
    fn test_builder_arch_with_locale() {
        let builder = ContainerfileBuilder::new("archlinux:latest", "test").add_base_packages(
            DistroFamily::ArchLike,
            Some("/bin/bash"),
            Some("en_US.UTF-8"),
        );
        let cf = builder.build();
        assert!(cf.contains("localedef -i en_US -f UTF-8 en_US.UTF-8"));
    }

    #[test]
    fn test_builder_alpine() {
        let builder = ContainerfileBuilder::new("alpine:3.20", "test").add_base_packages(
            DistroFamily::AlpineLike,
            None,
            None,
        );
        let cf = builder.build();
        assert!(cf.contains("apk add --no-cache"));
        assert!(cf.contains("sudo"));
        assert!(cf.contains("ENV PODBOX_CONTAINER=test"));
    }
}
