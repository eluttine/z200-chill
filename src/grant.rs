use crate::ast::Program;
use crate::manifest::Manifest;
use crate::program::{self, ExportInfo, ExportInfoKind};
use crate::types::{self, Type};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

/// A compiled interface artifact for a package. It records the names the
/// package exports so another compilation unit can later resolve `SEIZE`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantFile {
    pub schema: u32,
    pub package: String,
    pub edition: String,
    pub int_widths: String,
    /// Optional absolute path to the source package. `run` uses it to link
    /// grant-file dependencies when the source is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_dir: Option<String>,
    pub exports: Vec<GrantExport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantExport {
    pub name: String,
    pub kind: GrantKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantKind {
    Variable,
    Procedure,
}

impl GrantFile {
    pub fn from_package(manifest: &Manifest, program: &Program) -> Result<Self, program::Error> {
        let exports = program::exports(program)?;
        let mut entries = Vec::with_capacity(exports.len());

        for export in exports {
            match export.kind {
                ExportInfoKind::Var(ty) => entries.push(GrantExport {
                    name: export.name,
                    kind: GrantKind::Variable,
                    mode: Some(type_name(ty)),
                    params: Vec::new(),
                    result: None,
                }),
                ExportInfoKind::Proc { params, result } => entries.push(GrantExport {
                    name: export.name,
                    kind: GrantKind::Procedure,
                    mode: None,
                    params: params.into_iter().map(type_name).collect(),
                    result: result.map(type_name),
                }),
            }
        }

        Ok(Self {
            schema: SCHEMA,
            package: manifest.package.name.clone(),
            edition: manifest.package.edition.as_str().to_owned(),
            int_widths: manifest.build.int_widths.to_string(),
            package_dir: None,
            exports: entries,
        })
    }

    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self)
            .map_err(|error| format!("cannot serialize grant file: {error}"))
    }

    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: Self =
            toml::from_str(text).map_err(|error| format!("invalid grant file: {error}"))?;
        if file.schema != SCHEMA {
            return Err(format!(
                "unsupported grant-file schema {}; expected {SCHEMA}",
                file.schema
            ));
        }
        Ok(file)
    }

    /// Convert the recorded interface into the shape `sema` uses for `SEIZE`
    /// resolution against external dependencies.
    pub fn to_export_infos(&self) -> Result<Vec<ExportInfo>, String> {
        let mut infos = Vec::with_capacity(self.exports.len());

        for export in &self.exports {
            let kind = match export.kind {
                GrantKind::Variable => {
                    let mode = export.mode.as_deref().ok_or_else(|| {
                        format!("variable `{}` has no mode in the grant file", export.name)
                    })?;
                    ExportInfoKind::Var(parse_type(mode)?)
                }
                GrantKind::Procedure => {
                    let params = export
                        .params
                        .iter()
                        .map(|mode| parse_type(mode))
                        .collect::<Result<Vec<_>, _>>()?;
                    let result = match &export.result {
                        Some(mode) => Some(parse_type(mode)?),
                        None => None,
                    };
                    ExportInfoKind::Proc { params, result }
                }
            };

            infos.push(ExportInfo {
                name: export.name.clone(),
                kind,
            });
        }

        Ok(infos)
    }
}

fn parse_type(name: &str) -> Result<Type, String> {
    types::resolve_mode(name).ok_or_else(|| format!("unsupported mode `{name}` in grant file"))
}

fn type_name(ty: Type) -> String {
    match ty {
        Type::Int(kind) => kind.name().to_owned(),
        Type::Bool => "BOOL".to_owned(),
        Type::Char => "CHAR".to_owned(),
        Type::String => "CHARS".to_owned(),
        // Expression-only class; it cannot occur in a declared signature.
        Type::IntLiteral => "INT".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    fn modules_package() -> (Manifest, Program) {
        let manifest = Manifest::parse(include_str!("../examples/modules/Chill.toml")).unwrap();
        let tokens = lexer::tokenize(include_str!("../examples/modules/src/main.ch")).unwrap();
        let program = parser::parse(&tokens).unwrap();
        (manifest, program)
    }

    #[test]
    fn builds_exports_for_modules_example() {
        let (manifest, program) = modules_package();
        let grant = GrantFile::from_package(&manifest, &program).unwrap();

        assert_eq!(grant.schema, SCHEMA);
        assert_eq!(grant.package, "modules");
        assert_eq!(grant.edition, "z200-11/99");
        assert_eq!(grant.int_widths, "16-32-64");
        assert_eq!(grant.exports.len(), 2);

        let bump = grant.exports.iter().find(|e| e.name == "Bump").unwrap();
        assert_eq!(bump.kind, GrantKind::Procedure);
        assert!(bump.params.is_empty());
        assert_eq!(bump.result, None);

        let value = grant.exports.iter().find(|e| e.name == "value").unwrap();
        assert_eq!(value.kind, GrantKind::Variable);
        assert_eq!(value.mode.as_deref(), Some("INT"));
    }

    #[test]
    fn round_trips_through_toml() {
        let (manifest, program) = modules_package();
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let text = grant.to_toml().unwrap();
        let parsed = GrantFile::from_toml(&text).unwrap();
        assert_eq!(grant, parsed);
    }

    #[test]
    fn converts_exports_for_consumption() {
        let (manifest, program) = modules_package();
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let infos = grant.to_export_infos().unwrap();

        assert_eq!(infos.len(), 2);
        let value = infos.iter().find(|info| info.name == "value").unwrap();
        assert!(matches!(value.kind, ExportInfoKind::Var(Type::Int(_))));

        let bump = infos.iter().find(|info| info.name == "Bump").unwrap();
        assert!(matches!(
            &bump.kind,
            ExportInfoKind::Proc { params, result } if params.is_empty() && result.is_none()
        ));
    }

    #[test]
    fn rejects_unknown_schema() {
        let text = r#"
schema = 999
package = "x"
edition = "z200-11/99"
int_widths = "16-32-64"
exports = []
"#;
        let error = GrantFile::from_toml(text).unwrap_err();
        assert!(error.contains("unsupported grant-file schema"));
    }
}
