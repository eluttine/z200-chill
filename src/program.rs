use crate::ast::{
    Action, Declaration, ModeRef, Module, ModuleKind, NameRef, Param, Passing, PrefixClause,
    Program, VisibilityKind,
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

/// A type as recorded in a portable interface. Structure modes are referenced
/// by their stable symbolic name (`package!Module!Mode`); local `ModeId`s are
/// never serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalType {
    Builtin(Type),
    Mode(String),
}

#[derive(Debug, Clone)]
pub struct ModeInfo {
    pub name: String,
    pub module_name: Option<String>,
    pub package_name: Option<String>,
    pub fields: Vec<(String, ExternalType)>,
    pub forbidden: Vec<String>,
}

impl ModeInfo {
    pub fn symbol(&self) -> String {
        crate::modes::mode_symbol(
            self.package_name.as_deref(),
            self.module_name.as_deref(),
            &self.name,
        )
    }
}

#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub name: String,
    pub module_name: Option<String>,
    pub package_name: Option<String>,
    pub prefix: Vec<String>,
    pub kind: ExportInfoKind,
    pub friends: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum ExportInfoKind {
    Var(ExternalType),
    Proc {
        params: Vec<ExternalType>,
        result: Option<ExternalType>,
    },
    Mode(ModeInfo),
}

/// Static structure shared by the checker, interpreter, and grant emitter:
/// the interface of every module and the resolved export table.
#[derive(Debug, Clone)]
pub struct ProgramInfo {
    pub interfaces: Vec<ModuleInterface>,
    pub exports: Vec<ExportEntry>,
    pub modes: ModeTable,
    /// Local mode names per module span, including nested modules.
    pub module_modes_by_span: HashMap<Span, HashMap<String, Type>>,
    pub body_for_spec: HashMap<usize, usize>,
    pub spec_for_body: HashMap<usize, usize>,
}

pub fn analyze(program: &Program) -> Result<ProgramInfo, Error> {
    if let Some(remote) = program.remotes.first() {
        return Err(Error::coded(
            "E0701",
            remote.span,
            "remote pieces are not expanded yet",
        ));
    }
    for module in &program.modules {
        if let Some(context) = find_remote_context(module) {
            return Err(Error::coded(
                "E0701",
                context.span,
                "remote context pieces are not expanded yet",
            ));
        }
    }

    let (mut modes, module_modes, module_modes_by_span) = build_modes(program)?;
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
            &HashMap::new(),
        )?;
    }

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind != ModuleKind::Body {
            continue;
        }
        let spec = spec_for_body
            .get(&index)
            .map(|spec_index| &program.modules[*spec_index]);
        let mut sources: Vec<&Module> = Vec::new();
        if let Some(spec) = spec {
            sources.push(spec);
        }
        sources.push(module);
        check_nested_specs(&sources, module, &module_modes_by_span)?;
    }

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind == ModuleKind::Body && !spec_for_body.contains_key(&index) {
            let reach_vars =
                collect_child_grants(module, &module_modes[index], &module_modes_by_span)?;
            add_grants(
                module,
                &interfaces[index],
                index,
                &mut exports,
                &mut modes,
                &reach_vars,
            )?;
        }
    }

    for (index, module) in program.modules.iter().enumerate() {
        if !module.contexts.is_empty() {
            check_contexts(module, index, &exports)?;
        }
    }

    Ok(ProgramInfo {
        interfaces,
        exports,
        modes,
        module_modes_by_span,
        body_for_spec,
        spec_for_body,
    })
}

