use crate::ast::{
    Declaration, ModeRef, Module, ModuleKind, NameRef, Param, Passing, Program, VisibilityKind,
};
use crate::lexer::Span;
use crate::modes::{FieldInfo, ModeTable, StructMode};
use crate::types::{self, ModeId, ProcType, Type};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code: classify(&message),
            span,
            message,
        }
    }

    pub fn coded(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

fn classify(message: &str) -> &'static str {
    if message.contains("not implemented yet") {
        "E0409"
    } else if message.contains("unknown mode") {
        "E0401"
    } else if message.contains("duplicate") {
        "E0402"
    } else if message.contains("spec")
        || message.contains("body")
        || message.contains("GRANT")
        || message.contains("grant")
        || message.contains("exported")
    {
        "E0406"
    } else {
        "E0400"
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Default)]
pub struct ModuleInterface {
    pub vars: HashMap<String, Type>,
    pub procs: HashMap<String, ProcType>,
    pub modes: HashMap<String, Type>,
}

#[derive(Debug, Clone)]
pub enum ExportKind {
    Var(Type),
    Proc(ProcType),
    Mode(ModeId),
}

#[derive(Debug, Clone)]
pub struct ExportEntry {
    /// Index of the module that owns the implementation. For a spec export this
    /// is the paired body module.
    pub module: usize,
    pub package: usize,
    pub package_name: Option<String>,
    pub module_name: Option<String>,
    pub name: String,
    /// Exported prefix (`GRANT ... PREFIXED ...`). Empty means the automatic
    /// module/package qualifiers.
    pub prefix: Vec<String>,
    pub kind: ExportKind,
    /// `GRANT ... TO ...` friend modules, if restricted.
    pub friends: Option<Vec<String>>,
}

impl ExportEntry {
    pub fn qualified_names(&self) -> Vec<String> {
        if self.prefix.is_empty() {
            let mut names = vec![self.name.clone()];
            if let Some(module) = &self.module_name {
                names.push(format!("{module}!{}", self.name));
                if let Some(package) = &self.package_name {
                    names.push(format!("{package}!{module}!{}", self.name));
                }
            }
            names
        } else {
            let prefix = self.prefix.join("!");
            let mut names = vec![format!("{prefix}!{}", self.name)];
            if let Some(package) = &self.package_name {
                names.push(format!("{package}!{prefix}!{}", self.name));
            }
            names
        }
    }

    fn qualified_prefixes(&self) -> Vec<Vec<String>> {
        if self.prefix.is_empty() {
            let mut prefixes = Vec::new();
            if let Some(module) = &self.module_name {
                prefixes.push(vec![module.clone()]);
                if let Some(package) = &self.package_name {
                    prefixes.push(vec![package.clone(), module.clone()]);
                }
            }
            prefixes
        } else {
            let mut prefixes = vec![self.prefix.clone()];
            if let Some(package) = &self.package_name {
                let mut package_prefix = vec![package.clone()];
                package_prefix.extend(self.prefix.iter().cloned());
                prefixes.push(package_prefix);
            }
            prefixes
        }
    }

    /// Whether the export's `TO` restriction permits this module.
    pub fn is_accessible_to(&self, package: Option<&str>, module: Option<&str>) -> bool {
        let Some(friends) = &self.friends else {
            return true;
        };
        let Some(module) = module else {
            return false;
        };
        let qualified = match package {
            Some(package) => format!("{package}!{module}"),
            None => module.to_owned(),
        };
        friends
            .iter()
            .any(|friend| friend == module || friend == &qualified)
    }
}

