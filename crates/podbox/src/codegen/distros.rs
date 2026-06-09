#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistroFamily {
    DebianLike,
    FedoraLike,
    ArchLike,
    AlpineLike,
    Unknown,
}

impl DistroFamily {
    pub fn from_base_image(base: &str) -> Self {
        let base_lower = base.to_lowercase();
        if base_lower.contains("debian")
            || base_lower.contains("ubuntu")
            || base_lower.contains("mint")
            || base_lower.contains("kali")
            || base_lower.contains("pop")
            || base_lower.contains("elementary")
        {
            Self::DebianLike
        } else if base_lower.contains("fedora")
            || base_lower.contains("rhel")
            || base_lower.contains("centos")
            || base_lower.contains("rocky")
            || base_lower.contains("alma")
            || base_lower.contains("nobara")
        {
            Self::FedoraLike
        } else if base_lower.contains("arch")
            || base_lower.contains("cachy")
            || base_lower.contains("manjaro")
            || base_lower.contains("endeavouros")
            || base_lower.contains("garuda")
        {
            Self::ArchLike
        } else if base_lower.contains("alpine") {
            Self::AlpineLike
        } else {
            Self::Unknown
        }
    }

    pub fn manager(&self) -> &'static str {
        match self {
            Self::DebianLike => "apt",
            Self::FedoraLike => "dnf",
            Self::ArchLike => "pacman",
            Self::AlpineLike => "apk",
            Self::Unknown => "dnf",
        }
    }

    pub fn install_cmd(&self) -> &'static str {
        match self {
            Self::DebianLike => "apt-get update && apt-get install -y --no-install-recommends",
            Self::FedoraLike => "dnf install -y",
            Self::ArchLike => "pacman -Syu --noconfirm",
            Self::AlpineLike => "apk add --no-cache",
            Self::Unknown => "dnf install -y",
        }
    }

    pub fn clean_cmd(&self) -> &'static str {
        match self {
            Self::DebianLike => "rm -rf /var/lib/apt/lists/*",
            Self::FedoraLike => "dnf clean all",
            Self::ArchLike => "pacman -Scc --noconfirm",
            Self::AlpineLike => "",
            Self::Unknown => "dnf clean all",
        }
    }

    pub fn base_packages(&self, host_shell: Option<&str>) -> Vec<String> {
        let mut pkgs = match self {
            Self::DebianLike => vec![
                "sudo".into(),
                "locales".into(),
                "curl".into(),
                "tar".into(),
                "unzip".into(),
                "wget".into(),
                "which".into(),
                "coreutils".into(),
                "diffutils".into(),
                "findutils".into(),
                "grep".into(),
                "sed".into(),
                "gawk".into(),
                "bash-completion".into(),
            ],
            Self::FedoraLike => vec![
                "sudo".into(),
                "curl".into(),
                "tar".into(),
                "unzip".into(),
                "wget".into(),
                "which".into(),
                "coreutils".into(),
                "diffutils".into(),
                "findutils".into(),
                "grep".into(),
                "sed".into(),
                "gawk".into(),
                "bash-completion".into(),
            ],
            Self::ArchLike => vec![
                "sudo".into(),
                "curl".into(),
                "tar".into(),
                "unzip".into(),
                "wget".into(),
                "which".into(),
                "coreutils".into(),
                "diffutils".into(),
                "findutils".into(),
                "grep".into(),
                "sed".into(),
                "gawk".into(),
                "bash-completion".into(),
            ],
            Self::AlpineLike => vec![
                "sudo".into(),
                "curl".into(),
                "tar".into(),
                "unzip".into(),
                "wget".into(),
                "which".into(),
                "coreutils".into(),
                "diffutils".into(),
                "findutils".into(),
                "grep".into(),
                "sed".into(),
                "gawk".into(),
                "bash-completion".into(),
            ],
            Self::Unknown => vec![
                "sudo".into(),
                "curl".into(),
                "tar".into(),
                "unzip".into(),
                "wget".into(),
                "which".into(),
                "coreutils".into(),
                "diffutils".into(),
                "findutils".into(),
                "grep".into(),
                "sed".into(),
                "gawk".into(),
                "bash-completion".into(),
            ],
        };

        if let Some(shell) = host_shell {
            let shell_pkgs = Self::shell_packages(self, shell);
            for pkg in shell_pkgs {
                if !pkgs.contains(&pkg) {
                    pkgs.push(pkg);
                }
            }
        }

        pkgs
    }

    fn shell_packages(&self, shell_path: &str) -> Vec<String> {
        let shell_name = shell_path.split('/').next_back().unwrap_or("");
        let mut pkgs = Vec::new();

        match shell_name {
            "bash" => {
                pkgs.push("bash".into());
                pkgs.push("bash-completion".into());
            }
            "zsh" => {
                pkgs.push("zsh".into());
                match self {
                    Self::DebianLike => pkgs.push("zsh-common".into()),
                    Self::FedoraLike => pkgs.push("zsh".into()),
                    Self::ArchLike => pkgs.push("zsh-completions".into()),
                    Self::AlpineLike => pkgs.push("zsh".into()),
                    Self::Unknown => pkgs.push("zsh".into()),
                }
            }
            "fish" => {
                pkgs.push("fish".into());
            }
            "sh" | "dash" => match self {
                Self::DebianLike => pkgs.push("dash".into()),
                Self::FedoraLike => pkgs.push("dash".into()),
                Self::ArchLike => pkgs.push("dash".into()),
                Self::AlpineLike => pkgs.push("dash".into()),
                Self::Unknown => pkgs.push("dash".into()),
            },
            _ => {}
        }

        pkgs
    }

    pub fn remove_cmd(&self, packages: &[String]) -> String {
        if packages.is_empty() {
            return String::new();
        }
        let pkgs = packages.join(" ");
        match self {
            Self::DebianLike => {
                format!(
                    "apt-get purge -y {} && apt-get autoremove -y && {}",
                    pkgs,
                    Self::clean_cmd(self)
                )
            }
            Self::FedoraLike => {
                format!("dnf remove -y {} && {}", pkgs, Self::clean_cmd(self))
            }
            Self::ArchLike => {
                format!(
                    "pacman -Rns --noconfirm {} && {}",
                    pkgs,
                    Self::clean_cmd(self)
                )
            }
            Self::AlpineLike => {
                format!("apk del {} && {}", pkgs, Self::clean_cmd(self))
            }
            Self::Unknown => {
                format!("dnf remove -y {} && {}", pkgs, Self::clean_cmd(self))
            }
        }
    }

    pub fn locale_packages(&self) -> Vec<String> {
        match self {
            Self::DebianLike => vec!["locales".into()],
            Self::FedoraLike => vec!["glibc-all-langpacks".into()],
            Self::ArchLike => vec!["glibc".into()],
            Self::AlpineLike => vec!["musl-locales".into()],
            Self::Unknown => vec!["locales".into()],
        }
    }
}

