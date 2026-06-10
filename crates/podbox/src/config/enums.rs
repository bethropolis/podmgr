use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    Build { base: String },
    Prebuilt { ref_str: String },
}

impl ImageSource {
    pub fn is_prebuilt(&self) -> bool {
        matches!(self, Self::Prebuilt { .. })
    }

    pub fn is_build(&self) -> bool {
        matches!(self, Self::Build { .. })
    }
}

/// Type-safe package manager identifier.
///
/// Replaces raw `&str` dispatch everywhere.  Always use the enum rather
/// than hard-coding strings like `"dnf"` / `"apt"`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    #[default]
    Dnf,
    Apt,
    Pacman,
    Apk,
    Zypper,
}

impl PackageManager {
    /// Canonical display name (e.g. `"dnf"`, `"apt"`).
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Dnf => "dnf",
            Self::Apt => "apt",
            Self::Pacman => "pacman",
            Self::Apk => "apk",
            Self::Zypper => "zypper",
        }
    }

    /// Parse a string into a `PackageManager`, returning `None` for unknown
    /// values (callers fall back to distro detection).
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "dnf" => Some(Self::Dnf),
            "apt" => Some(Self::Apt),
            "pacman" => Some(Self::Pacman),
            "apk" => Some(Self::Apk),
            "zypper" => Some(Self::Zypper),
            _ => None,
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for PackageManager {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_opt(s.trim()).ok_or_else(|| format!("unknown package manager: {s}"))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum GpuMode {
    #[default]
    Auto,
    Enabled,
    Disabled,
    Nvidia,
}

impl<'de> Deserialize<'de> for GpuMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct GpuModeVisitor;

        impl<'de> de::Visitor<'de> for GpuModeVisitor {
            type Value = GpuMode;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("true, false, \"auto\", or \"nvidia\"")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<GpuMode, E> {
                Ok(if v {
                    GpuMode::Enabled
                } else {
                    GpuMode::Disabled
                })
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<GpuMode, E> {
                match v {
                    "auto" => Ok(GpuMode::Auto),
                    "nvidia" => Ok(GpuMode::Nvidia),
                    "true" => Ok(GpuMode::Enabled),
                    "false" => Ok(GpuMode::Disabled),
                    _ => Err(de::Error::unknown_variant(
                        v,
                        &["auto", "nvidia", "true", "false"],
                    )),
                }
            }
        }

        deserializer.deserialize_any(GpuModeVisitor)
    }
}

impl Serialize for GpuMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            GpuMode::Auto => serializer.serialize_str("auto"),
            GpuMode::Enabled => serializer.serialize_bool(true),
            GpuMode::Disabled => serializer.serialize_bool(false),
            GpuMode::Nvidia => serializer.serialize_str("nvidia"),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OnStop {
    #[default]
    Keep,
    Remove,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum XdgDirValue {
    Simple(bool),
    Detailed {
        enabled: bool,
        #[serde(default)]
        read_write: bool,
    },
}

impl Default for XdgDirValue {
    fn default() -> Self {
        XdgDirValue::Simple(false)
    }
}

impl XdgDirValue {
    pub fn is_enabled(&self) -> bool {
        match self {
            XdgDirValue::Simple(b) => *b,
            XdgDirValue::Detailed { enabled, .. } => *enabled,
        }
    }

    pub fn is_read_write(&self) -> bool {
        match self {
            XdgDirValue::Simple(_) => false,
            XdgDirValue::Detailed { read_write, .. } => *read_write,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    use super::*;

    #[test]
    fn test_gpu_mode_parses_true() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = true
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Enabled);
    }

    #[test]
    fn test_gpu_mode_parses_false() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = false
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Disabled);
    }

    #[test]
    fn test_gpu_mode_parses_auto_string() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = "auto"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Auto);
    }

    #[test]
    fn test_gpu_mode_parses_nvidia_string() {
        let toml = r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
gpu = "nvidia"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.integration.gpu, GpuMode::Nvidia);
    }

    #[test]
    fn test_gpu_mode_serialize() {
        assert_eq!(serde_json::to_string(&GpuMode::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&GpuMode::Enabled).unwrap(), "true");
        assert_eq!(serde_json::to_string(&GpuMode::Disabled).unwrap(), "false");
        assert_eq!(
            serde_json::to_string(&GpuMode::Nvidia).unwrap(),
            "\"nvidia\""
        );
        #[derive(Serialize)]
        struct Wrapper {
            gpu: GpuMode,
        }
        let wrapper = Wrapper {
            gpu: GpuMode::Nvidia,
        };
        let toml_out = toml::to_string(&wrapper).unwrap();
        assert!(toml_out.contains("gpu = \"nvidia\""));
        let wrapper = Wrapper {
            gpu: GpuMode::Enabled,
        };
        let toml_out = toml::to_string(&wrapper).unwrap();
        assert!(toml_out.contains("gpu = true"));
    }

    #[test]
    fn test_gpu_mode_deserialize_toml_key() {
        let cases = [
            ("gpu = true", GpuMode::Enabled),
            ("gpu = false", GpuMode::Disabled),
            ("gpu = \"auto\"", GpuMode::Auto),
            ("gpu = \"nvidia\"", GpuMode::Nvidia),
        ];
        for (toml_snippet, expected) in &cases {
            let full = format!(
                r#"
[image]
base = "fedora:41"
name = "env"
[container]
name = "env"
home = "~/env"
[integration]
{}
"#,
                toml_snippet
            );
            let cfg: Config = toml::from_str(&full).unwrap();
            assert_eq!(cfg.integration.gpu, *expected);
        }
    }
}