/// Resolve a `SEIZE` name against the program's exports.
pub fn resolve_export<'a>(
    exports: &'a [ExportEntry],
    name: &NameRef,
    package: Option<&str>,
    module: Option<&str>,
) -> Result<&'a ExportEntry, String> {
    let candidates: Vec<&ExportEntry> = exports
        .iter()
        .filter(|export| {
            let prefix_matches = if name.prefix.is_empty() {
                export.prefix.is_empty()
            } else {
                export
                    .qualified_prefixes()
                    .iter()
                    .any(|prefix| prefix == &name.prefix)
            };
            export.name == name.name && prefix_matches && export.is_accessible_to(package, module)
        })
        .collect();

    match candidates.as_slice() {
        [] => Err(format!(
            "cannot SEIZE `{}`: no visible module grants it",
            name.qualified()
        )),
        [export] => Ok(export),
        _ => {
            let qualified = candidates
                .iter()
                .flat_map(|export| export.qualified_names())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "cannot SEIZE `{}`: ambiguous; use a qualified name ({qualified})",
                name.qualified()
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub name: String,
    pub module_name: Option<String>,
    pub package_name: Option<String>,
    pub prefix: Vec<String>,
    pub kind: ExportInfoKind,
}

#[derive(Debug, Clone)]
pub enum ExportInfoKind {
    Var(Type),
    Proc {
        params: Vec<Type>,
        result: Option<Type>,
    },
}

/// Static structure shared by the checker, interpreter, and grant emitter:
/// the interface of every module and the resolved export table.
#[derive(Debug, Clone)]
pub struct ProgramInfo {
    pub interfaces: Vec<ModuleInterface>,
    pub exports: Vec<ExportEntry>,
    pub modes: ModeTable,
    pub body_for_spec: HashMap<usize, usize>,
    pub spec_for_body: HashMap<usize, usize>,
}

pub fn analyze(program: &Program) -> Result<ProgramInfo, Error> {
    let (mut modes, module_modes) = build_modes(program)?;
    let interfaces: Vec<ModuleInterface> = program
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| collect_interface(module, &module_modes[index]))
        .collect::<Result<_, _>>()?;

    let mut body_by_name: HashMap<(usize, String), usize> = HashMap::new();
    let mut spec_by_name: HashMap<(usize, String), usize> = HashMap::new();

    for (index, module) in program.modules.iter().enumerate() {
        let Some(name) = &module.name else {
            if module.kind == ModuleKind::Spec {
                return Err(Error::new(module.span, "spec module must be named"));
            }
            continue;
        };
        let target = match module.kind {
            ModuleKind::Body => &mut body_by_name,
            ModuleKind::Spec => &mut spec_by_name,
        };
        if target
            .insert((module.package, name.clone()), index)
            .is_some()
        {
            return Err(Error::new(
                module.span,
                format!("duplicate module name `{name}`"),
            ));
        }
    }

    let mut exports: Vec<ExportEntry> = Vec::new();
    let mut body_for_spec = HashMap::new();
    let mut spec_for_body = HashMap::new();

    for ((package, name), spec_index) in &spec_by_name {
        let Some(body_index) = body_by_name.get(&(*package, name.clone())).copied() else {
            return Err(Error::new(
                program.modules[*spec_index].span,
                format!("spec module `{name}` has no body"),
            ));
        };
        body_for_spec.insert(*spec_index, body_index);
        spec_for_body.insert(body_index, *spec_index);

        let spec = &program.modules[*spec_index];
        let body = &program.modules[body_index];

        if has_grant(body) {
            return Err(Error::new(
                body.span,
                format!("module `{name}` has a SPEC; GRANT must be in the spec module"),
            ));
        }

        check_interface(&interfaces[*spec_index], &interfaces[body_index], spec.span)?;
        add_grants(
            spec,
            &interfaces[*spec_index],
            body_index,
            &mut exports,
            &mut modes,
        )?;
    }

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind == ModuleKind::Body && !spec_for_body.contains_key(&index) {
            add_grants(module, &interfaces[index], index, &mut exports, &mut modes)?;
        }
    }

    Ok(ProgramInfo {
        interfaces,
        exports,
        modes,
        body_for_spec,
        spec_for_body,
    })
}