pub fn detect_host_shell() -> Option<String> {
    std::env::var("SHELL").ok().filter(|s| !s.is_empty())
}

pub fn detect_host_locale() -> Option<String> {
    std::env::var("LANG")
        .ok()
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LC_CTYPE").ok())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distro_from_base_image() {
        assert_eq!(
            DistroFamily::from_base_image("debian:12"),
            DistroFamily::DebianLike
        );
        assert_eq!(
            DistroFamily::from_base_image("ubuntu:24.04"),
            DistroFamily::DebianLike
        );
        assert_eq!(
            DistroFamily::from_base_image("fedora:41"),
            DistroFamily::FedoraLike
        );
        assert_eq!(
            DistroFamily::from_base_image("cachy-latest"),
            DistroFamily::ArchLike
        );
        assert_eq!(
            DistroFamily::from_base_image("archlinux:latest"),
            DistroFamily::ArchLike
        );
        assert_eq!(
            DistroFamily::from_base_image("alpine:3.20"),
            DistroFamily::AlpineLike
        );
        assert_eq!(
            DistroFamily::from_base_image("unknown:latest"),
            DistroFamily::Unknown
        );
    }

    #[test]
    fn test_debian_base_packages() {
        let pkgs = DistroFamily::DebianLike.base_packages(Some("/usr/bin/fish"));
        assert!(pkgs.contains(&"sudo".into()));
        assert!(pkgs.contains(&"locales".into()));
        assert!(pkgs.contains(&"curl".into()));
        assert!(pkgs.contains(&"fish".into()));
    }

    #[test]
    fn test_fedora_base_packages() {
        let pkgs = DistroFamily::FedoraLike.base_packages(Some("/usr/bin/zsh"));
        assert!(pkgs.contains(&"sudo".into()));
        assert!(pkgs.contains(&"zsh".into()));
        assert!(pkgs.contains(&"zsh-completions".into()) || pkgs.contains(&"zsh".into()));
    }

    #[test]
    fn test_arch_base_packages() {
        let pkgs = DistroFamily::ArchLike.base_packages(Some("/bin/bash"));
        assert!(pkgs.contains(&"sudo".into()));
        assert!(pkgs.contains(&"bash".into()));
        assert!(pkgs.contains(&"bash-completion".into()));
    }

    #[test]
    fn test_alpine_base_packages() {
        let pkgs = DistroFamily::AlpineLike.base_packages(None);
        assert!(pkgs.contains(&"sudo".into()));
        assert!(pkgs.contains(&"curl".into()));
    }

    #[test]
    fn test_install_cmd() {
        assert_eq!(
            DistroFamily::DebianLike.install_cmd(),
            "apt-get update && apt-get install -y --no-install-recommends"
        );
        assert_eq!(DistroFamily::FedoraLike.install_cmd(), "dnf install -y");
        assert_eq!(
            DistroFamily::ArchLike.install_cmd(),
            "pacman -Syu --noconfirm"
        );
        assert_eq!(DistroFamily::AlpineLike.install_cmd(), "apk add --no-cache");
    }
}
