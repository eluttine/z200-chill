use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone)]
pub struct Build {
    pub entry: PathBuf,
    /// `None` means auto-discover `.ch` files next to the entry.
    pub sources: Option<Vec<PathBuf>>,
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    package: RawPackage,
    #[serde(default)]
    build: RawBuild,
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default)]
    pieces: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPackage {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawBuild {
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    sources: Option<Vec<String>>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("cannot read manifest {}: {err}", path.display()))?;
        let default_name = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        Self::parse_with_default_name(&text, default_name.as_deref())
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        Self::parse_with_default_name(text, None)
    }

    fn parse_with_default_name(text: &str, default_name: Option<&str>) -> Result<Self, String> {
        let raw: RawManifest =
            toml::from_str(text).map_err(|err| format!("invalid TOML: {err}"))?;

        let name = raw
            .package
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| default_name.map(str::to_owned))
            .ok_or_else(|| "package.name must not be empty".to_owned())?;

        let entry = raw
            .build
            .entry
            .as_deref()
            .map(str::trim)
            .unwrap_or("src/main.ch");
        if entry.is_empty() {
            return Err("build.entry must not be empty".to_owned());
        }

        let sources = match raw.build.sources {
            None => None,
            Some(sources) => {
                let mut paths = Vec::with_capacity(sources.len());
                for source in sources {
                    let source = source.trim();
                    if source.is_empty() {
                        return Err("build.sources entries must not be empty".to_owned());
                    }
                    paths.push(PathBuf::from(source));
                }
                Some(paths)
            }
        };

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
            package: Package { name },
            build: Build {
                entry: PathBuf::from(entry),
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

[build]
entry = "src/main.ch"
"#;

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
            Some(vec![
                PathBuf::from("src/library.ch"),
                PathBuf::from("src/extra.ch")
            ])
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