/// The portable public interface of the program, sorted by name.
pub fn exports(program: &Program) -> Result<Vec<ExportInfo>, Error> {
    let info = analyze(program)?;
    let modes = info.modes;
    let mut exports: Vec<ExportInfo> = info
        .exports
        .into_iter()
        .map(|export| {
            let kind = match export.kind {
                ExportKind::Var(ty) => ExportInfoKind::Var(portable_type(ty, &modes)),
                ExportKind::Proc(signature) => ExportInfoKind::Proc {
                    params: signature
                        .params
                        .into_iter()
                        .map(|ty| portable_type(ty, &modes))
                        .collect(),
                    result: signature.result.map(|ty| portable_type(ty, &modes)),
                },
                ExportKind::Mode(id) => {
                    let mode = modes.get(id);
                    let mut forbidden: Vec<String> = mode.forbidden.iter().cloned().collect();
                    forbidden.sort();
                    ExportInfoKind::Mode(ModeInfo {
                        name: mode.name.clone(),
                        module_name: mode.module_name.clone(),
                        package_name: mode.package_name.clone(),
                        fields: mode
                            .fields
                            .iter()
                            .map(|field| (field.name.clone(), portable_type(field.ty, &modes)))
                            .collect(),
                        forbidden,
                    })
                }
            };
            ExportInfo {
                name: export.name,
                module_name: export.module_name,
                package_name: export.package_name,
                prefix: export.prefix,
                kind,
                friends: export.friends,
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

pub fn portable_type(ty: Type, modes: &ModeTable) -> ExternalType {
    match ty {
        Type::Struct(id) => ExternalType::Mode(modes.get(id).symbol()),
        other => ExternalType::Builtin(other),
    }
}

/// A dependency's portable interface: mode definitions needed to interpret its
/// exported types, plus the names it actually grants.
#[derive(Debug, Clone, Default)]
pub struct ExternalInterface {
    pub modes: Vec<ModeInfo>,
    pub exports: Vec<ExportInfo>,
}

/// Symbolic mode names encountered in external interfaces, mapped to freshly
/// allocated local `ModeId`s.
#[derive(Debug, Default)]
pub struct ExternalModeIds {
    ids: HashMap<String, ModeId>,
}

impl ExternalModeIds {
    pub fn get(&self, symbol: &str) -> Option<ModeId> {
        self.ids.get(symbol).copied()
    }
}

/// Instantiate structure modes from external grant files into `modes`.
pub fn add_external_modes(
    modes: &mut ModeTable,
    external: &[ExternalInterface],
) -> Result<ExternalModeIds, String> {
    let mut ids = ExternalModeIds::default();
    let mut definitions: Vec<&ModeInfo> = Vec::new();

    for interface in external {
        for mode in &interface.modes {
            let symbol = mode.symbol();
            if ids.ids.contains_key(&symbol) {
                continue;
            }
            let id = ModeId(modes.modes.len());
            ids.ids.insert(symbol.clone(), id);
            modes.modes.push(StructMode {
                name: mode.name.clone(),
                // External modes are never the local defining module.
                package: usize::MAX,
                module: usize::MAX,
                package_name: mode.package_name.clone(),
                module_name: mode.module_name.clone(),
                fields: Vec::new(),
                forbidden: HashSet::new(),
            });
            definitions.push(mode);
        }
    }

    detect_mode_cycles(&definitions, &ids)?;

    for mode in definitions {
        let id = ids.ids[&mode.symbol()];
        let mut fields = Vec::with_capacity(mode.fields.len());
        let mut names = HashSet::new();
        for (name, ty) in &mode.fields {
            if !names.insert(name.clone()) {
                return Err(format!(
                    "duplicate field `{name}` in external mode `{}`",
                    mode.symbol()
                ));
            }
            let ty = resolve_external_type(ty, &ids)?;
            fields.push(FieldInfo {
                name: name.clone(),
                ty,
            });
        }
        let mut forbidden = HashSet::new();
        for name in &mode.forbidden {
            if !fields.iter().any(|field| &field.name == name) {
                return Err(format!(
                    "external mode `{}` forbids unknown field `{name}`",
                    mode.symbol()
                ));
            }
            forbidden.insert(name.clone());
        }
        let entry = modes.get_mut(id);
        entry.fields = fields;
        entry.forbidden = forbidden;
    }

    Ok(ids)
}

fn detect_mode_cycles(definitions: &[&ModeInfo], ids: &ExternalModeIds) -> Result<(), String> {
    fn visit(
        index: usize,
        definitions: &[&ModeInfo],
        by_symbol: &HashMap<String, usize>,
        ids: &ExternalModeIds,
        visiting: &mut HashSet<usize>,
        done: &mut HashSet<usize>,
    ) -> Result<(), String> {
        if done.contains(&index) {
            return Ok(());
        }
        if !visiting.insert(index) {
            return Err(format!(
                "cyclic structure mode `{}` in external interface",
                definitions[index].symbol()
            ));
        }
        for (_, ty) in &definitions[index].fields {
            let ExternalType::Mode(reference) = ty else {
                continue;
            };
            if ids.get(reference).is_none() {
                return Err(format!(
                    "external mode `{}` references unknown mode `{reference}`",
                    definitions[index].symbol()
                ));
            }
            if let Some(next) = by_symbol.get(reference) {
                visit(*next, definitions, by_symbol, ids, visiting, done)?;
            }
        }
        visiting.remove(&index);
        done.insert(index);
        Ok(())
    }

    let by_symbol: HashMap<String, usize> = definitions
        .iter()
        .enumerate()
        .map(|(index, mode)| (mode.symbol(), index))
        .collect();
    let mut done = HashSet::new();
    for index in 0..definitions.len() {
        let mut visiting = HashSet::new();
        visit(
            index,
            definitions,
            &by_symbol,
            ids,
            &mut visiting,
            &mut done,
        )?;
    }
    Ok(())
}

pub fn add_external_exports(
    exports: &mut Vec<ExportEntry>,
    external: &[ExternalInterface],
    ids: &ExternalModeIds,
) -> Result<(), String> {
    for interface in external {
        for info in &interface.exports {
            let kind = match &info.kind {
                ExportInfoKind::Var(ty) => ExportKind::Var(resolve_external_type(ty, ids)?),
                ExportInfoKind::Proc { params, result } => ExportKind::Proc(ProcType {
                    params: params
                        .iter()
                        .map(|ty| resolve_external_type(ty, ids))
                        .collect::<Result<_, _>>()?,
                    result: match result {
                        Some(ty) => Some(resolve_external_type(ty, ids)?),
                        None => None,
                    },
                }),
                ExportInfoKind::Mode(mode) => {
                    let id = ids.get(&mode.symbol()).ok_or_else(|| {
                        format!("external mode `{}` was not materialized", mode.symbol())
                    })?;
                    ExportKind::Mode(id)
                }
            };
            exports.push(ExportEntry {
                module: usize::MAX,
                package: usize::MAX,
                package_name: info.package_name.clone(),
                module_name: info.module_name.clone(),
                name: info.name.clone(),
                prefix: info.prefix.clone(),
                kind,
                friends: info.friends.clone(),
            });
        }
    }
    Ok(())
}

fn resolve_external_type(ty: &ExternalType, ids: &ExternalModeIds) -> Result<Type, String> {
    match ty {
        ExternalType::Builtin(Type::Struct(_)) => {
            Err("internal: structure modes must use symbolic references".to_owned())
        }
        ExternalType::Builtin(ty) => Ok(*ty),
        ExternalType::Mode(symbol) => ids
            .get(symbol)
            .map(Type::Struct)
            .ok_or_else(|| format!("external interface references unknown mode `{symbol}`")),
    }
}

type ModeBuildResult = (
    ModeTable,
    Vec<HashMap<String, Type>>,
    HashMap<Span, HashMap<String, Type>>,
);

fn build_modes(program: &Program) -> Result<ModeBuildResult, Error> {
    let mut table = ModeTable::default();
    let mut top_maps = Vec::with_capacity(program.modules.len());
    let mut span_maps = HashMap::new();

    for (index, module) in program.modules.iter().enumerate() {
        let mut local = HashMap::new();
        let mut resolution = HashMap::new();
        process_module_modes(
            module,
            index,
            module.package,
            module.package_name.clone(),
            &mut local,
            &mut resolution,
            &mut table,
            &mut span_maps,
        )?;
        top_maps.push(local);
    }

    Ok((table, top_maps, span_maps))
}

#[allow(clippy::too_many_arguments)]
fn process_module_modes(
    module: &Module,
    top: usize,
    top_package: usize,
    top_package_name: Option<String>,
    local: &mut HashMap<String, Type>,
    resolution: &mut HashMap<String, Type>,
    table: &mut ModeTable,
    span_maps: &mut HashMap<Span, HashMap<String, Type>>,
) -> Result<(), Error> {
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
                    let ty = resolve_mode_with(&field.mode, resolution)?;
                    field_infos.push(FieldInfo {
                        name: field.name.clone(),
                        ty,
                    });
                }
                let id = ModeId(table.modes.len());
                table.modes.push(StructMode {
                    name: mode_def.name.clone(),
                    package: top_package,
                    module: top,
                    package_name: top_package_name.clone(),
                    module_name: module.name.clone(),
                    fields: field_infos,
                    forbidden: std::collections::HashSet::new(),
                });
                Type::Struct(id)
            }
            crate::ast::ModeDefinitionBody::Alias(mode) => resolve_mode_with(mode, resolution)?,
        };

        if local.insert(mode_def.name.clone(), ty).is_some() {
            return Err(Error::new(
                mode_def.span,
                format!("duplicate mode `{}`", mode_def.name),
            ));
        }
        resolution.insert(mode_def.name.clone(), ty);
    }

    span_maps.insert(module.span, local.clone());

    walk_nested_modules(&module.actions, &mut |child| {
        let mut child_local = HashMap::new();
        let mut child_resolution = resolution.clone();
        process_module_modes(
            child,
            top,
            top_package,
            top_package_name.clone(),
            &mut child_local,
            &mut child_resolution,
            table,
            span_maps,
        )
    })
}

