use crate::ast::{
    Declaration, ModeRef, Module, ModuleKind, Param, Passing, Program, VisibilityKind,
};
use crate::lexer::Span;
use crate::types::{self, ProcType, Type};
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
}

#[derive(Debug, Clone)]
pub enum ExportKind {
    Var(Type),
    Proc(ProcType),
}

#[derive(Debug, Clone)]
pub struct Export {
    /// Index of the module that owns the implementation. For a spec export this
    /// is the paired body module.
    pub module: usize,
    pub kind: ExportKind,
}

#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub name: String,
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
    pub exports: HashMap<String, Export>,
    pub body_for_spec: HashMap<usize, usize>,
    pub spec_for_body: HashMap<usize, usize>,
}

pub fn analyze(program: &Program) -> Result<ProgramInfo, Error> {
    let interfaces: Vec<ModuleInterface> = program
        .modules
        .iter()
        .map(collect_interface)
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

    let mut exports: HashMap<String, Export> = HashMap::new();
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
        add_grants(spec, &interfaces[*spec_index], body_index, &mut exports)?;
    }

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind == ModuleKind::Body && !spec_for_body.contains_key(&index) {
            add_grants(module, &interfaces[index], index, &mut exports)?;
        }
    }

    Ok(ProgramInfo {
        interfaces,
        exports,
        body_for_spec,
        spec_for_body,
    })
}

/// The public interface of the program, sorted by name.
pub fn exports(program: &Program) -> Result<Vec<ExportInfo>, Error> {
    let mut exports: Vec<ExportInfo> = analyze(program)?
        .exports
        .into_iter()
        .map(|(name, export)| ExportInfo {
            name,
            kind: match export.kind {
                ExportKind::Var(ty) => ExportInfoKind::Var(ty),
                ExportKind::Proc(signature) => ExportInfoKind::Proc {
                    params: signature.params,
                    result: signature.result,
                },
            },
        })
        .collect();
    exports.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(exports)
}

pub fn collect_interface(module: &Module) -> Result<ModuleInterface, Error> {
    let mut interface = ModuleInterface::default();

    for declaration in &module.declarations {
        match declaration {
            Declaration::Var(list) => {
                for var in list {
                    let ty = resolve_mode(&var.mode)?;
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
                let signature = signature_from_params(&proc_def.params, &proc_def.result)?;
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
                let proc_sig = signature_from_params(&signature.params, &signature.result)?;
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
            Declaration::Visibility(_) => {}
        }
    }

    Ok(interface)
}

pub fn signature_from_params(
    params: &[Param],
    result: &Option<ModeRef>,
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
        param_types.push(resolve_mode(&param.mode)?);
    }

    let result_type = match result {
        Some(mode) => Some(resolve_mode(mode)?),
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
    exports: &mut HashMap<String, Export>,
) -> Result<(), Error> {
    for declaration in &module.declarations {
        let Declaration::Visibility(visibility) = declaration else {
            continue;
        };
        if visibility.kind != VisibilityKind::Grant {
            continue;
        }

        for name in &visibility.names {
            let kind = if let Some(ty) = interface.vars.get(&name.name) {
                ExportKind::Var(*ty)
            } else if let Some(signature) = interface.procs.get(&name.name) {
                ExportKind::Proc(signature.clone())
            } else {
                return Err(Error::new(
                    name.span,
                    format!("cannot GRANT `{}`: not declared in this module", name.name),
                ));
            };

            if let Some(previous) = exports.get(&name.name) {
                let message = if previous.module == export_module {
                    format!("`{}` is granted more than once", name.name)
                } else {
                    format!("`{}` is granted by more than one module", name.name)
                };
                return Err(Error::new(name.span, message));
            }

            exports.insert(
                name.name.clone(),
                Export {
                    module: export_module,
                    kind,
                },
            );
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
