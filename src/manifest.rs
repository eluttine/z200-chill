use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The Z.200 edition a package targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edition {
    V1988,
    V1999,
}

impl Edition {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "z200-11/99" | "z200-1999" => Ok(Self::V1999),
            "z200-1988" => Ok(Self::V1988),
            other => Err(format!(
                "unsupported edition `{other}`; expected `z200-11/99` (or `z200-1988`)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1988 => "z200-1988",
            Self::V1999 => "z200-11/99",
        }
    }
}

impl fmt::Display for Edition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Integer widths for `SHORT_INT`, `INT`, and `LONG_INT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidthProfile {
    pub short: u16,
    pub int: u16,
    pub long: u16,
}

impl Default for WidthProfile {
    fn default() -> Self {
        Self {
            short: 16,
            int: 32,
            long: 64,
        }
    }
}

impl WidthProfile {
    /// Parse `INT-LONG` (legacy) or `SHORT-INT-LONG`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split('-').map(str::trim).collect();
        let mut widths = Vec::with_capacity(parts.len());

        for part in parts {
            if part.is_empty() {
                return Err(format!("invalid int_widths `{text}`: empty width"));
            }
            let width: u16 = part.parse().map_err(|_| {
                format!("invalid int_widths `{text}`: `{part}` is not an unsigned integer")
            })?;
            if width == 0 {
                return Err(format!(
                    "invalid int_widths `{text}`: widths must be positive"
                ));
            }
            widths.push(width);
        }

        let profile = match widths.as_slice() {
            [int, long] => Self {
                short: *int,
                int: *int,
                long: *long,
            },
            [short, int, long] => Self {
                short: *short,
                int: *int,
                long: *long,
            },
            _ => {
                return Err(format!(
                    "invalid int_widths `{text}`: expected `INT-LONG` or `SHORT-INT-LONG`"
                ));
            }
        };

        if !(profile.short <= profile.int && profile.int <= profile.long) {
            return Err(format!(
                "invalid int_widths `{text}`: require width(SHORT_INT) <= width(INT) <= width(LONG_INT)"
            ));
        }

        Ok(profile)
    }
}

impl fmt::Display for WidthProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.short, self.int, self.long)
    }
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub package: Package,
    pub build: Build,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: Edition,
}

#[derive(Debug, Clone)]
pub struct Build {
    pub entry: PathBuf,
    pub int_widths: WidthProfile,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    package: RawPackage,
    build: RawBuild,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    edition: String,
}

#[derive(Debug, Deserialize)]
struct RawBuild {
    entry: String,
    #[serde(default)]
    int_widths: Option<String>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("cannot read manifest {}: {err}", path.display()))?;
        Self::parse(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawManifest =
            toml::from_str(text).map_err(|err| format!("invalid TOML: {err}"))?;

        if raw.package.name.trim().is_empty() {
            return Err("package.name must not be empty".to_owned());
        }
        if raw.package.version.trim().is_empty() {
            return Err("package.version must not be empty".to_owned());
        }
        if raw.build.entry.trim().is_empty() {
            return Err("build.entry must not be empty".to_owned());
        }

        let edition = Edition::parse(raw.package.edition.trim())?;
        let int_widths = match raw.build.int_widths.as_deref() {
            Some(text) => WidthProfile::parse(text)?,
            None => WidthProfile::default(),
        };

        Ok(Self {
            package: Package {
                name: raw.package.name.trim().to_owned(),
                version: raw.package.version.trim().to_owned(),
                edition,
            },
            build: Build {
                entry: PathBuf::from(raw.build.entry.trim()),
                int_widths,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
[package]
name = "hello"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#;

    #[test]
    fn parses_three_widths() {
        let manifest = Manifest::parse(MANIFEST).unwrap();
        assert_eq!(manifest.package.name, "hello");
        assert_eq!(manifest.package.edition, Edition::V1999);
        assert_eq!(
            manifest.build.int_widths,
            WidthProfile {
                short: 16,
                int: 32,
                long: 64
            }
        );
        assert_eq!(manifest.build.entry, PathBuf::from("src/main.ch"));
    }

    #[test]
    fn legacy_two_widths_reuse_int_for_short() {
        let text = MANIFEST.replace("16-32-64", "32-64");
        let manifest = Manifest::parse(&text).unwrap();
        assert_eq!(
            manifest.build.int_widths,
            WidthProfile {
                short: 32,
                int: 32,
                long: 64
            }
        );
    }

    #[test]
    fn widths_must_be_ordered() {
        let text = MANIFEST.replace("16-32-64", "64-32-16");
        let err = Manifest::parse(&text).unwrap_err();
        assert!(err.contains("width(SHORT_INT) <= width(INT) <= width(LONG_INT)"));
    }

    #[test]
    fn missing_widths_uses_default() {
        let text = r#"
[package]
name = "hello"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
"#;
        let manifest = Manifest::parse(text).unwrap();
        assert_eq!(manifest.build.int_widths, WidthProfile::default());
    }

    #[test]
    fn rejects_unknown_edition() {
        let text = MANIFEST.replace("z200-11/99", "z200-3000");
        let err = Manifest::parse(&text).unwrap_err();
        assert!(err.contains("unsupported edition"));
    }

    #[test]
    fn rejects_empty_entry() {
        let text = MANIFEST.replace("src/main.ch", "");
        let err = Manifest::parse(&text).unwrap_err();
        assert!(err.contains("build.entry"));
    }
}