fn find_remote_context(module: &Module) -> Option<&crate::ast::Context> {
    if let Some(context) = module
        .contexts
        .iter()
        .find(|context| context.remote.is_some())
    {
        return Some(context);
    }
    let mut found = None;
    let _ = walk_nested_modules(&module.actions, &mut |child| {
        if found.is_none() {
            found = find_remote_context(child);
        }
        Ok(())
    });
    found
}

fn walk_nested_modules<'a>(
    actions: &'a [Action],
    visit: &mut impl FnMut(&'a Module) -> Result<(), Error>,
) -> Result<(), Error> {
    for action in actions {
        match action {
            Action::Module(module) => visit(module)?,
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    walk_nested_modules(&branch.body, visit)?;
                }
                if let Some(actions) = else_actions {
                    walk_nested_modules(actions, visit)?;
                }
            }
            Action::Do { body, .. } => walk_nested_modules(body, visit)?,
            Action::Case {
                alternatives,
                else_actions,
                ..
            } => {
                for alternative in alternatives {
                    walk_nested_modules(&alternative.body, visit)?;
                }
                if let Some(actions) = else_actions {
                    walk_nested_modules(actions, visit)?;
                }
            }
            Action::Labelled { action, .. } => {
                walk_nested_modules(std::slice::from_ref(action), visit)?
            }
            Action::Begin { actions, .. } => walk_nested_modules(actions, visit)?,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn resolve_mode_with(
    mode: &ModeRef,
    mode_names: &HashMap<String, Type>,
) -> Result<Type, Error> {
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
            Declaration::Mode(_) | Declaration::Visibility(_) | Declaration::SpecModule(_) => {}
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

/// Check context-list quasi declarations against the grants visible in the
/// module's enclosing reach. Contexts have no runtime effect; in this complete
/// program model they act as static assertions about the surrounding reach.
fn check_contexts(module: &Module, index: usize, exports: &[ExportEntry]) -> Result<(), Error> {
    let empty_modes = HashMap::new();
    let mut declared_names = HashSet::new();
    let mut vars: Vec<(String, Type, Span)> = Vec::new();
    let mut procs: Vec<(String, ProcType, Span)> = Vec::new();

    for (context_index, context) in module.contexts.iter().enumerate() {
        let mut current_names = HashSet::new();
        for declaration in &context.declarations {
            match declaration {
                Declaration::Var(list) => {
                    for var in list {
                        let ty = resolve_mode_with(&var.mode, &empty_modes)?;
                        if !declared_names.insert(var.name.clone()) {
                            return Err(Error::new(
                                var.span,
                                format!("duplicate context declaration `{}`", var.name),
                            ));
                        }
                        current_names.insert(var.name.clone());
                        vars.push((var.name.clone(), ty, var.span));
                    }
                }
                Declaration::ProcSignature(signature) => {
                    let ty = signature_from_params_with(
                        &signature.params,
                        &signature.result,
                        &empty_modes,
                    )?;
                    if !declared_names.insert(signature.name.clone()) {
                        return Err(Error::new(
                            signature.span,
                            format!("duplicate context declaration `{}`", signature.name),
                        ));
                    }
                    current_names.insert(signature.name.clone());
                    procs.push((signature.name.clone(), ty, signature.span));
                }
                Declaration::Proc(proc_def) => {
                    let ty = signature_from_params_with(
                        &proc_def.params,
                        &proc_def.result,
                        &empty_modes,
                    )?;
                    if !declared_names.insert(proc_def.name.clone()) {
                        return Err(Error::new(
                            proc_def.span,
                            format!("duplicate context declaration `{}`", proc_def.name),
                        ));
                    }
                    current_names.insert(proc_def.name.clone());
                    procs.push((proc_def.name.clone(), ty, proc_def.span));
                }
                Declaration::Mode(_) => {
                    return Err(Error::new(
                        context.span,
                        "mode definitions in contexts are not implemented yet",
                    ));
                }
                Declaration::Visibility(_) | Declaration::SpecModule(_) => {}
            }
        }

        for declaration in &context.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if context_index == 0 {
                return Err(Error::new(
                    visibility.span,
                    "a context with no enclosing context cannot contain visibility statements",
                ));
            }
            if visibility.prefix.is_some()
                || !visibility.friends.is_empty()
                || visibility.forbid.is_some()
            {
                return Err(Error::new(
                    visibility.span,
                    "advanced clauses in context visibility are not implemented yet",
                ));
            }
            let targets: Vec<String> = if visibility.all {
                if visibility.kind == VisibilityKind::Grant {
                    current_names.iter().cloned().collect()
                } else {
                    declared_names.iter().cloned().collect()
                }
            } else {
                visibility
                    .names
                    .iter()
                    .map(|name| name.name.clone())
                    .collect()
            };
            for name in targets {
                if visibility.kind == VisibilityKind::Grant {
                    if !current_names.contains(&name) {
                        return Err(Error::new(
                            visibility.span,
                            format!("cannot GRANT `{name}` in a context: not declared here"),
                        ));
                    }
                } else if !declared_names.contains(&name) {
                    return Err(Error::new(
                        visibility.span,
                        format!("cannot SEIZE `{name}` in a context: no earlier declaration"),
                    ));
                } else {
                    declared_names.insert(name);
                }
            }
        }
    }

    for (name, declared, span) in vars {
        let export = context_export(exports, module, index, &name, span)?;
        match &export.kind {
            ExportKind::Var(actual) if *actual == declared => {}
            ExportKind::Var(actual) => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` has mode {} but the enclosing grant has mode {}",
                        declared.describe(),
                        actual.describe()
                    ),
                ));
            }
            _ => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` is a variable but the enclosing reach grants a different kind of name"
                    ),
                ));
            }
        }
    }

    for (name, declared, span) in procs {
        let export = context_export(exports, module, index, &name, span)?;
        match &export.kind {
            ExportKind::Proc(actual) if *actual == declared => {}
            ExportKind::Proc(_) => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` has a procedure signature that does not match the enclosing grant"
                    ),
                ));
            }
            _ => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` is a procedure but the enclosing reach grants a different kind of name"
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn context_export<'a>(
    exports: &'a [ExportEntry],
    module: &Module,
    index: usize,
    name: &str,
    span: Span,
) -> Result<&'a ExportEntry, Error> {
    let candidates: Vec<&ExportEntry> = exports
        .iter()
        .filter(|export| export.module != index)
        .filter(|export| export.name == name && export.prefix.is_empty())
        .filter(|export| {
            export.is_accessible_to(module.package_name.as_deref(), module.name.as_deref())
        })
        .collect();
    match candidates.as_slice() {
        [] => Err(Error::new(
            span,
            format!("context declaration `{name}` has no matching grant in the enclosing reach"),
        )),
        [export] => Ok(export),
        _ => Err(Error::new(
            span,
            format!("context declaration `{name}` is ambiguous; several modules grant it"),
        )),
    }
}