/// The public interface of the program, sorted by name.
pub fn exports(program: &Program) -> Result<Vec<ExportInfo>, Error> {
    let mut exports: Vec<ExportInfo> = analyze(program)?
        .exports
        .into_iter()
        .filter(|export| !matches!(export.kind, ExportKind::Mode(_)))
        .map(|export| {
            let kind = match export.kind {
                ExportKind::Var(ty) => ExportInfoKind::Var(ty),
                ExportKind::Proc(signature) => ExportInfoKind::Proc {
                    params: signature.params,
                    result: signature.result,
                },
                ExportKind::Mode(_) => unreachable!("mode exports are filtered out"),
            };
            ExportInfo {
                name: export.name,
                module_name: export.module_name,
                package_name: export.package_name,
                prefix: export.prefix,
                kind,
            }
        })
        .collect();
    exports.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.package_name.cmp(&right.package_name))
            .then_with(|| left.module_name.cmp(&right.module_name))
    });
    Ok(exports)
}

fn build_modes(program: &Program) -> Result<(ModeTable, Vec<HashMap<String, Type>>), Error> {
    let mut table = ModeTable::default();
    let mut module_modes: Vec<HashMap<String, Type>> = vec![HashMap::new(); program.modules.len()];

    for (index, module) in program.modules.iter().enumerate() {
        for declaration in &module.declarations {
            let Declaration::Mode(mode_def) = declaration else {
                continue;
            };

            let ty = match &mode_def.body {
                crate::ast::ModeDefinitionBody::Struct(fields) => {
                    let mut field_infos = Vec::with_capacity(fields.len());
                    let mut names = std::collections::HashSet::new();
                    for field in fields {
                        if !names.insert(field.name.clone()) {
                            return Err(Error::new(
                                field.span,
                                format!("duplicate field `{}`", field.name),
                            ));
                        }
                        let ty = resolve_mode_with(&field.mode, &module_modes[index])?;
                        field_infos.push(FieldInfo {
                            name: field.name.clone(),
                            ty,
                        });
                    }
                    let id = ModeId(table.modes.len());
                    table.modes.push(StructMode {
                        name: mode_def.name.clone(),
                        package: module.package,
                        module: index,
                        fields: field_infos,
                        forbidden: std::collections::HashSet::new(),
                    });
                    Type::Struct(id)
                }
                crate::ast::ModeDefinitionBody::Alias(mode) => {
                    resolve_mode_with(mode, &module_modes[index])?
                }
            };

            if module_modes[index]
                .insert(mode_def.name.clone(), ty)
                .is_some()
            {
                return Err(Error::new(
                    mode_def.span,
                    format!("duplicate mode `{}`", mode_def.name),
                ));
            }
        }
    }

    Ok((table, module_modes))
}

fn resolve_mode_with(mode: &ModeRef, mode_names: &HashMap<String, Type>) -> Result<Type, Error> {
    mode_names
        .get(&mode.name)
        .copied()
        .or_else(|| types::resolve_mode(&mode.name))
        .ok_or_else(|| {
            Error::new(
                mode.span,
                format!(
                    "unknown mode `{}`; only scalar modes and previously defined modes are supported in structure fields",
                    mode.name
                ),
            )
        })
}

