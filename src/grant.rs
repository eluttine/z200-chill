use crate::ast::{Passing, Program};
use crate::manifest::Manifest;
use crate::program::{self, ExportInfo, ExportInfoKind};
use crate::types::{self, Type};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 4;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default)]
    pub kind: GrantModeKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<GrantField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<GrantSetElement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<GrantComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantSetElement {
    pub name: String,
    /// Set representation value, stored as text because TOML integers are i64.
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GrantModeKind {
    #[default]
    Struct,
    Set,
    Moreta,
    Region,
    Task,
    Interface,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantComponent {
    pub name: String,
    pub kind: GrantComponentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passing: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub constructor: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub destructor: bool,
    #[serde(default = "default_public")]
    pub public: bool,
}

fn default_public() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantComponentKind {
    Variable,
    Procedure,
    Mode,
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
    /// Parameter passing modes, in `params` order. Empty means all `in`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passing: Vec<String>,
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
        let program = program::normalize(program)?;
        let info = program::analyze(&program)?;
        let exports = program::exports(&program)?;
        if let Err(message) = validate_exports(&exports, &info.modes) {
            let span = program
                .modules
                .first()
                .map(|module| module.span)
                .unwrap_or_else(|| crate::lexer::Span::new(crate::lexer::SourceId(0), 0, 0));
            return Err(program::Error::coded("E0406", span, message));
        }
        let mut modes: Vec<GrantMode> = info
            .modes
            .modes
            .iter()
            .filter(|mode| mode.module_name.is_some())
            .map(|mode| GrantMode {
                name: mode.path.clone().unwrap_or_else(|| mode.name.clone()),
                implements: Vec::new(),
                module: mode.module_name.clone(),
                package: mode.package_name.clone(),
                kind: GrantModeKind::Struct,
                fields: mode
                    .fields
                    .iter()
                    .map(|field| GrantField {
                        name: field.name.clone(),
                        mode: external_type_name(&program::portable_type(field.ty, &info.modes)),
                    })
                    .collect(),
                elements: Vec::new(),
                components: Vec::new(),
            })
            .chain(
                info.modes
                    .sets
                    .iter()
                    .filter(|mode| mode.module_name.is_some())
                    .map(|mode| GrantMode {
                        name: mode.path.clone().unwrap_or_else(|| mode.name.clone()),
                        implements: Vec::new(),
                        module: mode.module_name.clone(),
                        package: mode.package_name.clone(),
                        kind: GrantModeKind::Set,
                        fields: Vec::new(),
                        elements: mode
                            .elements
                            .iter()
                            .map(|(name, value)| GrantSetElement {
                                name: name.clone(),
                                value: value.to_string(),
                            })
                            .collect(),
                        components: Vec::new(),
                    }),
            )
            .chain(
                info.modes
                    .moretas
                    .iter()
                    .filter(|mode| mode.module_name.is_some())
                    .map(|mode| GrantMode {
                        name: mode.path.clone().unwrap_or_else(|| mode.name.clone()),
                        implements: mode
                            .implements
                            .iter()
                            .map(|id| info.modes.get_moreta(*id).symbol())
                            .collect(),
                        module: mode.module_name.clone(),
                        package: mode.package_name.clone(),
                        kind: match mode.kind {
                            crate::ast::MoretaKind::Region => GrantModeKind::Region,
                            crate::ast::MoretaKind::Task => GrantModeKind::Task,
                            crate::ast::MoretaKind::Interface => GrantModeKind::Interface,
                            _ => GrantModeKind::Moreta,
                        },
                        fields: Vec::new(),
                        elements: Vec::new(),
                        components: mode
                            .components
                            .iter()
                            .map(|component| match component {
                                crate::modes::MoretaComponent::Var {
                                    name,
                                    ty,
                                    visibility,
                                } => GrantComponent {
                                    name: name.clone(),
                                    kind: GrantComponentKind::Variable,
                                    mode: Some(external_type_name(&program::portable_type(
                                        *ty,
                                        &info.modes,
                                    ))),
                                    params: Vec::new(),
                                    passing: Vec::new(),
                                    result: None,
                                    constructor: false,
                                    destructor: false,
                                    public: *visibility
                                        == crate::modes::ComponentVisibility::Public,
                                },
                                crate::modes::MoretaComponent::Proc {
                                    name,
                                    signature,
                                    constructor,
                                    destructor,
                                    visibility,
                                    ..
                                } => GrantComponent {
                                    name: name.clone(),
                                    kind: GrantComponentKind::Procedure,
                                    mode: None,
                                    params: signature
                                        .params
                                        .iter()
                                        .map(|ty| {
                                            external_type_name(&program::portable_type(
                                                *ty,
                                                &info.modes,
                                            ))
                                        })
                                        .collect(),
                                    passing: signature
                                        .passing
                                        .iter()
                                        .map(|passing| passing_name(*passing).to_owned())
                                        .collect(),
                                    result: signature.result.map(|ty| {
                                        external_type_name(&program::portable_type(ty, &info.modes))
                                    }),
                                    constructor: *constructor,
                                    destructor: *destructor,
                                    public: *visibility
                                        == crate::modes::ComponentVisibility::Public,
                                },
                                crate::modes::MoretaComponent::Mode {
                                    name,
                                    ty,
                                    visibility,
                                } => GrantComponent {
                                    name: name.clone(),
                                    kind: GrantComponentKind::Mode,
                                    mode: Some(external_type_name(&program::portable_type(
                                        *ty,
                                        &info.modes,
                                    ))),
                                    params: Vec::new(),
                                    passing: Vec::new(),
                                    result: None,
                                    constructor: false,
                                    destructor: false,
                                    public: *visibility
                                        == crate::modes::ComponentVisibility::Public,
                                },
                            })
                            .collect(),
                    }),
            )
            .collect();
        modes.sort_by_key(GrantMode::symbol);

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
                    passing: Vec::new(),
                    result: None,
                    forbid: Vec::new(),
                    friends,
                }),
                ExportInfoKind::Proc {
                    params,
                    passing,
                    result,
                } => entries.push(GrantExport {
                    name,
                    module: module_name,
                    package: package_name,
                    prefix,
                    kind: GrantKind::Procedure,
                    mode: None,
                    params: params.iter().map(external_type_name).collect(),
                    passing: passing
                        .iter()
                        .map(|passing| passing_name(*passing).to_owned())
                        .collect(),
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
                    passing: Vec::new(),
                    result: None,
                    forbid: mode.forbidden.clone(),
                    friends,
                }),
            }
        }

        Ok(Self {
            schema: SCHEMA,
            package: manifest.package.name.clone(),
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
            let components = mode
                .components
                .iter()
                .map(|component| match component.kind {
                    GrantComponentKind::Variable => {
                        let ty = component.mode.as_deref().ok_or_else(|| {
                            format!(
                                "variable component `{}` has no mode in the grant file",
                                component.name
                            )
                        })?;
                        Ok(program::ModeComponentInfo::Var {
                            name: component.name.clone(),
                            ty: parse_external_type(ty, &definitions)?,
                            public: component.public,
                        })
                    }
                    GrantComponentKind::Procedure => {
                        let params = component
                            .params
                            .iter()
                            .map(|mode| parse_external_type(mode, &definitions))
                            .collect::<Result<Vec<_>, _>>()?;
                        let passing = component
                            .params
                            .iter()
                            .enumerate()
                            .map(|(index, _)| match component.passing.get(index) {
                                Some(name) => parse_passing(name),
                                None => Ok(Passing::In),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if passing.len() != params.len() {
                            return Err(format!(
                                "procedure component `{}` has mismatched passing modes",
                                component.name
                            ));
                        }
                        let result = match &component.result {
                            Some(mode) => Some(parse_external_type(mode, &definitions)?),
                            None => None,
                        };
                        Ok(program::ModeComponentInfo::Proc {
                            name: component.name.clone(),
                            params,
                            passing,
                            result,
                            constructor: component.constructor,
                            destructor: component.destructor,
                            public: component.public,
                        })
                    }
                    GrantComponentKind::Mode => {
                        let ty = component.mode.as_deref().ok_or_else(|| {
                            format!(
                                "mode component `{}` has no mode in the grant file",
                                component.name
                            )
                        })?;
                        Ok(program::ModeComponentInfo::Mode {
                            name: component.name.clone(),
                            ty: parse_external_type(ty, &definitions)?,
                            public: component.public,
                        })
                    }
                })
                .collect::<Result<Vec<_>, String>>()?;
            mode_indices.insert(mode.symbol(), modes.len());
            modes.push(program::ModeInfo {
                name: mode.name.clone(),
                implements: mode.implements.clone(),
                module_name: mode.module.clone(),
                package_name: mode.package.clone(),
                fields,
                elements: mode
                    .elements
                    .iter()
                    .map(|element| {
                        Ok((
                            element.name.clone(),
                            element.value.parse::<i128>().map_err(|_| {
                                format!(
                                    "invalid set element value `{}` in grant file",
                                    element.value
                                )
                            })?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                forbidden: Vec::new(),
                kind: match mode.kind {
                    GrantModeKind::Struct => program::ModeInfoKind::Struct,
                    GrantModeKind::Set => program::ModeInfoKind::Set,
                    GrantModeKind::Moreta => program::ModeInfoKind::Moreta,
                    GrantModeKind::Region => program::ModeInfoKind::Region,
                    GrantModeKind::Task => program::ModeInfoKind::Task,
                    GrantModeKind::Interface => program::ModeInfoKind::Interface,
                },
                components,
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
                    let passing = export
                        .params
                        .iter()
                        .enumerate()
                        .map(|(index, _)| match export.passing.get(index) {
                            Some(name) => parse_passing(name),
                            None => Ok(Passing::In),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    ExportInfoKind::Proc {
                        params,
                        passing,
                        result,
                    }
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

fn passing_name(passing: Passing) -> &'static str {
    match passing {
        Passing::In => "in",
        Passing::Out => "out",
        Passing::InOut => "inout",
        Passing::Loc => "loc",
    }
}

fn parse_passing(name: &str) -> Result<Passing, String> {
    match name {
        "in" => Ok(Passing::In),
        "out" => Ok(Passing::Out),
        "inout" => Ok(Passing::InOut),
        "loc" => Ok(Passing::Loc),
        other => Err(format!("unknown parameter passing mode `{other}`")),
    }
}

fn validate_exports(
    exports: &[program::ExportInfo],
    modes: &crate::modes::ModeTable,
) -> Result<(), String> {
    for export in exports {
        match &export.kind {
            program::ExportInfoKind::Var(ty) => validate_external_type(ty)?,
            program::ExportInfoKind::Proc {
                params,
                passing,
                result,
            } => {
                if passing.len() != params.len() {
                    return Err(format!(
                        "procedure export `{}` has {} parameter mode(s) but {} passing mode(s)",
                        export.name,
                        params.len(),
                        passing.len()
                    ));
                }
                for ty in params {
                    validate_external_type(ty)?;
                }
                if let Some(ty) = result {
                    validate_external_type(ty)?;
                }
            }
            program::ExportInfoKind::Mode(mode) => {
                for (_, ty) in &mode.fields {
                    validate_external_type(ty)?;
                }
            }
        }
    }
    for mode in &modes.modes {
        for field in &mode.fields {
            validate_external_type(&program::portable_type(field.ty, modes))?;
        }
    }
    Ok(())
}

fn validate_external_type(ty: &program::ExternalType) -> Result<(), String> {
    match ty {
        program::ExternalType::Builtin(Type::Moreta(_)) => {
            Err("module mode types are not supported in grant interfaces yet".to_owned())
        }
        program::ExternalType::Builtin(Type::Buffer { element, .. })
            if !matches!(
                element,
                crate::types::RefTarget::Int(_)
                    | crate::types::RefTarget::Float
                    | crate::types::RefTarget::Bool
                    | crate::types::RefTarget::Char
                    | crate::types::RefTarget::String
                    | crate::types::RefTarget::BoundedString { .. }
                    | crate::types::RefTarget::WChar
                    | crate::types::RefTarget::WString
                    | crate::types::RefTarget::WBoundedString { .. }
                    | crate::types::RefTarget::Bits
                    | crate::types::RefTarget::BoundedBits { .. }
                    | crate::types::RefTarget::Text { .. }
            ) =>
        {
            Err("buffer element modes in grant interfaces must be scalar".to_owned())
        }
        program::ExternalType::Array { element, .. }
        | program::ExternalType::MultiArray { element, .. }
        | program::ExternalType::Ref(element)
        | program::ExternalType::Row(element) => validate_external_type(element),
        program::ExternalType::Buffer { element, .. } => validate_external_type(element),
        _ => Ok(()),
    }
}

fn parse_external_type(
    name: &str,
    definitions: &std::collections::HashMap<String, &GrantMode>,
) -> Result<program::ExternalType, String> {
    if let Some(rest) = name.strip_prefix("REF ") {
        let target = parse_external_type(rest.trim(), definitions)?;
        return Ok(program::ExternalType::Ref(Box::new(target)));
    }
    if let Some(rest) = name.strip_prefix("ROW ") {
        let target = parse_external_type(rest.trim(), definitions)?;
        return Ok(program::ExternalType::Row(Box::new(target)));
    }
    if name == "PTR" {
        return Ok(program::ExternalType::Ptr);
    }
    if let Some(rest) = name.strip_prefix("ARRAY(") {
        let (bounds, element) = rest
            .split_once(')')
            .ok_or_else(|| format!("invalid array mode `{name}` in grant file"))?;
        let mut dims = Vec::new();
        for bound in bounds.split(',') {
            let (low, high) = bound
                .split_once(':')
                .ok_or_else(|| format!("invalid array mode `{name}` in grant file"))?;
            let low = low
                .trim()
                .parse::<i128>()
                .map_err(|_| format!("invalid array lower bound in `{name}`"))?;
            let high = high
                .trim()
                .parse::<i128>()
                .map_err(|_| format!("invalid array upper bound in `{name}`"))?;
            if low > high {
                return Err(format!("invalid array bounds in `{name}`"));
            }
            dims.push(types::ArrayDimension { low, high });
        }
        let element = parse_external_type(element.trim(), definitions)?;
        if matches!(
            element,
            program::ExternalType::Array { .. }
                | program::ExternalType::MultiArray { .. }
                | program::ExternalType::Builtin(Type::Array(_) | Type::MultiArray(_))
        ) {
            return Err(format!(
                "nested array elements are not implemented in `{name}`"
            ));
        }
        return match dims.as_slice() {
            [dim] => Ok(program::ExternalType::Array {
                low: dim.low,
                high: dim.high,
                element: Box::new(element),
            }),
            _ => Ok(program::ExternalType::MultiArray {
                dims,
                element: Box::new(element),
            }),
        };
    }
    if let Some(member) = name.strip_prefix("POWERSET ") {
        let member = match types::resolve_mode(member.trim()) {
            Some(Type::Int(kind)) => types::PowerSetMember::Int(kind),
            Some(Type::Bool) => types::PowerSetMember::Bool,
            Some(Type::Char) => types::PowerSetMember::Char,
            Some(Type::WChar) => types::PowerSetMember::WChar,
            _ => return Err(format!("invalid POWERSET member mode in `{name}`")),
        };
        return Ok(program::ExternalType::Builtin(Type::PowerSet(
            types::PowerSetType { member },
        )));
    }
    if name == "ACCESS CHARS" {
        return Ok(program::ExternalType::Builtin(Type::Access(
            types::AccessType {
                indexed: false,
                index: None,
            },
        )));
    }
    if let Some(rest) = name.strip_prefix("ACCESS(") {
        let (index, suffix) = rest
            .split_once(')')
            .ok_or_else(|| format!("invalid access mode `{name}` in grant file"))?;
        if suffix.trim() != "CHARS" {
            return Err(format!("invalid access record mode in `{name}`"));
        }
        let index = index.trim();
        let access = if index == "INT" || index == "SHORT_INT" || index == "LONG_INT" {
            types::AccessType {
                indexed: true,
                index: None,
            }
        } else {
            let (low, high) = index
                .split_once(':')
                .ok_or_else(|| format!("invalid access index mode in `{name}`"))?;
            let low = low
                .trim()
                .parse::<i128>()
                .map_err(|_| format!("invalid access index lower bound in `{name}`"))?;
            let high = high
                .trim()
                .parse::<i128>()
                .map_err(|_| format!("invalid access index upper bound in `{name}`"))?;
            types::AccessType {
                indexed: true,
                index: Some(types::ArrayDimension { low, high }),
            }
        };
        return Ok(program::ExternalType::Builtin(Type::Access(access)));
    }
    if let Some(rest) = name.strip_prefix("BUFFER(") {
        let (length, element) = rest
            .split_once(')')
            .ok_or_else(|| format!("invalid buffer mode `{name}` in grant file"))?;
        let length = length
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid buffer length in `{name}`"))?;
        let element = parse_external_type(element.trim(), definitions)?;
        return Ok(program::ExternalType::Buffer {
            length: Some(length),
            element: Box::new(element),
        });
    }
    if let Some(rest) = name.strip_prefix("BUFFER ") {
        let element = parse_external_type(rest.trim(), definitions)?;
        return Ok(program::ExternalType::Buffer {
            length: None,
            element: Box::new(element),
        });
    }
    if let Some(rest) = name.strip_prefix("EVENT(") {
        let length = rest
            .strip_suffix(')')
            .ok_or_else(|| format!("invalid event mode `{name}` in grant file"))?
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid event length in `{name}`"))?;
        return Ok(program::ExternalType::Builtin(Type::Event {
            length: Some(length),
        }));
    }
    if let Some(rest) = name.strip_prefix("WTEXT(") {
        let (length, indexed, index) = parse_text_mode_body(rest, name)?;
        return Ok(program::ExternalType::Builtin(Type::Text {
            length,
            wide: true,
            indexed,
            index,
        }));
    }
    if let Some(rest) = name.strip_prefix("TEXT(") {
        let (length, indexed, index) = parse_text_mode_body(rest, name)?;
        return Ok(program::ExternalType::Builtin(Type::Text {
            length,
            wide: false,
            indexed,
            index,
        }));
    }
    if let Some(rest) = name.strip_prefix("BOOLS(") {
        let (length, suffix) = rest
            .split_once(')')
            .ok_or_else(|| format!("invalid string mode `{name}` in grant file"))?;
        let length = length
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid string length in `{name}`"))?;
        let varying = match suffix.trim() {
            "" => false,
            "VARYING" => true,
            other => return Err(format!("invalid string attribute `{other}` in `{name}`")),
        };
        return Ok(program::ExternalType::Builtin(Type::BoundedBits {
            length,
            varying,
        }));
    }
    if let Some(rest) = name.strip_prefix("WCHARS(") {
        let (length, suffix) = rest
            .split_once(')')
            .ok_or_else(|| format!("invalid string mode `{name}` in grant file"))?;
        let length = length
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid string length in `{name}`"))?;
        let varying = match suffix.trim() {
            "" => false,
            "VARYING" => true,
            other => return Err(format!("invalid string attribute `{other}` in `{name}`")),
        };
        return Ok(program::ExternalType::Builtin(Type::WBoundedString {
            length,
            varying,
        }));
    }
    if let Some(rest) = name.strip_prefix("CHARS(") {
        let (length, suffix) = rest
            .split_once(')')
            .ok_or_else(|| format!("invalid string mode `{name}` in grant file"))?;
        let length = length
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid string length in `{name}`"))?;
        let varying = match suffix.trim() {
            "" => false,
            "VARYING" => true,
            other => return Err(format!("invalid string attribute `{other}` in `{name}`")),
        };
        return Ok(program::ExternalType::Builtin(Type::BoundedString {
            length,
            varying,
        }));
    }
    if let Some(ty) = types::resolve_mode(name) {
        return Ok(program::ExternalType::Builtin(ty));
    }
    if name.contains('!') {
        if let Some(definition) = definitions.get(name) {
            return Ok(match definition.kind {
                GrantModeKind::Struct => program::ExternalType::Mode(name.to_owned()),
                GrantModeKind::Set => program::ExternalType::Set(name.to_owned()),
                GrantModeKind::Moreta
                | GrantModeKind::Region
                | GrantModeKind::Task
                | GrantModeKind::Interface => program::ExternalType::Moreta(name.to_owned()),
            });
        }
        return Err(format!(
            "grant file references mode `{name}` but does not define it"
        ));
    }
    Err(format!("unsupported mode `{name}` in grant file"))
}

fn parse_text_mode_body(
    rest: &str,
    name: &str,
) -> Result<(usize, bool, Option<types::ArrayDimension>), String> {
    let (length, suffix) = rest
        .split_once(')')
        .ok_or_else(|| format!("invalid text mode `{name}` in grant file"))?;
    let length = length
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid text length in `{name}`"))?;
    let suffix = suffix.trim();
    if suffix.is_empty() {
        return Ok((length, false, None));
    }
    if matches!(suffix, "INT" | "SHORT_INT" | "LONG_INT") {
        return Ok((length, true, None));
    }
    let (low, high) = suffix
        .split_once(':')
        .ok_or_else(|| format!("invalid text index mode in `{name}`"))?;
    let low = low
        .trim()
        .parse::<i128>()
        .map_err(|_| format!("invalid text index lower bound in `{name}`"))?;
    let high = high
        .trim()
        .parse::<i128>()
        .map_err(|_| format!("invalid text index upper bound in `{name}`"))?;
    if low > high {
        return Err(format!("invalid text index bounds in `{name}`"));
    }
    Ok((length, true, Some(types::ArrayDimension { low, high })))
}

fn external_type_name(ty: &program::ExternalType) -> String {
    match ty {
        program::ExternalType::Builtin(ty) => type_name(*ty),
        program::ExternalType::Mode(symbol)
        | program::ExternalType::Moreta(symbol)
        | program::ExternalType::Set(symbol) => symbol.clone(),
        program::ExternalType::Ptr => "PTR".to_owned(),
        program::ExternalType::Ref(target) => {
            format!("REF {}", external_type_name(target))
        }
        program::ExternalType::Row(target) => {
            format!("ROW {}", external_type_name(target))
        }
        program::ExternalType::Buffer { length, element } => match length {
            Some(length) => format!("BUFFER({length}) {}", external_type_name(element)),
            None => format!("BUFFER {}", external_type_name(element)),
        },
        program::ExternalType::Array { low, high, element } => {
            format!("ARRAY({low}:{high}) {}", external_type_name(element))
        }
        program::ExternalType::MultiArray { dims, element } => {
            let bounds = dims
                .iter()
                .map(|dim| format!("{}:{}", dim.low, dim.high))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ARRAY({bounds}) {}", external_type_name(element))
        }
    }
}

fn type_name(ty: Type) -> String {
    match ty {
        Type::Int(kind) => kind.name().to_owned(),
        Type::Bool => "BOOL".to_owned(),
        Type::Float => "FLOAT".to_owned(),
        Type::Char => "CHAR".to_owned(),
        Type::String => "CHARS".to_owned(),
        Type::BoundedString {
            length,
            varying: true,
        } => format!("CHARS({length}) VARYING"),
        Type::BoundedString {
            length,
            varying: false,
        } => format!("CHARS({length})"),
        Type::WChar => "WCHAR".to_owned(),
        Type::WString => "WCHARS".to_owned(),
        Type::WBoundedString {
            length,
            varying: true,
        } => format!("WCHARS({length}) VARYING"),
        Type::WBoundedString {
            length,
            varying: false,
        } => format!("WCHARS({length})"),
        Type::Text {
            length,
            wide,
            indexed,
            index,
        } => {
            let name = if wide {
                format!("WTEXT({length})")
            } else {
                format!("TEXT({length})")
            };
            if !indexed {
                name
            } else {
                match index {
                    Some(bounds) => format!("{name} {}:{}", bounds.low, bounds.high),
                    None => format!("{name} INT"),
                }
            }
        }
        Type::Event {
            length: Some(length),
        } => format!("EVENT({length})"),
        Type::Event { length: None } => "EVENT".to_owned(),
        Type::Instance => "INSTANCE".to_owned(),
        Type::Time => "TIME".to_owned(),
        Type::Buffer { length, element } => match length {
            Some(length) => format!("BUFFER({length}) {}", type_name(element.ty())),
            None => format!("BUFFER {}", type_name(element.ty())),
        },
        Type::Duration => "DURATION".to_owned(),
        Type::Bits => "BOOLS".to_owned(),
        Type::BoundedBits {
            length,
            varying: true,
        } => format!("BOOLS({length}) VARYING"),
        Type::BoundedBits {
            length,
            varying: false,
        } => format!("BOOLS({length})"),
        Type::Ref(target) => format!("REF {}", type_name(target.ty())),
        Type::Ptr => "PTR".to_owned(),
        Type::Row(target) => format!("ROW {}", type_name(target.ty())),
        Type::Null => "NULL".to_owned(),
        // Structure modes must be referenced symbolically; this branch should
        // not be reached because portable types use `ExternalType::Mode`.
        Type::Struct(_) => "STRUCT".to_owned(),
        Type::Set(_) => "SET".to_owned(),
        Type::Moreta(_) => "MODULE".to_owned(),
        Type::Array(array) => format!(
            "ARRAY({}:{}) {}",
            array.low,
            array.high,
            type_name(array.element.ty())
        ),
        // Expression-only classes; they cannot occur in a declared signature.
        Type::IntLiteral => "INT".to_owned(),
        Type::MultiArray(array) => {
            let bounds = array
                .dimensions()
                .iter()
                .map(|dim| format!("{}:{}", dim.low, dim.high))
                .collect::<Vec<_>>()
                .join(", ");
            format!("ARRAY({bounds}) {}", type_name(array.element.ty()))
        }
        Type::Association => "ASSOCIATION".to_owned(),
        Type::PowerSet(set) => format!(
            "POWERSET {}",
            match set.member {
                types::PowerSetMember::Int(kind) => kind.name(),
                types::PowerSetMember::Bool => "BOOL",
                types::PowerSetMember::Char => "CHAR",
                types::PowerSetMember::WChar => "WCHAR",
            }
        ),
        Type::Access(access) => {
            if !access.indexed {
                "ACCESS CHARS".to_owned()
            } else if let Some(index) = access.index {
                format!("ACCESS({}:{}) CHARS", index.low, index.high)
            } else {
                "ACCESS(INT) CHARS".to_owned()
            }
        }
        Type::ArrayTuple | Type::DynamicArray { .. } => "ARRAY".to_owned(),
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

[build]
entry = "src/main.ch"
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

[build]
entry = "src/main.ch"
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
            ExportInfoKind::Proc { params, result, .. } if params.is_empty() && result.is_none()
        ));
    }

    #[test]
    fn records_structure_modes_forbid_and_friends() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "structs"

[build]
entry = "src/main.ch"
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
        assert!(text.contains("schema = 4"));
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

[build]
entry = "src/main.ch"
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
    fn array_types_round_trip() {
        let ty = Type::Array(types::ArrayType {
            low: -1,
            high: 2,
            element: types::ArrayElement::Scalar(types::Scalar::Int(types::IntKind::Int)),
        });
        let text = type_name(ty);
        assert_eq!(
            parse_external_type(&text, &std::collections::HashMap::new()).unwrap(),
            program::ExternalType::Array {
                low: -1,
                high: 2,
                element: Box::new(program::ExternalType::Builtin(Type::Int(
                    types::IntKind::Int
                ))),
            }
        );
    }

    #[test]
    fn powerset_types_round_trip() {
        for member in [
            types::PowerSetMember::Int(types::IntKind::Int),
            types::PowerSetMember::Bool,
            types::PowerSetMember::Char,
        ] {
            let ty = Type::PowerSet(types::PowerSetType { member });
            let text = type_name(ty);
            assert_eq!(
                parse_external_type(&text, &std::collections::HashMap::new()).unwrap(),
                program::ExternalType::Builtin(ty)
            );
        }
    }

    #[test]
    fn access_types_round_trip() {
        for access in [
            types::AccessType {
                indexed: false,
                index: None,
            },
            types::AccessType {
                indexed: true,
                index: None,
            },
            types::AccessType {
                indexed: true,
                index: Some(types::ArrayDimension { low: 0, high: 3 }),
            },
        ] {
            let ty = Type::Access(access);
            let text = type_name(ty);
            assert_eq!(
                parse_external_type(&text, &std::collections::HashMap::new()).unwrap(),
                program::ExternalType::Builtin(ty)
            );
        }
    }

    #[test]
    fn named_sets_and_references_round_trip() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "sets"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT S, x, y;
  NEWMODE S = SET(a, b = 5);
  DCL x S;
  DCL y REF S;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("sets".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let set = grant.modes.iter().find(|mode| mode.name == "S").unwrap();
        assert_eq!(set.kind, GrantModeKind::Set);
        assert_eq!(
            set.elements,
            vec![
                GrantSetElement {
                    name: "a".to_owned(),
                    value: "0".to_owned()
                },
                GrantSetElement {
                    name: "b".to_owned(),
                    value: "5".to_owned()
                },
            ]
        );

        let interface = GrantFile::from_toml(&grant.to_toml().unwrap())
            .unwrap()
            .to_interface()
            .unwrap();
        let set = interface
            .modes
            .iter()
            .find(|mode| mode.name == "S")
            .unwrap();
        assert_eq!(set.kind, program::ModeInfoKind::Set);
        assert_eq!(set.elements.len(), 2);
        let symbol = "sets!M!S";
        let x = interface
            .exports
            .iter()
            .find(|export| export.name == "x")
            .unwrap();
        assert!(matches!(
            &x.kind,
            ExportInfoKind::Var(program::ExternalType::Set(name)) if name == symbol
        ));
        let y = interface
            .exports
            .iter()
            .find(|export| export.name == "y")
            .unwrap();
        assert!(matches!(
            &y.kind,
            ExportInfoKind::Var(program::ExternalType::Ref(target))
                if matches!(target.as_ref(), program::ExternalType::Set(name) if name == symbol)
        ));
    }

    #[test]
    fn buffer_of_structure_mode_round_trips() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "buffers"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT Point, b;
  NEWMODE Point = STRUCT(x INT);
  DCL b BUFFER(2) Point;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("buffers".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let buffer = grant
            .exports
            .iter()
            .find(|export| export.name == "b")
            .unwrap();
        assert_eq!(buffer.mode.as_deref(), Some("BUFFER(2) buffers!M!Point"));

        let interface = GrantFile::from_toml(&grant.to_toml().unwrap())
            .unwrap()
            .to_interface()
            .unwrap();
        let buffer = interface
            .exports
            .iter()
            .find(|export| export.name == "b")
            .unwrap();
        assert!(matches!(
            &buffer.kind,
            ExportInfoKind::Var(program::ExternalType::Buffer { length: Some(2), element })
                if matches!(element.as_ref(), program::ExternalType::Mode(name) if name == "buffers!M!Point")
        ));
    }

    #[test]
    fn bounded_string_types_round_trip() {
        for ty in [
            Type::BoundedString {
                length: 20,
                varying: false,
            },
            Type::BoundedString {
                length: 20,
                varying: true,
            },
            Type::WBoundedString {
                length: 20,
                varying: false,
            },
            Type::WBoundedString {
                length: 20,
                varying: true,
            },
            Type::WChar,
            Type::WString,
            Type::Bits,
            Type::BoundedBits {
                length: 12,
                varying: false,
            },
            Type::BoundedBits {
                length: 12,
                varying: true,
            },
            Type::Text {
                length: 20,
                wide: false,
                indexed: false,
                index: None,
            },
            Type::Text {
                length: 20,
                wide: true,
                indexed: false,
                index: None,
            },
            Type::Text {
                length: 20,
                wide: false,
                indexed: true,
                index: None,
            },
            Type::Text {
                length: 20,
                wide: true,
                indexed: true,
                index: Some(types::ArrayDimension { low: 0, high: 3 }),
            },
        ] {
            let text = type_name(ty);
            assert_eq!(
                parse_external_type(&text, &std::collections::HashMap::new()).unwrap(),
                program::ExternalType::Builtin(ty)
            );
        }
    }

    #[test]
    fn multidimensional_array_types_round_trip() {
        let ty = program::ExternalType::MultiArray {
            dims: vec![
                types::ArrayDimension { low: 1, high: 2 },
                types::ArrayDimension { low: 0, high: 3 },
            ],
            element: Box::new(program::ExternalType::Builtin(Type::Int(
                types::IntKind::Int,
            ))),
        };
        let text = external_type_name(&ty);
        assert_eq!(text, "ARRAY(1:2, 0:3) INT");
        assert_eq!(
            parse_external_type(&text, &std::collections::HashMap::new()).unwrap(),
            ty
        );
    }

    #[test]
    fn encodes_module_modes() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "moreta"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT P, Obj;
  NEWMODE P = MODULE BODY
    DCL value INT;
    Get: PROC () RETURNS (INT);
      RESULT value;
    END Get;
  END P;
  DCL Obj P;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("moreta".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let mode = grant.modes.iter().find(|mode| mode.name == "P").unwrap();
        assert_eq!(mode.kind, GrantModeKind::Moreta);
        assert!(mode.components.iter().any(|component| {
            component.name == "value" && component.kind == GrantComponentKind::Variable
        }));
        assert!(mode.components.iter().any(|component| {
            component.name == "Get" && component.kind == GrantComponentKind::Procedure
        }));

        let parsed = GrantFile::from_toml(&grant.to_toml().unwrap()).unwrap();
        let interface = parsed.to_interface().unwrap();
        let info = interface
            .modes
            .iter()
            .find(|mode| mode.name == "P")
            .unwrap();
        assert_eq!(info.kind, program::ModeInfoKind::Moreta);
        let obj = interface
            .exports
            .iter()
            .find(|export| export.name == "Obj")
            .unwrap();
        assert!(matches!(
            &obj.kind,
            ExportInfoKind::Var(program::ExternalType::Moreta(_))
        ));
    }

    #[test]
    fn encodes_region_modes() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "regions"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT R;
  NEWMODE R = REGION BODY
    DCL value INT;
  END R;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("regions".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let mode = grant.modes.iter().find(|mode| mode.name == "R").unwrap();
        assert_eq!(mode.kind, GrantModeKind::Region);
        let interface = GrantFile::from_toml(&grant.to_toml().unwrap())
            .unwrap()
            .to_interface()
            .unwrap();
        let info = interface
            .modes
            .iter()
            .find(|mode| mode.name == "R")
            .unwrap();
        assert_eq!(info.kind, program::ModeInfoKind::Region);
    }

    #[test]
    fn encodes_task_modes() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "tasks"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT T;
  NEWMODE T = TASK BODY
    DCL value INT;
    Set: PROC (n INT);
      value := n;
    END Set;
  END T;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("tasks".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let mode = grant.modes.iter().find(|mode| mode.name == "T").unwrap();
        assert_eq!(mode.kind, GrantModeKind::Task);
        let interface = GrantFile::from_toml(&grant.to_toml().unwrap())
            .unwrap()
            .to_interface()
            .unwrap();
        let info = interface
            .modes
            .iter()
            .find(|mode| mode.name == "T")
            .unwrap();
        assert_eq!(info.kind, program::ModeInfoKind::Task);
    }

    #[test]
    fn nested_mode_paths_round_trip() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT Holder;
  NEWMODE Holder = MODULE BODY
    NEWMODE Point = STRUCT (x INT);
    DCL p Point;
  END Holder;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("nested".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let point = grant
            .modes
            .iter()
            .find(|mode| mode.name == "Holder!Point")
            .expect("nested mode should keep its qualified path");
        assert_eq!(point.symbol(), "nested!M!Holder!Point");

        let interface = GrantFile::from_toml(&grant.to_toml().unwrap())
            .unwrap()
            .to_interface()
            .unwrap();
        let point = interface
            .modes
            .iter()
            .find(|mode| mode.name == "Holder!Point")
            .expect("nested mode path should survive the grant round trip");
        assert_eq!(point.symbol(), "nested!M!Holder!Point");
    }

    #[test]
    fn encodes_interface_modes() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "interfaces"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT I, P;
  NEWMODE I = INTERFACE
    GRANT Get;
    Get: PROC () RETURNS (INT) END;
  END I;
  NEWMODE P = MODULE SPEC
    GRANT Get;
    Get: PROC () RETURNS (INT) END;
  END P;
  NEWMODE P = MODULE BODY IMPLEMENTS I
    DCL value INT INIT := 3;
    Get: PROC () RETURNS (INT);
      RESULT value;
    END Get;
  END P;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some("interfaces".to_owned());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let interface = grant.modes.iter().find(|mode| mode.name == "I").unwrap();
        assert_eq!(interface.kind, GrantModeKind::Interface);
        let implementation = grant.modes.iter().find(|mode| mode.name == "P").unwrap();
        assert_eq!(implementation.implements.len(), 1);
        let interface_info = GrantFile::from_toml(&grant.to_toml().unwrap())
            .unwrap()
            .to_interface()
            .unwrap();
        let info = interface_info
            .modes
            .iter()
            .find(|mode| mode.name == "I")
            .unwrap();
        assert_eq!(info.kind, program::ModeInfoKind::Interface);
        let implementation = interface_info
            .modes
            .iter()
            .find(|mode| mode.name == "P")
            .unwrap();
        assert_eq!(implementation.implements.len(), 1);
    }

    #[test]
    fn structure_array_types_round_trip() {
        let symbol = "pkg!M!Point".to_owned();
        let mode = GrantMode {
            name: "Point".to_owned(),
            implements: Vec::new(),
            module: Some("M".to_owned()),
            package: Some("pkg".to_owned()),
            kind: GrantModeKind::Struct,
            fields: vec![GrantField {
                name: "x".to_owned(),
                mode: "INT".to_owned(),
            }],
            elements: Vec::new(),
            components: Vec::new(),
        };
        let mut definitions = std::collections::HashMap::new();
        definitions.insert(symbol.clone(), &mode);
        let ty = program::ExternalType::Array {
            low: 0,
            high: 1,
            element: Box::new(program::ExternalType::Mode(symbol)),
        };
        let text = external_type_name(&ty);
        assert_eq!(text, "ARRAY(0:1) pkg!M!Point");
        assert_eq!(parse_external_type(&text, &definitions).unwrap(), ty);
    }

    #[test]
    fn records_parameter_passing() {
        let manifest = Manifest::parse(
            r#"
[package]
name = "passing"

[build]
entry = "src/main.ch"
"#,
        )
        .unwrap();
        let tokens = lexer::tokenize(
            r#"
M: MODULE
  GRANT F;
  F: PROC (a INT, b INT OUT, c INT INOUT, d INT LOC);
  END F;
END M;
"#,
        )
        .unwrap();
        let mut program = parser::parse(&tokens).unwrap();
        for module in &mut program.modules {
            module.package_name = Some(manifest.package.name.clone());
        }
        let grant = GrantFile::from_package(&manifest, &program).unwrap();
        let export = grant.exports.iter().find(|e| e.name == "F").unwrap();
        assert_eq!(export.passing, ["in", "out", "inout", "loc"]);
        let parsed = GrantFile::from_toml(&grant.to_toml().unwrap()).unwrap();
        let interface = parsed.to_interface().unwrap();
        let proc_ty = match &interface.exports[0].kind {
            ExportInfoKind::Proc { passing, .. } => passing,
            other => panic!("expected procedure export, found {other:?}"),
        };
        assert_eq!(
            proc_ty,
            &[Passing::In, Passing::Out, Passing::InOut, Passing::Loc]
        );
    }

    #[test]
    fn rejects_unknown_schema() {
        let text = r#"
schema = 999
package = "x"
exports = []
"#;
        let error = GrantFile::from_toml(text).unwrap_err();
        assert!(error.contains("unsupported grant-file schema"));
    }
}