/// Names granted by a module's directly enclosed modules into its reach.
fn collect_child_grants(
    module: &Module,
    mode_names: &HashMap<String, Type>,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
) -> Result<HashMap<String, ExportKind>, Error> {
    let mut grants = HashMap::new();
    for action in &module.actions {
        let Action::Module(child) = action else {
            continue;
        };
        let mut child_modes = mode_names.clone();
        if let Some(modes) = span_modes.get(&child.span) {
            child_modes.extend(modes.clone());
        }
        for (name, ty) in collect_module_exports(child, &child_modes, span_modes)? {
            // Duplicate sibling grants are allowed; ambiguity is reported if
            // the parent re-grants or another module seizes the name.
            grants.entry(name).or_insert(ty);
        }
    }
    Ok(grants)
}

/// Names a module grants into its parent reach. Accounts recursively for
/// grants from directly enclosed modules.
fn collect_module_exports(
    module: &Module,
    mode_names: &HashMap<String, Type>,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
) -> Result<HashMap<String, ExportKind>, Error> {
    let mut grantable: HashMap<String, ExportKind> = HashMap::new();
    for declaration in &module.declarations {
        match declaration {
            Declaration::Var(list) => {
                for var in list {
                    if let Ok(ty) = resolve_mode_with(&var.mode, mode_names) {
                        grantable.insert(var.name.clone(), ExportKind::Var(ty));
                    }
                }
            }
            Declaration::Proc(proc_def) => {
                if let Ok(signature) =
                    signature_from_params_with(&proc_def.params, &proc_def.result, mode_names)
                {
                    grantable.insert(proc_def.name.clone(), ExportKind::Proc(signature));
                }
            }
            Declaration::Mode(mode_def) => {
                if let Some(modes) = span_modes.get(&module.span) {
                    if let Some(Type::Struct(id)) = modes.get(&mode_def.name) {
                        grantable.insert(mode_def.name.clone(), ExportKind::Mode(*id));
                    }
                }
            }
            _ => {}
        }
    }
    for action in &module.actions {
        let Action::Module(child) = action else {
            continue;
        };
        let mut child_modes = mode_names.clone();
        if let Some(modes) = span_modes.get(&child.span) {
            child_modes.extend(modes.clone());
        }
        for (name, export) in collect_module_exports(child, &child_modes, span_modes)? {
            grantable.entry(name).or_insert(export);
        }
    }

    let mut exports = HashMap::new();
    let mut granted = HashSet::new();
    for declaration in &module.declarations {
        let Declaration::Visibility(visibility) = declaration else {
            continue;
        };
        if visibility.kind != VisibilityKind::Grant {
            continue;
        }
        if visibility.prefix.is_some()
            || !visibility.friends.is_empty()
            || visibility.forbid.is_some()
        {
            continue;
        }
        let names: Vec<(String, Span)> = if visibility.all {
            grantable
                .keys()
                .map(|name| (name.clone(), visibility.span))
                .collect()
        } else {
            visibility
                .names
                .iter()
                .map(|name| (name.name.clone(), name.span))
                .collect()
        };
        for (name, span) in names {
            if !granted.insert(name.clone()) {
                return Err(Error::new(
                    span,
                    format!("`{name}` is granted more than once"),
                ));
            }
            let Some(export) = grantable.get(&name).cloned() else {
                return Err(Error::new(
                    span,
                    format!(
                        "cannot GRANT `{name}`: not a declaration or child grant in this reach"
                    ),
                ));
            };
            exports.insert(name, export);
        }
    }
    Ok(exports)
}