pub fn collect_interface(
    module: &Module,
    mode_names: &HashMap<String, Type>,
) -> Result<ModuleInterface, Error> {
    let mut interface = ModuleInterface {
        modes: mode_names.clone(),
        ..ModuleInterface::default()
    };

    // A mode brought in by `SEIZE` is not known while interfaces are collected;
    // the mode checker resolves those names later. Only those specific names may be
    // skipped here, so genuine typos still fail.
    let seized: HashSet<&str> = module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Visibility(visibility) if visibility.kind == VisibilityKind::Seize => {
                Some(visibility)
            }
            _ => None,
        })
        .flat_map(|visibility| visibility.names.iter())
        .map(|name| name.name.as_str())
        .collect();

    for declaration in &module.declarations {
        match declaration {
            Declaration::Var(list) => {
                for var in list {
                    let ty = match resolve_mode_with(&var.mode, mode_names) {
                        Ok(ty) => ty,
                        Err(_) if seized.contains(var.mode.name.as_str()) => continue,
                        Err(error) => return Err(error),
                    };
                    if interface.vars.insert(var.name.clone(), ty).is_some()
                        || interface.procs.contains_key(&var.name)
                    {
                        return Err(Error::new(
                            var.span,
                            format!("duplicate declaration of `{}`", var.name),
                        ));
                    }
                }
            }
            Declaration::Proc(proc_def) => {
                let signature = match signature_from_params_with(
                    &proc_def.params,
                    &proc_def.result,
                    mode_names,
                ) {
                    Ok(signature) => signature,
                    Err(_) if has_seized_mode(&proc_def.params, &proc_def.result, &seized) => {
                        continue
                    }
                    Err(error) => return Err(error),
                };
                if interface
                    .procs
                    .insert(proc_def.name.clone(), signature)
                    .is_some()
                    || interface.vars.contains_key(&proc_def.name)
                {
                    return Err(Error::new(
                        proc_def.span,
                        format!("duplicate declaration of `{}`", proc_def.name),
                    ));
                }
            }
            Declaration::ProcSignature(signature) => {
                let proc_sig = match signature_from_params_with(
                    &signature.params,
                    &signature.result,
                    mode_names,
                ) {
                    Ok(proc_sig) => proc_sig,
                    Err(_) if has_seized_mode(&signature.params, &signature.result, &seized) => {
                        continue
                    }
                    Err(error) => return Err(error),
                };
                if interface
                    .procs
                    .insert(signature.name.clone(), proc_sig)
                    .is_some()
                    || interface.vars.contains_key(&signature.name)
                {
                    return Err(Error::new(
                        signature.span,
                        format!("duplicate declaration of `{}`", signature.name),
                    ));
                }
            }
            Declaration::Mode(_) | Declaration::Visibility(_) => {}
        }
    }

    Ok(interface)
}

fn has_seized_mode(params: &[Param], result: &Option<ModeRef>, seized: &HashSet<&str>) -> bool {
    params
        .iter()
        .any(|param| seized.contains(param.mode.name.as_str()))
        || result
            .as_ref()
            .is_some_and(|mode| seized.contains(mode.name.as_str()))
}

pub fn signature_from_params(
    params: &[Param],
    result: &Option<ModeRef>,
) -> Result<ProcType, Error> {
    signature_from_params_with(params, result, &HashMap::new())
}

pub fn signature_from_params_with(
    params: &[Param],
    result: &Option<ModeRef>,
    mode_names: &HashMap<String, Type>,
) -> Result<ProcType, Error> {
    let mut param_types = Vec::with_capacity(params.len());
    let mut seen = HashSet::new();

    for param in params {
        if param.passing != Passing::In {
            return Err(Error::new(
                param.span,
                format!(
                    "parameter passing mode `{:?}` is not implemented yet",
                    param.passing
                ),
            ));
        }
        if !seen.insert(param.name.as_str()) {
            return Err(Error::new(
                param.span,
                format!("duplicate parameter `{}`", param.name),
            ));
        }
        param_types.push(resolve_mode_with(&param.mode, mode_names)?);
    }

    let result_type = match result {
        Some(mode) => Some(resolve_mode_with(mode, mode_names)?),
        None => None,
    };

    Ok(ProcType {
        params: param_types,
        result: result_type,
    })
}

pub fn resolve_mode(mode: &ModeRef) -> Result<Type, Error> {
    types::resolve_mode(&mode.name).ok_or_else(|| {
        Error::new(
            mode.span,
            format!(
                "unknown mode `{}`; v0 supports SHORT_INT, INT, LONG_INT, and BOOL",
                mode.name
            ),
        )
    })
}

fn has_grant(module: &Module) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::Visibility(v) if v.kind == VisibilityKind::Grant)
    })
}

