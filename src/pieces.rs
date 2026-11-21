use crate::ast::{PieceDesignator, PieceDesignatorKind};
use crate::manifest::Manifest;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

/// Resolves Z.200 piece designators to package-local source files.
///
/// Z.200 leaves designator lookup implementation defined. This resolver uses:
/// - literal: a package-relative path;
/// - text reference: the manifest `[pieces]` table;
/// - empty: `pieces/<unit-name>.ch`.
pub struct PieceResolver<'a> {
    package_dir: &'a Path,
    pieces: &'a HashMap<String, PathBuf>,
}

impl<'a> PieceResolver<'a> {
    pub fn new(package_dir: &'a Path, pieces: &'a HashMap<String, PathBuf>) -> Self {
        Self {
            package_dir,
            pieces,
        }
    }

    pub fn from_manifest(package_dir: &'a Path, manifest: &'a Manifest) -> Self {
        Self::new(package_dir, &manifest.pieces)
    }

    /// Resolve a designator without checking whether the file exists.
    pub fn resolve(
        &self,
        designator: &PieceDesignator,
        unit_name: Option<&str>,
        _containing_file: &Path,
    ) -> Result<PathBuf, String> {
        match &designator.kind {
            PieceDesignatorKind::Literal(path) => {
                self.package_relative(Path::new(path), "piece designator")
            }
            PieceDesignatorKind::TextRef(name) => {
                let path = self
                    .pieces
                    .get(name)
                    .ok_or_else(|| format!("unknown piece text reference `{name}`"))?;
                self.package_relative(path, &format!("piece `{name}`"))
            }
            PieceDesignatorKind::Empty => {
                let name = unit_name.ok_or_else(|| {
                    "an empty piece designator requires a named program unit".to_owned()
                })?;
                Ok(self.package_dir.join("pieces").join(format!("{name}.ch")))
            }
        }
    }

    /// Resolve and require the piece to exist as a file.
    pub fn resolve_existing(
        &self,
        designator: &PieceDesignator,
        unit_name: Option<&str>,
        containing_file: &Path,
    ) -> Result<PathBuf, String> {
        let path = self.resolve(designator, unit_name, containing_file)?;
        if !path.is_file() {
            return Err(format!("piece file not found: {}", path.display()));
        }
        Ok(path)
    }

    /// Resolve, require existence, and enter the piece into `visiting`.
    ///
    /// The caller should remove the returned canonical path from `visiting` once the
    /// piece has been expanded. Re-entering an active piece is a cycle.
    pub fn resolve_checked(
        &self,
        designator: &PieceDesignator,
        unit_name: Option<&str>,
        containing_file: &Path,
        visiting: &mut HashSet<PathBuf>,
    ) -> Result<PathBuf, String> {
        let path = self.resolve_existing(designator, unit_name, containing_file)?;
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        if !visiting.insert(canonical.clone()) {
            return Err(format!("cycle involving piece {}", canonical.display()));
        }
        Ok(canonical)
    }

    fn package_relative(&self, path: &Path, what: &str) -> Result<PathBuf, String> {
        if path.is_absolute() {
            return Err(format!("{what} must be relative to the package directory"));
        }
        let mut depth = 0usize;
        for component in path.components() {
            match component {
                Component::Normal(_) => depth += 1,
                Component::CurDir => {}
                Component::ParentDir => {
                    if depth == 0 {
                        return Err(format!("{what} escapes the package directory"));
                    }
                    depth -= 1;
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(format!("{what} must be relative to the package directory"));
                }
            }
        }
        Ok(self.package_dir.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::PieceDesignatorKind;
    use crate::lexer::{SourceId, Span};
    use std::fs;

    fn designator(kind: PieceDesignatorKind) -> PieceDesignator {
        PieceDesignator {
            kind,
            span: Span::new(SourceId(0), 0, 0),
        }
    }

    fn pieces() -> HashMap<String, PathBuf> {
        let mut pieces = HashMap::new();
        pieces.insert("counter".to_owned(), PathBuf::from("pieces/counter.ch"));
        pieces
    }

    #[test]
    fn resolves_literal_text_reference_and_empty() {
        let dir = Path::new("/pkg");
        let pieces = pieces();
        let resolver = PieceResolver::new(dir, &pieces);
        let containing = Path::new("/pkg/src/main.ch");

        assert_eq!(
            resolver
                .resolve(
                    &designator(PieceDesignatorKind::Literal("pieces/a.ch".to_owned())),
                    None,
                    containing,
                )
                .unwrap(),
            PathBuf::from("/pkg/pieces/a.ch")
        );
        assert_eq!(
            resolver
                .resolve(
                    &designator(PieceDesignatorKind::TextRef("counter".to_owned())),
                    None,
                    containing,
                )
                .unwrap(),
            PathBuf::from("/pkg/pieces/counter.ch")
        );
        assert_eq!(
            resolver
                .resolve(
                    &designator(PieceDesignatorKind::Empty),
                    Some("counter"),
                    containing,
                )
                .unwrap(),
            PathBuf::from("/pkg/pieces/counter.ch")
        );
    }

    #[test]
    fn rejects_missing_text_reference_and_unnamed_empty() {
        let dir = Path::new("/pkg");
        let pieces = pieces();
        let resolver = PieceResolver::new(dir, &pieces);
        let containing = Path::new("/pkg/src/main.ch");

        let error = resolver
            .resolve(
                &designator(PieceDesignatorKind::TextRef("missing".to_owned())),
                None,
                containing,
            )
            .unwrap_err();
        assert!(error.contains("unknown piece text reference"));

        let error = resolver
            .resolve(&designator(PieceDesignatorKind::Empty), None, containing)
            .unwrap_err();
        assert!(error.contains("named program unit"));
    }

    #[test]
    fn rejects_absolute_and_escaping_paths() {
        let dir = Path::new("/pkg");
        let pieces = pieces();
        let resolver = PieceResolver::new(dir, &pieces);
        let containing = Path::new("/pkg/src/main.ch");

        for path in ["/tmp/a.ch", "../outside.ch", "pieces/../../outside.ch"] {
            let error = resolver
                .resolve(
                    &designator(PieceDesignatorKind::Literal(path.to_owned())),
                    None,
                    containing,
                )
                .unwrap_err();
            assert!(
                error.contains("relative") || error.contains("escapes"),
                "{path}: {error}"
            );
        }
    }

    #[test]
    fn detects_piece_cycles() {
        let dir = std::env::temp_dir().join(format!("z200-pieces-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("pieces")).unwrap();
        let piece = dir.join("pieces/a.ch");
        fs::write(&piece, "").unwrap();

        let pieces = pieces();
        let resolver = PieceResolver::new(&dir, &pieces);
        let containing = dir.join("src/main.ch");
        let mut visiting = HashSet::new();

        let designator = designator(PieceDesignatorKind::Literal("pieces/a.ch".to_owned()));
        resolver
            .resolve_checked(&designator, None, &containing, &mut visiting)
            .unwrap();
        let error = resolver
            .resolve_checked(&designator, None, &containing, &mut visiting)
            .unwrap_err();
        assert!(error.contains("cycle"), "{error}");

        fs::remove_dir_all(&dir).unwrap();
    }
}