/// Old and new prefixes of a visibility statement, following §12.2.3.3.
///
/// `PREFIXED` is shorthand: for `GRANT` the new prefix is attached and the old
/// prefix is empty; for `SEIZE` the old prefix is attached and the new prefix is
/// empty. The subset does not support locally prefixed names, so a non-empty new
/// prefix on `SEIZE` is rejected.
pub fn seize_old_prefix(prefix: &Option<PrefixClause>) -> Result<&[String], String> {
    match prefix {
        None | Some(PrefixClause::Prefixed(None)) => Ok(&[]),
        Some(PrefixClause::Prefixed(Some(prefix))) => Ok(prefix),
        Some(PrefixClause::Rename { old, new }) => {
            if new.is_empty() {
                Ok(old)
            } else {
                Err("SEIZE with a non-empty new prefix is not implemented yet".to_owned())
            }
        }
    }
}

/// Resolve a `SEIZE` postfix to its source name and the local simple name.
pub fn seize_name(
    name: &NameRef,
    prefix: &Option<PrefixClause>,
) -> Result<(NameRef, String), String> {
    let old = seize_old_prefix(prefix)?;
    let mut source_prefix = old.to_vec();
    source_prefix.extend(name.prefix.iter().cloned());
    Ok((
        NameRef {
            prefix: source_prefix,
            name: name.name.clone(),
            span: name.span,
        },
        name.name.clone(),
    ))
}