fn add_grants(
    module: &Module,
    interface: &ModuleInterface,
    export_module: usize,
    exports: &mut Vec<ExportEntry>,
    modes: &mut ModeTable,
) -> Result<(), Error> {
    let module_name = module.name.clone();
    let package = module.package;
    let package_name = module.package_name.clone();
    let mut granted = HashSet::new();

    for declaration in &module.declarations {
        let Declaration::Visibility(visibility) = declaration else {
            continue;
        };
        if visibility.kind != VisibilityKind::Grant {
            continue;
        }

        let friends = if visibility.friends.is_empty() {
            None
        } else {
            Some(
                visibility
                    .friends
                    .iter()
                    .map(|friend| friend.qualified())
                    .collect::<Vec<_>>(),
            )
        };
        let export_prefix = match &visibility.prefix {
            None => Vec::new(),
            Some(parts) if parts.is_empty() => module
                .name
                .clone()
                .map(|name| vec![name])
                .unwrap_or_default(),
            Some(parts) => parts.clone(),
        };

        let names: Vec<(String, Span)> = if visibility.all {
            let mut names: Vec<String> = interface
                .vars
                .keys()
                .chain(interface.procs.keys())
                .chain(
                    interface
                        .modes
                        .iter()
                        .filter(|(_, ty)| matches!(ty, Type::Struct(_)))
                        .map(|(name, _)| name),
                )
                .cloned()
                .collect();
            names.sort();
            names.dedup();
            names
                .into_iter()
                .map(|name| (name, visibility.span))
                .collect()
        } else {
            let mut names = Vec::with_capacity(visibility.names.len());
            for name in &visibility.names {
                if !name.prefix.is_empty() {
                    return Err(Error::new(
                        name.span,
                        "GRANT prefixes are not implemented yet",
                    ));
                }
                names.push((name.name.clone(), name.span));
            }
            names
        };

        for (name, span) in names {
            let grant_key = format!("{}!{name}", export_prefix.join("!"));
            if !granted.insert(grant_key) {
                return Err(Error::new(
                    span,
                    format!("`{name}` is granted more than once"),
                ));
            }

            let kind = if let Some(mode) = interface.modes.get(&name) {
                let Type::Struct(id) = mode else {
                    return Err(Error::new(
                        span,
                        format!("cannot GRANT `{name}`: only structure modes are supported as mode grants"),
                    ));
                };
                if let Some(forbid) = &visibility.forbid {
                    let mode_info = modes.get_mut(*id);
                    let fields: Vec<String> = if forbid.all {
                        mode_info
                            .fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect()
                    } else {
                        forbid.fields.clone()
                    };
                    for field in fields {
                        if !mode_info.fields.iter().any(|info| info.name == field) {
                            return Err(Error::new(
                                span,
                                format!("FORBID field `{field}` is not a field of `{name}`"),
                            ));
                        }
                        mode_info.forbidden.insert(field);
                    }
                }
                ExportKind::Mode(*id)
            } else if let Some(ty) = interface.vars.get(&name) {
                if visibility.forbid.is_some() {
                    return Err(Error::new(
                        span,
                        format!("FORBID requires a structure mode, not `{name}`"),
                    ));
                }
                ExportKind::Var(*ty)
            } else if let Some(signature) = interface.procs.get(&name) {
                if visibility.forbid.is_some() {
                    return Err(Error::new(
                        span,
                        format!("FORBID requires a structure mode, not `{name}`"),
                    ));
                }
                ExportKind::Proc(signature.clone())
            } else {
                return Err(Error::new(
                    span,
                    format!("cannot GRANT `{name}`: not declared in this module"),
                ));
            };

            exports.push(ExportEntry {
                module: export_module,
                package,
                package_name: package_name.clone(),
                module_name: module_name.clone(),
                name,
                prefix: export_prefix.clone(),
                kind,
                friends: friends.clone(),
            });
        }
    }

    Ok(())
}

fn check_interface(
    spec: &ModuleInterface,
    body: &ModuleInterface,
    span: Span,
) -> Result<(), Error> {
    for (name, ty) in &spec.vars {
        match body.vars.get(name) {
            Some(body_ty) if body_ty == ty => {}
            Some(_) => {
                return Err(Error::new(
                    span,
                    format!("spec variable `{name}` has a different mode in the body"),
                ));
            }
            None => {
                return Err(Error::new(
                    span,
                    format!("body is missing spec variable `{name}`"),
                ));
            }
        }
    }

    for (name, signature) in &spec.procs {
        match body.procs.get(name) {
            Some(body_signature) if body_signature == signature => {}
            Some(_) => {
                return Err(Error::new(
                    span,
                    format!("spec procedure `{name}` has a different signature in the body"),
                ));
            }
            None => {
                return Err(Error::new(
                    span,
                    format!("body is missing spec procedure `{name}`"),
                ));
            }
        }
    }

    Ok(())
}
