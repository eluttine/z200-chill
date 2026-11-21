use serde::Deserialize;
use std::collections::HashMap;
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

impl fmt::Display for WidthProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.short, self.int, self.long)
    }
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub package: Package,
    pub build: Build,
    pub dependencies: HashMap<String, PathBuf>,
    /// Text-reference names for remote piece designators.
    pub pieces: HashMap<String, PathBuf>,
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
    pub sources: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    package: RawPackage,
    build: RawBuild,
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default)]
    pieces: HashMap<String, String>,
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
    sources: Vec<String>,
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

        let mut sources = Vec::with_capacity(raw.build.sources.len());
        for source in raw.build.sources {
            let source = source.trim();
            if source.is_empty() {
                return Err("build.sources entries must not be empty".to_owned());
            }
            sources.push(PathBuf::from(source));
        }

        let mut dependencies = HashMap::new();
        for (name, path) in raw.dependencies {
            let name = name.trim();
            let path = path.trim();
            if name.is_empty() {
                return Err("dependency name must not be empty".to_owned());
            }
            if path.is_empty() {
                return Err(format!("dependency `{name}` must have a grant-file path"));
            }
            dependencies.insert(name.to_owned(), PathBuf::from(path));
        }

        let mut pieces = HashMap::new();
        for (name, path) in raw.pieces {
            let name = name.trim();
            let path = path.trim();
            if name.is_empty() {
                return Err("piece text-reference name must not be empty".to_owned());
            }
            if path.is_empty() {
                return Err(format!("piece `{name}` must have a path"));
            }
            pieces.insert(name.to_owned(), PathBuf::from(path));
        }

        Ok(Self {
            package: Package {
                name: raw.package.name.trim().to_owned(),
                version: raw.package.version.trim().to_owned(),
                edition,
            },
            build: Build {
                entry: PathBuf::from(raw.build.entry.trim()),
                sources,
            },
            dependencies,
            pieces,
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
"#;

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

    #[test]
    fn parses_sources() {
        let text = MANIFEST.replace(
            "entry = \"src/main.ch\"",
            "entry = \"src/main.ch\"\nsources = [\"src/library.ch\", \"src/extra.ch\"]",
        );
        let manifest = Manifest::parse(&text).unwrap();
        assert_eq!(
            manifest.build.sources,
            vec![
                PathBuf::from("src/library.ch"),
                PathBuf::from("src/extra.ch")
            ]
        );
    }

    #[test]
    fn rejects_empty_source() {
        let text = MANIFEST.replace(
            "entry = \"src/main.ch\"",
            "entry = \"src/main.ch\"\nsources = [\"\"]",
        );
        let err = Manifest::parse(&text).unwrap_err();
        assert!(err.contains("build.sources entries must not be empty"));
    }

    #[test]
    fn parses_dependencies() {
        let text = format!("{MANIFEST}\n[dependencies]\nlib = \"../lib/target/lib.grt.toml\"\n");
        let manifest = Manifest::parse(&text).unwrap();
        assert_eq!(
            manifest.dependencies.get("lib"),
            Some(&PathBuf::from("../lib/target/lib.grt.toml"))
        );
    }

    #[test]
    fn rejects_empty_dependency_path() {
        let text = format!("{MANIFEST}\n[dependencies]\nlib = \"\"\n");
        let err = Manifest::parse(&text).unwrap_err();
        assert!(err.contains("must have a grant-file path"));
    }
}