pub(crate) fn grant_prefixes(
    visibility: &crate::ast::Visibility,
    module: &Module,
) -> Result<(Vec<String>, Vec<String>), Error> {
    match &visibility.prefix {
        None => Ok((Vec::new(), Vec::new())),
        Some(PrefixClause::Prefixed(Some(prefix))) => Ok((Vec::new(), prefix.clone())),
        Some(PrefixClause::Prefixed(None)) => {
            let name = module.name.clone().ok_or_else(|| {
                Error::new(
                    visibility.span,
                    "GRANT ... PREFIXED without a prefix requires a named module",
                )
            })?;
            Ok((Vec::new(), vec![name]))
        }
        Some(PrefixClause::Rename { old, new }) => Ok((old.clone(), new.clone())),
    }
}

fn apply_forbid(
    mode: &mut StructMode,
    name: &str,
    forbid: &crate::ast::Forbid,
    span: Span,
) -> Result<(), Error> {
    let fields: Vec<String> = if forbid.all {
        mode.fields.iter().map(|field| field.name.clone()).collect()
    } else {
        forbid.fields.clone()
    };
    for field in fields {
        if !mode.fields.iter().any(|info| info.name == field) {
            return Err(Error::new(
                span,
                format!("FORBID field `{field}` is not a field of `{name}`"),
            ));
        }
        mode.forbidden.insert(field);
    }
    Ok(())
}

