use crate::ast::{Declaration, Expr, Module, ModuleKind, Program};
use crate::lexer::Span;
use crate::modes::ModeTable;
use crate::types::Type;
use std::collections::HashMap;

mod contexts;
mod exports;
mod grants;
mod interface;
mod modes;
mod normalize;
mod reach;
mod specs;
use contexts::walk_nested_modules;
use contexts::{check_contexts, find_remote_context};
pub use exports::{exports, resolve_export, ExportEntry, ExportKind, ModuleInterface};
use grants::{add_grants, has_grant};
pub(crate) use grants::{apply_forbid, grant_prefixes};
pub use grants::{seize_name, seize_old_prefix};
pub use interface::{
    add_external_exports, add_external_modes, portable_type, ExportInfo, ExportInfoKind,
    ExternalInterface, ExternalModeIds, ExternalType, ModeInfo,
};
use modes::build_modes;
pub(crate) use modes::resolve_mode_with;
pub use modes::{
    collect_interface, resolve_mode, signature_from_params, signature_from_params_with,
};
pub use normalize::normalize;
use reach::build_reach_tree;
pub use reach::{ReachExport, ReachExportKind, ReachNode, ReachTree};
use specs::apply_virtual_grants;
use specs::{check_interface, check_nested_specs};

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

#[derive(Debug, Clone)]
pub struct MoretaSource {
    pub declarations: Vec<Declaration>,
    pub invariant: Option<Expr>,
}

pub struct ProgramInfo {
    pub interfaces: Vec<ModuleInterface>,
    pub exports: Vec<ExportEntry>,
    pub modes: ModeTable,
    pub reaches: ReachTree,
    pub moreta_sources: HashMap<crate::types::MoretaId, MoretaSource>,
    pub body_for_spec: HashMap<usize, usize>,
    pub spec_for_body: HashMap<usize, usize>,
}

/// Apply virtual grants from nested specs and return the normalized program.
/// All semantic entry points should analyze the normalized program so behavior does
/// not depend on whether the loader already normalized it.
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

    let (mut modes, module_modes, module_modes_by_span, moreta_sources) = build_modes(program)?;
    let interfaces: Vec<ModuleInterface> = program
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| collect_interface(module, &module_modes[index]))
        .collect::<Result<_, _>>()?;

    let mut module_bodies: HashMap<(usize, String), usize> = HashMap::new();
    let mut module_specs: HashMap<(usize, String), usize> = HashMap::new();
    let mut region_bodies: HashMap<(usize, String), usize> = HashMap::new();
    let mut region_specs: HashMap<(usize, String), usize> = HashMap::new();

    for (index, module) in program.modules.iter().enumerate() {
        let Some(name) = &module.name else {
            if module.kind.is_spec() {
                return Err(Error::new(
                    module.span,
                    "spec module or spec region must be named",
                ));
            }
            continue;
        };
        let target = match module.kind {
            ModuleKind::Body => &mut module_bodies,
            ModuleKind::Spec => &mut module_specs,
            ModuleKind::Region => &mut region_bodies,
            ModuleKind::SpecRegion => &mut region_specs,
        };
        if target
            .insert((module.package, name.clone()), index)
            .is_some()
        {
            return Err(Error::new(
                module.span,
                format!("duplicate module/region name `{name}`"),
            ));
        }
    }

    let reaches = build_reach_tree(program, &interfaces, &module_modes_by_span, &mut modes)?;

    let mut exports: Vec<ExportEntry> = Vec::new();
    let mut body_for_spec = HashMap::new();
    let mut spec_for_body = HashMap::new();

    for (spec_map, body_map, label) in [
        (&module_specs, &module_bodies, "module"),
        (&region_specs, &region_bodies, "region"),
    ] {
        for ((package, name), spec_index) in spec_map {
            let Some(body_index) = body_map.get(&(*package, name.clone())).copied() else {
                return Err(Error::new(
                    program.modules[*spec_index].span,
                    format!("spec {label} `{name}` has no body"),
                ));
            };
            body_for_spec.insert(*spec_index, body_index);
            spec_for_body.insert(body_index, *spec_index);

            let spec = &program.modules[*spec_index];
            let body = &program.modules[body_index];

            if has_grant(body) {
                return Err(Error::new(
                    body.span,
                    format!("{label} `{name}` has a SPEC; GRANT must be in the spec {label}"),
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
        if module.kind.is_body() && !spec_for_body.contains_key(&index) {
            let reach_vars = reaches
                .get(module.span)
                .map(|node| {
                    let mut vars: HashMap<String, ExportKind> = HashMap::new();
                    for (name, entries) in &node.exports {
                        for entry in entries {
                            if !entry.meta.prefix.is_empty() {
                                continue;
                            }
                            let kind = match &entry.kind {
                                ReachExportKind::Var(ty) => ExportKind::Var(*ty),
                                ReachExportKind::Proc(signature) => {
                                    ExportKind::Proc(signature.clone())
                                }
                                ReachExportKind::Mode(Type::Struct(id)) => ExportKind::Mode(*id),
                                ReachExportKind::Mode(_) => continue,
                            };
                            vars.entry(name.clone()).or_insert(kind);
                        }
                    }
                    vars
                })
                .unwrap_or_default();
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
        reaches,
        moreta_sources,
        body_for_spec,
        spec_for_body,
    })
}
