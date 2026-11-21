use crate::ast::Program;
use crate::manifest::Manifest;
use crate::program::{self, ExportInfo, ExportInfoKind};
use crate::types::{self, Type};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 2;

/// A compiled interface artifact for a package. It records the names the
/// package exports so another compilation unit can later resolve `SEIZE`.
///
/// This format is specific to `z200c`; Z.200 does not define an interchange
/// format. Structure modes are referenced symbolically (`package!Module!Mode`)
/// so local `ModeId`s never leak into the artifact.
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<GrantMode>,
    pub exports: Vec<GrantExport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantMode {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub fields: Vec<GrantField>,
}

impl GrantMode {
    pub fn symbol(&self) -> String {
        crate::modes::mode_symbol(self.package.as_deref(), self.module.as_deref(), &self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantField {
    pub name: String,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantExport {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    pub kind: GrantKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbid: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub friends: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantKind {
    Variable,
    Procedure,
    Mode,
}

impl GrantFile {
    pub fn from_package(manifest: &Manifest, program: &Program) -> Result<Self, program::Error> {
        let info = program::analyze(program)?;
        let mut modes: Vec<GrantMode> = info
            .modes
            .modes
            .iter()
            .filter(|mode| mode.module_name.is_some())
            .map(|mode| GrantMode {
                name: mode.name.clone(),
                module: mode.module_name.clone(),
                package: mode.package_name.clone(),
                fields: mode
                    .fields
                    .iter()
                    .map(|field| GrantField {
                        name: field.name.clone(),
                        mode: external_type_name(&program::portable_type(field.ty, &info.modes)),
                    })
                    .collect(),
            })
            .collect();
        modes.sort_by_key(GrantMode::symbol);

        let exports = program::exports(program)?;
        let mut entries = Vec::with_capacity(exports.len());
        for export in exports {
            let ExportInfo {
                name,
                module_name,
                package_name,
                prefix,
                kind,
                friends,
            } = export;
            let prefix = if prefix.is_empty() {
                None
            } else {
                Some(prefix.join("!"))
            };
            let friends = friends.unwrap_or_default();
            match kind {
                ExportInfoKind::Var(ty) => entries.push(GrantExport {
                    name,
                    module: module_name,
                    package: package_name,
                    prefix,
                    kind: GrantKind::Variable,
                    mode: Some(external_type_name(&ty)),
                    params: Vec::new(),
                    result: None,
                    forbid: Vec::new(),
                    friends,
                }),
                ExportInfoKind::Proc { params, result } => entries.push(GrantExport {
                    name,
                    module: module_name,
                    package: package_name,
                    prefix,
                    kind: GrantKind::Procedure,
                    mode: None,
                    params: params.iter().map(external_type_name).collect(),
                    result: result.as_ref().map(external_type_name),
                    forbid: Vec::new(),
                    friends,
                }),
                ExportInfoKind::Mode(mode) => entries.push(GrantExport {
                    name,
                    module: module_name,
                    package: package_name,
                    prefix,
                    kind: GrantKind::Mode,
                    mode: Some(mode.symbol()),
                    params: Vec::new(),
                    result: None,
                    forbid: mode.forbidden.clone(),
                    friends,
                }),
            }
        }

        Ok(Self {
            schema: SCHEMA,
            package: manifest.package.name.clone(),
            edition: manifest.package.edition.as_str().to_owned(),
            int_widths: manifest.build.int_widths.to_string(),
            package_dir: None,
            modes,
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
    pub fn to_interface(&self) -> Result<program::ExternalInterface, String> {
        let definitions: std::collections::HashMap<String, &GrantMode> = self
            .modes
            .iter()
            .map(|mode| (mode.symbol(), mode))
            .collect();

        let mut modes = Vec::with_capacity(self.modes.len());
        let mut mode_indices = std::collections::HashMap::new();
        for mode in &self.modes {
            let fields = mode
                .fields
                .iter()
                .map(|field| {
                    Ok((
                        field.name.clone(),
                        parse_external_type(&field.mode, &definitions)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            mode_indices.insert(mode.symbol(), modes.len());
            modes.push(program::ModeInfo {
                name: mode.name.clone(),
                module_name: mode.module.clone(),
                package_name: mode.package.clone(),
                fields,
                forbidden: Vec::new(),
            });
        }

        let mut exports = Vec::with_capacity(self.exports.len());
        for export in &self.exports {
            let friends = if export.friends.is_empty() {
                None
            } else {
                Some(export.friends.clone())
            };
            let kind = match export.kind {
                GrantKind::Variable => {
                    let mode = export.mode.as_deref().ok_or_else(|| {
                        format!("variable `{}` has no mode in the grant file", export.name)
                    })?;
                    ExportInfoKind::Var(parse_external_type(mode, &definitions)?)
                }
                GrantKind::Procedure => {
                    let params = export
                        .params
                        .iter()
                        .map(|mode| parse_external_type(mode, &definitions))
                        .collect::<Result<Vec<_>, _>>()?;
                    let result = match &export.result {
                        Some(mode) => Some(parse_external_type(mode, &definitions)?),
                        None => None,
                    };
                    ExportInfoKind::Proc { params, result }
                }
                GrantKind::Mode => {
                    let symbol = export.mode.as_deref().ok_or_else(|| {
                        format!("mode export `{}` has no mode reference", export.name)
                    })?;
                    let index = mode_indices.get(symbol).copied().ok_or_else(|| {
                        format!("mode export `{symbol}` has no definition in the grant file")
                    })?;
                    modes[index].forbidden = export.forbid.clone();
                    ExportInfoKind::Mode(modes[index].clone())
                }
            };

            exports.push(ExportInfo {
                name: export.name.clone(),
                module_name: export.module.clone(),
                package_name: export.package.clone(),
                prefix: export
                    .prefix
                    .as_deref()
                    .map(|prefix| prefix.split('!').map(str::to_owned).collect())
                    .unwrap_or_default(),
                kind,
                friends,
            });
        }

        Ok(program::ExternalInterface { modes, exports })
    }
}

fn parse_external_type(
    name: &str,
    definitions: &std::collections::HashMap<String, &GrantMode>,
) -> Result<program::ExternalType, String> {
    if let Some(ty) = types::resolve_mode(name) {
        return Ok(program::ExternalType::Builtin(ty));
    }
    if name.contains('!') {
        if definitions.contains_key(name) {
            return Ok(program::ExternalType::Mode(name.to_owned()));
        }
        return Err(format!(
            "grant file references mode `{name}` but does not define it"
        ));
    }
    Err(format!("unsupported mode `{name}` in grant file"))
}

fn external_type_name(ty: &program::ExternalType) -> String {
    match ty {
        program::ExternalType::Builtin(ty) => type_name(*ty),
        program::ExternalType::Mode(symbol) => symbol.clone(),
    }
}

fn type_name(ty: Type) -> String {
    match ty {
        Type::Int(kind) => kind.name().to_owned(),
        Type::Bool => "BOOL".to_owned(),
        Type::Char => "CHAR".to_owned(),
        Type::String => "CHARS".to_owned(),
        // Structure modes must be referenced symbolically; this branch should
        // not be reached because portable types use `ExternalType::Mode`.
        Type::Struct(_) => "STRUCT".to_owned(),
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
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some(manifest.package.name.clone());
        }
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
        assert_eq!(bump.module.as_deref(), Some("Counter"));
        assert_eq!(bump.package.as_deref(), Some("modules"));
        assert!(bump.params.is_empty());
        assert_eq!(bump.result, None);

        let value = grant.exports.iter().find(|e| e.name == "value").unwrap();
        assert_eq!(value.kind, GrantKind::Variable);
        assert_eq!(value.mode.as_deref(), Some("INT"));
    }

    #[test]
    fn emits_qualified_exports_for_duplicate_simple_names() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "dup"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
A: MODULE
  GRANT x;
  DCL x INT;
END A;

B: MODULE
  GRANT x;
  DCL x INT;
END B;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("dup".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();

        assert_eq!(grant.exports.len(), 2);
        assert!(grant.exports.iter().all(|export| export.name == "x"));
        assert_eq!(grant.exports[0].module.as_deref(), Some("A"));
        assert_eq!(grant.exports[1].module.as_deref(), Some("B"));
        assert!(grant
            .exports
            .iter()
            .all(|export| export.package.as_deref() == Some("dup")));
    }

    #[test]
    fn emits_prefixed_export() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "pref"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
A: MODULE
  GRANT x PREFIXED P;
  DCL x INT;
END A;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("pref".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();

        assert_eq!(grant.exports.len(), 1);
        assert_eq!(grant.exports[0].prefix.as_deref(), Some("P"));

        let interface = grant.to_interface().unwrap();
        assert_eq!(interface.exports[0].prefix, vec!["P".to_owned()]);
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
        let interface = grant.to_interface().unwrap();
        let infos = &interface.exports;

        assert_eq!(infos.len(), 2);
        let value = infos.iter().find(|info| info.name == "value").unwrap();
        assert!(matches!(
            value.kind,
            ExportInfoKind::Var(program::ExternalType::Builtin(Type::Int(_)))
        ));

        let bump = infos.iter().find(|info| info.name == "Bump").unwrap();
        assert!(matches!(
            &bump.kind,
            ExportInfoKind::Proc { params, result } if params.is_empty() && result.is_none()
        ));
    }

    #[test]
    fn records_structure_modes_forbid_and_friends() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "structs"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
A: MODULE
  NEWMODE Inner = STRUCT(v INT);
  NEWMODE S = STRUCT(x INT, inner Inner, hidden BOOL);
  GRANT S FORBID (hidden) TO B;
END A;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("structs".to_owned());
        }

        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        assert_eq!(grant.modes.len(), 2);
        let outer = grant.modes.iter().find(|mode| mode.name == "S").unwrap();
        assert!(outer
            .fields
            .iter()
            .any(|field| field.name == "inner" && field.mode == "structs!A!Inner"));

        let export = grant
            .exports
            .iter()
            .find(|export| export.kind == GrantKind::Mode && export.name == "S")
            .unwrap();
        assert_eq!(export.forbid, vec!["hidden".to_owned()]);
        assert_eq!(export.friends, vec!["B".to_owned()]);

        let text = grant.to_toml().unwrap();
        assert!(text.contains("schema = 2"));
        assert!(text.contains("[[modes]]"));

        let interface = grant.to_interface().unwrap();
        let mode = interface
            .exports
            .iter()
            .find_map(|export| match &export.kind {
                ExportInfoKind::Mode(mode) => Some(mode),
                _ => None,
            })
            .unwrap();
        assert_eq!(mode.forbidden, vec!["hidden".to_owned()]);
        assert!(matches!(
            mode.fields
                .iter()
                .find(|(name, _)| name == "inner")
                .map(|(_, ty)| ty),
            Some(program::ExternalType::Mode(symbol)) if symbol == "structs!A!Inner"
        ));
    }

    #[test]
    fn emits_regranted_child_export() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "regrant"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("regrant".to_owned());
        }

        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let export = grant
            .exports
            .iter()
            .find(|export| export.name == "x")
            .unwrap();
        assert_eq!(export.kind, GrantKind::Variable);
        assert_eq!(export.module.as_deref(), Some("M"));
        assert_eq!(export.mode.as_deref(), Some("INT"));
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