fn add_grants(
    module: &Module,
    interface: &ModuleInterface,
    export_module: usize,
    exports: &mut Vec<ExportEntry>,
    modes: &mut ModeTable,
    reach_exports: &HashMap<String, ExportKind>,
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
        let (old_prefix, export_prefix) = grant_prefixes(visibility, module)?;

        let names: Vec<(String, Span)> = if visibility.all {
            if old_prefix.is_empty() {
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
                // Only prefixed names in the module's reach match; the subset
                // has no such grantable names (nested modules are unsupported).
                Vec::new()
            }
        } else {
            let mut names = Vec::with_capacity(visibility.names.len());
            for name in &visibility.names {
                if !old_prefix.is_empty() || !name.prefix.is_empty() {
                    let mut source = old_prefix.clone();
                    source.extend(name.prefix.iter().cloned());
                    return Err(Error::new(
                        name.span,
                        format!(
                            "cannot GRANT `{}`: prefixed names are not declared in this module",
                            format_args!("{}!{}", source.join("!"), name.name)
                        ),
                    ));
                }
                names.push((name.name.clone(), name.span));
            }
            names
        };

        for (name, span) in names {
            let grant_key = format!("{}!{name}", old_prefix.join("!"));
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
                    apply_forbid(modes.get_mut(*id), &name, forbid, span)?;
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
            } else if let Some(kind) = reach_exports.get(&name) {
                match kind {
                    ExportKind::Mode(id) => {
                        if let Some(forbid) = &visibility.forbid {
                            apply_forbid(modes.get_mut(*id), &name, forbid, span)?;
                        }
                        ExportKind::Mode(*id)
                    }
                    ExportKind::Proc(signature) => {
                        if visibility.forbid.is_some() {
                            return Err(Error::new(
                                span,
                                format!("FORBID requires a structure mode, not `{name}`"),
                            ));
                        }
                        ExportKind::Proc(signature.clone())
                    }
                    ExportKind::Var(ty) => {
                        if visibility.forbid.is_some() {
                            return Err(Error::new(
                                span,
                                format!("FORBID requires a structure mode, not `{name}`"),
                            ));
                        }
                        ExportKind::Var(*ty)
                    }
                }
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

fn collect_nested_specs(sources: &[&Module]) -> Result<Vec<Module>, Error> {
    let mut nested: HashMap<String, Module> = HashMap::new();
    for source in sources {
        let declarations = source.declarations.iter().chain(
            source
                .contexts
                .iter()
                .flat_map(|context| context.declarations.iter()),
        );
        for declaration in declarations {
            let Declaration::SpecModule(spec) = declaration else {
                continue;
            };
            let Some(name) = &spec.name else {
                return Err(Error::new(spec.span, "nested spec module must be named"));
            };
            if nested.insert(name.clone(), (**spec).clone()).is_some() {
                return Err(Error::new(
                    spec.span,
                    format!("duplicate nested spec module `{name}`"),
                ));
            }
        }
    }
    Ok(nested.into_values().collect())
}

fn check_nested_specs(
    sources: &[&Module],
    body: &Module,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
) -> Result<(), Error> {
    let nested_specs = collect_nested_specs(sources)?;
    if nested_specs.is_empty() {
        return Ok(());
    }

    let mut nested_bodies: HashMap<String, &Module> = HashMap::new();
    for action in &body.actions {
        let Action::Module(nested) = action else {
            continue;
        };
        if nested.kind == ModuleKind::Body {
            if let Some(name) = &nested.name {
                nested_bodies.insert(name.clone(), nested);
            }
        }
    }

    for nested_spec in &nested_specs {
        let Some(name) = &nested_spec.name else {
            continue;
        };
        let Some(nested_body) = nested_bodies.get(name).copied() else {
            return Err(Error::new(
                nested_spec.span,
                format!("nested spec module `{name}` has no body"),
            ));
        };
        let empty = HashMap::new();
        let spec_modes = span_modes.get(&nested_spec.span).unwrap_or(&empty);
        let body_modes = span_modes.get(&nested_body.span).unwrap_or(&empty);
        let spec_interface = collect_interface(nested_spec, spec_modes)?;
        let body_interface = collect_interface(nested_body, body_modes)?;
        check_interface(&spec_interface, &body_interface, nested_spec.span)?;

        let body_clone = nested_body.clone();
        check_nested_specs(&[nested_spec, &body_clone], nested_body, span_modes)?;
    }
    Ok(())
}

/// Copy `GRANT`/`SEIZE` declarations from nested spec modules into their matching
/// nested body modules. This is the virtual-grant step for nested specs.
pub fn apply_virtual_grants(modules: &mut [Module]) -> Result<(), Error> {
    let mut specs: HashMap<(usize, String), usize> = HashMap::new();
    let mut bodies: Vec<usize> = Vec::new();
    for (index, module) in modules.iter().enumerate() {
        match module.kind {
            ModuleKind::Spec => {
                if let Some(name) = &module.name {
                    specs.insert((module.package, name.clone()), index);
                }
            }
            ModuleKind::Body => bodies.push(index),
        }
    }

    for body_index in bodies {
        let body_clone = modules[body_index].clone();
        let spec_clone = body_clone
            .name
            .as_ref()
            .and_then(|name| specs.get(&(body_clone.package, name.clone())).copied())
            .map(|spec_index| modules[spec_index].clone());

        let mut sources: Vec<&Module> = Vec::new();
        if let Some(spec) = &spec_clone {
            sources.push(spec);
        }
        sources.push(&body_clone);
        let nested_specs = collect_nested_specs(&sources)?;
        if nested_specs.is_empty() {
            continue;
        }
        apply_virtual_visibility(&nested_specs, &mut modules[body_index])?;
    }
    Ok(())
}

fn apply_virtual_visibility(nested_specs: &[Module], body: &mut Module) -> Result<(), Error> {
    for nested_spec in nested_specs {
        let Some(name) = &nested_spec.name else {
            continue;
        };
        let mut found = None;
        for action in &mut body.actions {
            if let Action::Module(module) = action {
                if module.kind == ModuleKind::Body && module.name.as_deref() == Some(name.as_str())
                {
                    found = Some(module);
                    break;
                }
            }
        }
        let Some(nested_body) = found else {
            continue;
        };

        let visibility = nested_spec
            .declarations
            .iter()
            .filter(|declaration| matches!(declaration, Declaration::Visibility(_)))
            .cloned()
            .collect::<Vec<_>>();

        if nested_body
            .declarations
            .iter()
            .any(|declaration| matches!(declaration, Declaration::Visibility(_)))
        {
            return Err(Error::new(
                nested_body.span,
                format!(
                    "nested body `{name}` must not contain visibility statements; they belong in the nested spec"
                ),
            ));
        }

        let nested_body_clone = nested_body.clone();
        let child_specs = collect_nested_specs(&[nested_spec, &nested_body_clone])?;
        nested_body.declarations.extend(visibility);
        apply_virtual_visibility(&child_specs, nested_body)?;
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
