use crate::ast::*;
use crate::ir;
use crate::lexer::{SourceId, Span};
use crate::manifest::WidthProfile;
use crate::program::{self, ExportKind};
use crate::sema::{self, CheckResult};
use crate::types::{self, IntKind, Type};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
    pub exception: Option<String>,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        let exception = if message == "ASSERTFAIL" {
            Some("ASSERTFAIL".to_owned())
        } else if message.starts_with("OVERFLOW") {
            Some("OVERFLOW".to_owned())
        } else {
            None
        };
        Self {
            code: classify(&message),
            span,
            message,
            exception,
        }
    }

    pub fn coded(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
            exception: None,
        }
    }

    pub fn exception(name: &str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "E0510",
            span,
            message: message.into(),
            exception: Some(name.to_owned()),
        }
    }

    pub fn raised(name: &str, span: Span) -> Self {
        Self {
            code: "E0509",
            span,
            message: format!("unhandled exception `{name}`"),
            exception: Some(name.to_owned()),
        }
    }
}

fn classify(message: &str) -> &'static str {
    if message.contains("ASSERTFAIL") {
        "E0501"
    } else if message.contains("OVERFLOW") {
        "E0502"
    } else if message.contains("division by zero")
        || message.contains("remainder by zero")
        || message.contains("positive divisor")
        || message.contains("negative exponent")
        || message.contains("exponent is too large")
    {
        "E0503"
    } else if message.contains("unhandled exception") {
        "E0509"
    } else if message.contains("undefined value") {
        "E0504"
    } else if message.contains("unknown variable") || message.contains("unknown name") {
        "E0505"
    } else if message.contains("call depth")
        || message.contains("expects")
        || message.contains("returned no result")
    {
        "E0506"
    } else if message.contains("EXIT")
        || message.contains("RETURN")
        || message.contains("cannot SEIZE")
        || message.contains("duplicate declaration")
    {
        "E0508"
    } else if message.contains("expected ")
        || message.contains("requires")
        || message.contains("cannot compare")
        || message.contains("cannot order")
        || message.contains("does not fit")
        || message.contains("conversion")
        || message.contains("integer widths")
    {
        "E0507"
    } else {
        "E0500"
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub fn run(program: &Program, profile: WidthProfile) -> Result<(), Error> {
    let check = sema::analyze(program, profile);
    if let Some(diagnostic) = check.diagnostics.first() {
        let span = diagnostic.span.unwrap_or_else(|| {
            program
                .modules
                .first()
                .map(|module| module.span)
                .unwrap_or(Span::new(SourceId(0), 0, 0))
        });
        return Err(Error::coded(
            diagnostic.code,
            span,
            diagnostic.message.clone(),
        ));
    }
    run_checked(program, profile, &check, &[])
}

pub fn run_checked(
    program: &Program,
    profile: WidthProfile,
    check: &CheckResult,
    external: &[program::ExternalInterface],
) -> Result<(), Error> {
    let program = program::normalize(program)
        .map_err(|error| Error::coded(error.code, error.span, error.message))?;
    let info = program::analyze(&program)
        .map_err(|error| Error::coded(error.code, error.span, error.message))?;
    let module_modes: Vec<HashMap<String, Type>> = info
        .interfaces
        .iter()
        .map(|interface| interface.modes.clone())
        .collect();
    let error_span = program
        .modules
        .first()
        .map(|module| module.span)
        .unwrap_or_else(|| Span::new(SourceId(0), 0, 0));
    let mut modes = info.modes;
    let external_modes = program::add_external_modes(&mut modes, external)
        .map_err(|message| Error::coded("E0600", error_span, message))?;
    let mut exports = info.exports;
    program::add_external_exports(&mut exports, external, &external_modes)
        .map_err(|message| Error::coded("E0600", error_span, message))?;
    let modes = Rc::new(modes);
    // A module may use a seized structure mode in its declarations, so resolve
    // mode seizes before any variable is declared.
    let mut module_modes = module_modes;
    for (index, module) in program.modules.iter().enumerate() {
        if module.kind.is_spec() {
            continue;
        }
        let package_name = module.package_name.as_deref();
        let module_name = module.name.as_deref();
        for declaration in &module.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            if visibility.all {
                for export in exports
                    .iter()
                    .filter(|export| export.module != index)
                    .filter(|export| export.is_accessible_to(package_name, module_name))
                {
                    if let ExportKind::Mode(id) = export.kind {
                        module_modes[index].insert(export.name.clone(), Type::Struct(id));
                    }
                }
            } else {
                for name in &visibility.names {
                    let Ok(export) =
                        program::resolve_export(&exports, name, package_name, module_name)
                    else {
                        continue;
                    };
                    if let ExportKind::Mode(id) = export.kind {
                        module_modes[index].insert(name.name.clone(), Type::Struct(id));
                    }
                }
            }
        }
    }

    let lowered = ir::lower(&program, check)
        .map_err(|error| Error::coded("E0400", error.span, error.message))?;
    let mut interpreter = Interpreter {
        profile,
        modules: Vec::new(),
        locals: Vec::new(),
        call_bases: Vec::new(),
        active_module: 0,
        active_scope_key: None,
        module_floor: 0,
        depth: 0,
        exports,
        modes,
        module_modes,
        reaches: info.reaches,
        modules_by_span: collect_modules_by_span(&program),
        package_names: program
            .modules
            .iter()
            .map(|module| module.package_name.clone())
            .collect(),
        nested_scopes: HashMap::new(),
        exprs: lowered.exprs,
        bodies: lowered.bodies,
    };

    interpreter.declare_modules(&program)?;
    interpreter.apply_seizes(&program)?;

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind.is_spec() {
            continue;
        }
        interpreter.active_module = index;
        interpreter.initialize_declarations(&module.declarations)?;
        interpreter.run_module(module)?;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Undefined,
    Int { value: i128, kind: Option<IntKind> },
    Float(f64),
    Bool(bool),
    Char(char),
    String(String),
    Struct(Vec<(String, Value)>),
    Array { low: i128, values: Vec<Value> },
    Association(Option<Rc<RefCell<AssociationState>>>),
    Access(Option<Rc<RefCell<AccessState>>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssociationState {
    path: String,
    existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AccessState {
    association: Option<Rc<RefCell<AssociationState>>>,
    position: usize,
}

struct Var {
    ty: Type,
    value: Value,
}

type VarCell = Rc<RefCell<Var>>;

#[derive(Clone, Copy)]
enum ImportKind {
    Var,
    Proc,
    Mode,
}

impl ImportKind {
    fn of(kind: &ExportKind) -> Self {
        match kind {
            ExportKind::Var(_) => Self::Var,
            ExportKind::Proc(_) => Self::Proc,
            ExportKind::Mode(_) => Self::Mode,
        }
    }
}

#[derive(Clone, Copy)]
struct ProcRef<'a> {
    def: &'a ProcDef,
    module: usize,
    scope_key: Option<Span>,
}

#[derive(Clone)]
struct ReachBinding<'a> {
    meta: crate::reach::ExportMeta,
    kind: ReachKind<'a>,
}

#[derive(Clone)]
enum ReachKind<'a> {
    Var(VarCell),
    Proc(ProcRef<'a>),
    Mode(Type),
}

#[derive(Default, Clone)]
struct Scope<'a> {
    vars: HashMap<String, VarCell>,
    procs: HashMap<String, ProcRef<'a>>,
    modes: HashMap<String, Type>,
    /// Names granted into this reach by directly enclosed modules.
    exports: HashMap<String, Vec<ReachBinding<'a>>>,
    /// Whether hoisted-module `SEIZE`s have been applied.
    seizes_applied: bool,
    /// Span of the nested module that owns this scope, if any.
    key: Option<Span>,
}

struct Frame {
    result_type: Option<Type>,
    result: Option<Value>,
}

#[derive(Debug)]
enum Flow {
    Normal,
    Return,
    Exit(Option<String>),
}

struct Interpreter<'a> {
    profile: WidthProfile,
    /// Persistent scopes, one per module, indexed like `program.modules`.
    modules: Vec<Scope<'a>>,
    /// Call and `BEGIN` scopes, searched before the active module scope.
    locals: Vec<Scope<'a>>,
    /// Index into `locals` at each procedure-call entry.
    call_bases: Vec<usize>,
    active_module: usize,
    /// Module scope used for lookups while executing a procedure.
    active_scope_key: Option<Span>,
    /// Lowest local scope visible in the current module.
    module_floor: usize,
    depth: usize,
    exports: Vec<program::ExportEntry>,
    modes: Rc<crate::modes::ModeTable>,
    module_modes: Vec<HashMap<String, Type>>,
    reaches: program::ReachTree,
    modules_by_span: HashMap<Span, &'a Module>,
    package_names: Vec<Option<String>>,
    /// Scopes for nested modules declared in a module body, keyed by span.
    nested_scopes: HashMap<Span, Scope<'a>>,
    exprs: ir::ExprTable,
    bodies: ir::BodyTable,
}

impl<'a> Interpreter<'a> {
    fn declare_modules(&mut self, program: &'a Program) -> Result<(), Error> {
        for (index, module) in program.modules.iter().enumerate() {
            let mode_names = self.module_modes[index].clone();
            let modes = Rc::clone(&self.modes);
            let mut scope = Scope {
                modes: mode_names.clone(),
                ..Scope::default()
            };
            if module.kind.is_body() {
                declare_into_scope(
                    &mut scope,
                    &module.declarations,
                    index,
                    &mode_names,
                    &modes,
                    None,
                )?;
            }
            self.modules.push(scope);
            if module.kind.is_body() {
                self.hoist_top_module(module, index)?;
            }
        }
        Ok(())
    }

    fn hoist_top_module(&mut self, module: &'a Module, index: usize) -> Result<(), Error> {
        let mut parent = std::mem::take(&mut self.modules[index]);
        let mode_names = self.module_modes[index].clone();
        self.hoist_children(module.span, &mut parent, index, &mode_names)?;
        self.modules[index] = parent;
        Ok(())
    }

    fn hoist_children(
        &mut self,
        span: Span,
        parent: &mut Scope<'a>,
        top: usize,
        mode_names: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        self.hoist_reach_children(span, parent, top, mode_names, false)?;
        self.seed_reach_exports(span, parent);
        Ok(())
    }

    /// Hoist the modules that belong to `span`'s reach. Module bodies only hoist
    /// directly-enclosed modules; block reaches also hoist modules declared in the
    /// block. Names are always resolved against the live mode table.
    fn hoist_reach_children(
        &mut self,
        span: Span,
        parent: &mut Scope<'a>,
        top: usize,
        mode_names: &HashMap<String, Type>,
        include_block_children: bool,
    ) -> Result<(), Error> {
        let Some(node) = self.reaches.get(span) else {
            return Ok(());
        };
        let children: Vec<usize> = node.children.clone();
        for child_id in children {
            let Some(child_node) = self.reaches.nodes.get(child_id) else {
                continue;
            };
            if !child_node.hoisted && !include_block_children {
                continue;
            }
            let child_span = child_node.span;
            let Some(child) = self.modules_by_span.get(&child_span).copied() else {
                continue;
            };
            let child_modes = child_node.interface.modes.clone();

            let mut resolution_modes = child_modes.clone();
            resolution_modes.extend(parent.modes.clone());
            resolution_modes.extend(mode_names.clone());
            let modes = Rc::clone(&self.modes);
            let mut scope = Scope {
                modes: child_modes,
                key: Some(child_span),
                ..Scope::default()
            };
            declare_into_scope(
                &mut scope,
                &child.declarations,
                top,
                &resolution_modes,
                &modes,
                Some(child_span),
            )?;
            self.hoist_children(child_span, &mut scope, top, mode_names)?;
            self.nested_scopes.insert(child_span, scope);
        }
        Ok(())
    }

    fn seed_reach_exports(&mut self, span: Span, scope: &mut Scope<'a>) {
        let Some(node) = self.reaches.get(span) else {
            return;
        };
        let exports = node.exports.clone();
        for (name, entries) in exports {
            for entry in entries {
                let Some(owner) = self.nested_scopes.get(&entry.owner) else {
                    continue;
                };
                let binding = match entry.kind {
                    program::ReachExportKind::Var(_) => {
                        let cell = owner.vars.get(&name).cloned().or_else(|| {
                            match owner.exports.get(&name) {
                                Some(exports) => match exports.as_slice() {
                                    [ReachBinding {
                                        kind: ReachKind::Var(cell),
                                        ..
                                    }] => Some(cell.clone()),
                                    _ => None,
                                },
                                None => None,
                            }
                        });
                        let Some(cell) = cell else {
                            continue;
                        };
                        ReachBinding {
                            meta: entry.meta,
                            kind: ReachKind::Var(cell),
                        }
                    }
                    program::ReachExportKind::Proc(_) => {
                        let proc_ref = owner.procs.get(&name).copied().or_else(|| {
                            match owner.exports.get(&name) {
                                Some(exports) => match exports.as_slice() {
                                    [ReachBinding {
                                        kind: ReachKind::Proc(proc_ref),
                                        ..
                                    }] => Some(*proc_ref),
                                    _ => None,
                                },
                                None => None,
                            }
                        });
                        let Some(proc_ref) = proc_ref else {
                            continue;
                        };
                        ReachBinding {
                            meta: entry.meta,
                            kind: ReachKind::Proc(proc_ref),
                        }
                    }
                    program::ReachExportKind::Mode(ty) => ReachBinding {
                        meta: entry.meta,
                        kind: ReachKind::Mode(ty),
                    },
                };
                scope.exports.entry(name.clone()).or_default().push(binding);
            }
        }
    }

    fn apply_seizes(&mut self, program: &'a Program) -> Result<(), Error> {
        let mut imports = Vec::new();

        for (current, module) in program.modules.iter().enumerate() {
            if module.kind.is_spec() {
                continue;
            }
            let package_name = module.package_name.as_deref();
            let module_name = module.name.as_deref();

            for declaration in &module.declarations {
                let Declaration::Visibility(visibility) = declaration else {
                    continue;
                };
                if visibility.kind != VisibilityKind::Seize {
                    continue;
                }

                if visibility.all {
                    let old_prefix = program::seize_old_prefix(&visibility.prefix)
                        .map_err(|message| Error::new(visibility.span, message))?;
                    let mut accessible: Vec<&program::ExportEntry> = self
                        .exports
                        .iter()
                        .filter(|export| export.module != current)
                        .filter(|export| export.prefix == old_prefix)
                        .filter(|export| export.is_accessible_to(package_name, module_name))
                        .collect();
                    accessible.sort_by(|left, right| {
                        left.name
                            .cmp(&right.name)
                            .then_with(|| left.package.cmp(&right.package))
                            .then_with(|| left.module.cmp(&right.module))
                    });

                    let mut names = HashSet::new();
                    for export in accessible {
                        if !names.insert(export.name.clone()) {
                            return Err(Error::new(
                                visibility.span,
                                format!(
                                    "SEIZE ALL is ambiguous for `{}`; use a qualified SEIZE",
                                    export.name
                                ),
                            ));
                        }
                        if export.module == usize::MAX {
                            return Err(Error::new(
                                visibility.span,
                                format!(
                                    "cannot SEIZE `{}`: external dependency is not linked",
                                    export.name
                                ),
                            ));
                        }
                        imports.push((
                            current,
                            export.name.clone(),
                            visibility.span,
                            export.module,
                            ImportKind::of(&export.kind),
                        ));
                    }
                    continue;
                }

                for name in &visibility.names {
                    let (source, local) = program::seize_name(name, &visibility.prefix)
                        .map_err(|message| Error::new(name.span, message))?;
                    let export =
                        program::resolve_export(&self.exports, &source, package_name, module_name)
                            .map_err(|message| Error::new(name.span, message))?;

                    if export.module == current {
                        return Err(Error::new(
                            name.span,
                            format!("cannot SEIZE `{}` from the same module", name.qualified()),
                        ));
                    }
                    if export.module == usize::MAX {
                        return Err(Error::new(
                            name.span,
                            format!(
                                "cannot SEIZE `{}`: external dependency is not linked",
                                name.qualified()
                            ),
                        ));
                    }

                    imports.push((
                        current,
                        local,
                        name.span,
                        export.module,
                        ImportKind::of(&export.kind),
                    ));
                }
            }
        }

        for (current, name, span, source, kind) in imports {
            let duplicate = match kind {
                ImportKind::Proc => {
                    let proc_ref = *self.modules[source]
                        .procs
                        .get(&name)
                        .or_else(|| match self.modules[source].exports.get(&name) {
                            Some(exports) => match exports.as_slice() {
                                [ReachBinding {
                                    kind: ReachKind::Proc(proc_ref),
                                    ..
                                }] => Some(proc_ref),
                                _ => None,
                            },
                            None => None,
                        })
                        .ok_or_else(|| {
                            Error::new(span, format!("cannot SEIZE `{name}`: not a procedure"))
                        })?;
                    self.modules[current]
                        .procs
                        .insert(name.clone(), proc_ref)
                        .is_some()
                }
                ImportKind::Var => {
                    let cell = self.modules[source]
                        .vars
                        .get(&name)
                        .cloned()
                        .or_else(|| match self.modules[source].exports.get(&name) {
                            Some(exports) => match exports.as_slice() {
                                [ReachBinding {
                                    kind: ReachKind::Var(cell),
                                    ..
                                }] => Some(cell.clone()),
                                _ => None,
                            },
                            None => None,
                        })
                        .ok_or_else(|| {
                            Error::new(span, format!("cannot SEIZE `{name}`: not a location"))
                        })?;
                    self.modules[current]
                        .vars
                        .insert(name.clone(), cell)
                        .is_some()
                }
                // Mode seizes are installed before declarations are collected.
                ImportKind::Mode => false,
            };
            if duplicate {
                return Err(Error::new(
                    span,
                    format!("duplicate declaration of `{name}`"),
                ));
            }
        }

        Ok(())
    }

    fn initialize_declarations(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                if let Some(init) = &var.init {
                    let value = self.eval_expr(&init.value)?;
                    self.assign_var(&var.name, value, var.span)?;
                }
            }
        }
        Ok(())
    }

    fn declare_local(&mut self, declarations: &'a [Declaration]) -> Result<(), Error> {
        let module = self.active_module;
        let mode_names = self.module_modes[module].clone();
        let modes = Rc::clone(&self.modes);
        let scope = self.locals.last_mut().expect("local scope");
        declare_into_scope(scope, declarations, module, &mode_names, &modes, None)
    }

    fn declare_local_vars(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let mut mode_names = self.modules[self.active_module].modes.clone();
        let floor = self.local_floor();
        for scope in &self.locals[floor..] {
            for (name, ty) in &scope.modes {
                mode_names.insert(name.clone(), *ty);
            }
        }
        let modes = Rc::clone(&self.modes);
        let scope = self.locals.last_mut().expect("local scope");
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let ty = resolve_mode_with(&var.mode, &mode_names)?;
                        if scope.vars.contains_key(&var.name) || scope.procs.contains_key(&var.name)
                        {
                            return Err(Error::new(
                                var.span,
                                format!("duplicate declaration of `{}`", var.name),
                            ));
                        }
                        scope.vars.insert(
                            var.name.clone(),
                            Rc::new(RefCell::new(Var {
                                ty,
                                value: default_value(&modes, ty),
                            })),
                        );
                    }
                }
                Declaration::Proc(proc_def) => {
                    return Err(Error::new(
                        proc_def.span,
                        "procedure definitions inside BEGIN are not implemented",
                    ));
                }
                Declaration::ProcSignature(_)
                | Declaration::Visibility(_)
                | Declaration::Mode(_)
                | Declaration::SpecModule(_) => {}
            }
        }
        Ok(())
    }

    fn run_module(&mut self, module: &'a Module) -> Result<(), Error> {
        let body = self
            .bodies
            .get(module.span)
            .ok_or_else(|| Error::new(module.span, "internal: missing module IR body"))?;
        let mut frame = Frame {
            result_type: None,
            result: None,
        };
        match self.exec_with_handler(module.span, &body, &mut frame)? {
            Flow::Normal | Flow::Return | Flow::Exit(None) => Ok(()),
            Flow::Exit(Some(name)) => {
                if module.name.as_deref() == Some(name.as_str()) {
                    Ok(())
                } else {
                    Err(Error::new(
                        module.span,
                        format!("unknown EXIT label `{name}`"),
                    ))
                }
            }
        }
    }

    fn exec_with_handler(
        &mut self,
        span: Span,
        stmts: &[ir::Stmt],
        frame: &mut Frame,
    ) -> Result<Flow, Error> {
        match self.exec_stmts(stmts, frame) {
            Err(error) if error.exception.is_some() => {
                let name = error.exception.as_deref().unwrap_or_default();
                if let Some(handler) = self.bodies.handler(span) {
                    if let Some(actions) = handler.alternatives.iter().find_map(|alternative| {
                        alternative
                            .exceptions
                            .iter()
                            .any(|exception| exception == name)
                            .then_some(&alternative.actions)
                    }) {
                        return self.exec_stmts(actions, frame);
                    }
                    if let Some(actions) = &handler.else_actions {
                        return self.exec_stmts(actions, frame);
                    }
                }
                Err(error)
            }
            other => other,
        }
    }

    fn exec_stmts(&mut self, stmts: &[ir::Stmt], frame: &mut Frame) -> Result<Flow, Error> {
        for stmt in stmts {
            match stmt {
                ir::Stmt::Assign {
                    target,
                    op,
                    value,
                    span,
                } => {
                    let value = if *op == AssignOp::Assign {
                        self.eval_ir(value)?
                    } else {
                        let current = self.read_target(target, *span)?;
                        let right = self.eval_ir(value)?;
                        let target_ty = self.target_type(target, *span)?;
                        self.apply_binary(
                            binary_for_assign(*op),
                            current,
                            right,
                            Some(target_ty),
                            *span,
                        )?
                    };
                    self.assign_target(target, value, *span)?;
                }
                ir::Stmt::Assert { condition, span } => {
                    let value = self.eval_ir(condition)?;
                    if !self.as_bool(value, *span)? {
                        return Err(Error::new(*span, "ASSERTFAIL"));
                    }
                }
                ir::Stmt::Call { call, .. } => {
                    self.eval_ir(call)?;
                }
                ir::Stmt::Return { value, span } => {
                    if let Some(value) = value {
                        let result_type = frame.result_type.ok_or_else(|| {
                            Error::new(
                                *span,
                                "RETURN with a value in a procedure that has no result",
                            )
                        })?;
                        let value = self.eval_ir(value)?;
                        frame.result = Some(self.convert(value, result_type, *span)?);
                    }
                    return Ok(Flow::Return);
                }
                ir::Stmt::Result { value, span } => {
                    let result_type = frame
                        .result_type
                        .ok_or_else(|| Error::new(*span, "RESULT outside a value procedure"))?;
                    let value = self.eval_ir(value)?;
                    frame.result = Some(self.convert(value, result_type, *span)?);
                }
                ir::Stmt::If {
                    branches,
                    else_actions,
                    else_span,
                    ..
                } => {
                    for branch in branches {
                        let condition = self.eval_ir(&branch.condition)?;
                        if self.as_bool(condition, branch.condition.span())? {
                            return self.exec_block(&branch.body, branch.span, frame);
                        }
                    }
                    if let (Some(actions), Some(span)) = (else_actions, else_span) {
                        return self.exec_block(actions, *span, frame);
                    }
                }
                ir::Stmt::Do {
                    control,
                    body,
                    span,
                } => return self.exec_do(control, body, frame, *span),
                ir::Stmt::Exit { label, .. } => return Ok(Flow::Exit(label.clone())),
                ir::Stmt::Case {
                    selector,
                    alternatives,
                    else_actions,
                    else_span,
                    ..
                } => {
                    let selector_value = self.eval_ir(selector)?;
                    let (value, _) = self.expect_int(selector_value, selector.span())?;

                    let explicitly_labelled = alternatives.iter().any(|alternative| {
                        alternative
                            .labels
                            .iter()
                            .any(|label| explicit_label_matches(*label, value))
                    });

                    for alternative in alternatives {
                        let matches = alternative.labels.iter().any(|label| match label {
                            CaseLabel::Else => !explicitly_labelled,
                            CaseLabel::Any => true,
                            other => explicit_label_matches(*other, value),
                        });
                        if matches {
                            return self.exec_block(&alternative.body, alternative.span, frame);
                        }
                    }

                    if let (Some(actions), Some(span)) = (else_actions, else_span) {
                        return self.exec_block(actions, *span, frame);
                    }
                }
                ir::Stmt::Labelled { label, action, .. } => {
                    match self.exec_stmts(std::slice::from_ref(action.as_ref()), frame)? {
                        Flow::Exit(Some(name)) if &name == label => return Ok(Flow::Normal),
                        flow => return Ok(flow),
                    }
                }
                ir::Stmt::Begin {
                    declarations,
                    actions,
                    span,
                } => {
                    self.locals.push(Scope::default());
                    let result = (|| {
                        self.declare_local_vars(declarations)?;
                        self.initialize_declarations(declarations)?;
                        let mut scope = self.locals.pop().expect("begin scope");
                        let mode_names = self.module_modes[self.active_module].clone();
                        self.hoist_block_children(*span, &mut scope, &mode_names)?;
                        self.seed_reach_exports(*span, &mut scope);
                        self.locals.push(scope);
                        self.exec_with_handler(*span, actions, frame)
                    })();
                    self.locals.pop();
                    return match result {
                        Ok(Flow::Exit(None)) => Ok(Flow::Normal),
                        other => other,
                    };
                }
                ir::Stmt::Empty { .. } => {}
                ir::Stmt::Cause { name, span } => {
                    return Err(Error::raised(name, *span));
                }
                ir::Stmt::Module {
                    label,
                    declarations,
                    actions,
                    span,
                } => {
                    return self.exec_nested_module(label.as_deref(), declarations, actions, *span);
                }
                ir::Stmt::Handled { action, span } => {
                    return self.exec_with_handler(
                        *span,
                        std::slice::from_ref(action.as_ref()),
                        frame,
                    );
                }
            }
        }
        Ok(Flow::Normal)
    }

    fn resolve_text_width(
        &mut self,
        width: crate::builtins::ClauseWidth,
        io: &[ir::TextIoArg],
        io_index: &mut usize,
        span: Span,
    ) -> Result<usize, Error> {
        match width {
            crate::builtins::ClauseWidth::Fixed(width) => Ok(width),
            crate::builtins::ClauseWidth::Variable => {
                let Some(ir::TextIoArg::Value(argument)) = io.get(*io_index) else {
                    return Err(textfail(span, "missing variable width value"));
                };
                *io_index += 1;
                let value = self.eval_ir(argument)?;
                let (value, _) = self.expect_int(value, argument.span())?;
                if value < 0 {
                    return Err(textfail(span, "negative variable width"));
                }
                Ok(value as usize)
            }
        }
    }

    fn exec_association(
        &mut self,
        op: crate::builtins::AssociationOp,
        first: &str,
        second: Option<&str>,
        value: Option<&ir::Node>,
        ty: Option<Type>,
        span: Span,
    ) -> Result<Value, Error> {
        use crate::builtins::AssociationOp;
        match op {
            AssociationOp::Associate => {
                let Some(value) = value else {
                    return Err(Error::new(span, "ASSOCIATE requires a path"));
                };
                let path = self.eval_ir(value)?;
                let Value::String(path) = path else {
                    return Err(Error::new(span, "ASSOCIATE path must be CHARS"));
                };
                let cell = self
                    .find_var_cell(first)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{first}`")))?;
                let mut var = cell.borrow_mut();
                match &var.value {
                    Value::Association(None) => {}
                    Value::Association(Some(_)) => {
                        return Err(Error::exception(
                            "ASSOCIATEFAIL",
                            span,
                            "ASSOCIATEFAIL: association already exists",
                        ));
                    }
                    _ => return Err(Error::new(span, "expected an ASSOCIATION location")),
                }
                let existing = std::path::Path::new(&path).exists();
                var.value = Value::Association(Some(Rc::new(RefCell::new(AssociationState {
                    path,
                    existing,
                }))));
                Ok(Value::Undefined)
            }
            AssociationOp::IsAssociated => {
                let cell = self
                    .find_var_cell(first)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{first}`")))?;
                let var = cell.borrow();
                let Value::Association(state) = &var.value else {
                    return Err(Error::new(span, "expected an ASSOCIATION location"));
                };
                Ok(Value::Bool(state.is_some()))
            }
            AssociationOp::Existing
            | AssociationOp::Readable
            | AssociationOp::Writeable
            | AssociationOp::Indexable
            | AssociationOp::Sequencible
            | AssociationOp::Variable => {
                let state = self.association_state(first, span)?;
                let state = state.borrow();
                let value = match op {
                    AssociationOp::Existing => state.existing,
                    AssociationOp::Readable => state.existing,
                    AssociationOp::Writeable => state.existing,
                    AssociationOp::Indexable => false,
                    AssociationOp::Sequencible => true,
                    AssociationOp::Variable => false,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(value))
            }
            AssociationOp::Dissociate => {
                let cell = self
                    .find_var_cell(first)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{first}`")))?;
                let mut var = cell.borrow_mut();
                let Value::Association(state) = &mut var.value else {
                    return Err(Error::new(span, "expected an ASSOCIATION location"));
                };
                if state.take().is_none() {
                    return Err(Error::exception(
                        "NOTASSOCIATED",
                        span,
                        "NOTASSOCIATED: empty association",
                    ));
                }
                Ok(Value::Undefined)
            }
            AssociationOp::Create => {
                let state = self.association_state(first, span)?;
                if state.borrow().existing {
                    return Err(Error::exception(
                        "CREATEFAIL",
                        span,
                        "CREATEFAIL: file already exists",
                    ));
                }
                let path = state.borrow().path.clone();
                std::fs::write(&path, "").map_err(|error| {
                    Error::exception("CREATEFAIL", span, format!("CREATEFAIL: {error}"))
                })?;
                state.borrow_mut().existing = true;
                Ok(Value::Undefined)
            }
            AssociationOp::Delete => {
                let state = self.association_state(first, span)?;
                if !state.borrow().existing {
                    return Err(Error::exception(
                        "DELETEFAIL",
                        span,
                        "DELETEFAIL: file does not exist",
                    ));
                }
                let path = state.borrow().path.clone();
                std::fs::remove_file(&path).map_err(|error| {
                    Error::exception("DELETEFAIL", span, format!("DELETEFAIL: {error}"))
                })?;
                state.borrow_mut().existing = false;
                Ok(Value::Undefined)
            }
            AssociationOp::Connect => {
                let Some(second) = second else {
                    return Err(Error::new(span, "CONNECT requires an association"));
                };
                let association = self.association_state(second, span)?;
                let cell = self
                    .find_var_cell(first)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{first}`")))?;
                let mut var = cell.borrow_mut();
                match &var.value {
                    Value::Access(None) => {}
                    Value::Access(Some(_)) => {
                        return Err(Error::exception(
                            "CONNECTFAIL",
                            span,
                            "CONNECTFAIL: access location is already connected",
                        ));
                    }
                    _ => return Err(Error::new(span, "expected an ACCESS location")),
                }
                var.value = Value::Access(Some(Rc::new(RefCell::new(AccessState {
                    association: Some(association),
                    position: 0,
                }))));
                Ok(Value::Undefined)
            }
            AssociationOp::Disconnect => {
                let cell = self
                    .find_var_cell(first)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{first}`")))?;
                let mut var = cell.borrow_mut();
                let Value::Access(state) = &mut var.value else {
                    return Err(Error::new(span, "expected an ACCESS location"));
                };
                if state.take().is_none() {
                    return Err(Error::exception(
                        "NOTCONNECTED",
                        span,
                        "NOTCONNECTED: access location is not connected",
                    ));
                }
                Ok(Value::Undefined)
            }
            AssociationOp::ReadRecord => {
                let access = self.access_state(first, span)?;
                let (association, position) = {
                    let access = access.borrow();
                    let Some(association) = access.association.clone() else {
                        return Err(Error::exception(
                            "NOTCONNECTED",
                            span,
                            "NOTCONNECTED: access location is not connected",
                        ));
                    };
                    (association, access.position)
                };
                let path = association.borrow().path.clone();
                let contents = std::fs::read_to_string(&path).map_err(|error| {
                    Error::exception("READFAIL", span, format!("READFAIL: {error}"))
                })?;
                let lines: Vec<&str> = contents.lines().collect();
                let Some(record) = lines.get(position) else {
                    return Err(Error::exception("READFAIL", span, "READFAIL: end of file"));
                };
                access.borrow_mut().position += 1;
                let record = Value::String((*record).to_owned());
                if let Some(target) = second {
                    self.assign_var(target, record, span)?;
                    Ok(Value::Undefined)
                } else {
                    let _ = ty;
                    Ok(record)
                }
            }
            AssociationOp::WriteRecord => {
                let Some(value) = value else {
                    return Err(Error::new(span, "WRITERECORD requires a value"));
                };
                let value = self.eval_ir(value)?;
                let Value::String(record) = value else {
                    return Err(Error::new(span, "WRITERECORD value must be CHARS"));
                };
                let access = self.access_state(first, span)?;
                let association = {
                    let access = access.borrow();
                    let Some(association) = access.association.clone() else {
                        return Err(Error::exception(
                            "NOTCONNECTED",
                            span,
                            "NOTCONNECTED: access location is not connected",
                        ));
                    };
                    association
                };
                let path = association.borrow().path.clone();
                use std::io::Write as _;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|error| {
                        Error::exception("WRITEFAIL", span, format!("WRITEFAIL: {error}"))
                    })?;
                writeln!(file, "{record}").map_err(|error| {
                    Error::exception("WRITEFAIL", span, format!("WRITEFAIL: {error}"))
                })?;
                Ok(Value::Undefined)
            }
        }
    }

    fn association_state(
        &self,
        name: &str,
        span: Span,
    ) -> Result<Rc<RefCell<AssociationState>>, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
        let var = cell.borrow();
        let Value::Association(Some(state)) = &var.value else {
            return Err(Error::exception(
                "NOTASSOCIATED",
                span,
                "NOTASSOCIATED: empty association",
            ));
        };
        Ok(state.clone())
    }

    fn access_state(&self, name: &str, span: Span) -> Result<Rc<RefCell<AccessState>>, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
        let var = cell.borrow();
        let Value::Access(Some(state)) = &var.value else {
            return Err(Error::exception(
                "NOTCONNECTED",
                span,
                "NOTCONNECTED: access location is not connected",
            ));
        };
        Ok(state.clone())
    }

    fn exec_block(
        &mut self,
        stmts: &[ir::Stmt],
        span: Span,
        frame: &mut Frame,
    ) -> Result<Flow, Error> {
        let mut scope = Scope::default();
        let mode_names = self.module_modes[self.active_module].clone();
        self.hoist_block_children(span, &mut scope, &mode_names)?;
        self.seed_reach_exports(span, &mut scope);
        self.locals.push(scope);
        let result = self.exec_stmts(stmts, frame);
        self.locals.pop();
        result
    }

    fn hoist_block_children(
        &mut self,
        span: Span,
        parent: &mut Scope<'a>,
        base_modes: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        let top = self.active_module;
        self.hoist_reach_children(span, parent, top, base_modes, true)
    }

    fn exec_nested_module(
        &mut self,
        label: Option<&str>,
        declarations: &[Declaration],
        actions: &[ir::Stmt],
        span: Span,
    ) -> Result<Flow, Error> {
        if let Some(scope) = self.nested_scopes.remove(&span) {
            return self.exec_hoisted_module(label, declarations, actions, span, scope);
        }

        let parent = self.current_reach();
        self.locals.push(Scope {
            modes: self
                .reaches
                .get(span)
                .map(|node| node.interface.modes.clone())
                .unwrap_or_default(),
            key: Some(span),
            ..Scope::default()
        });
        let previous_floor = self.module_floor;
        let previous_scope = self.active_scope_key;
        self.module_floor = self.locals.len() - 1;
        self.active_scope_key = Some(span);

        let result = (|| {
            self.apply_nested_seizes(declarations, &parent)?;
            self.declare_local_vars(declarations)?;
            self.initialize_declarations(declarations)?;

            let mut frame = Frame {
                result_type: None,
                result: None,
            };
            match self.exec_with_handler(span, actions, &mut frame)? {
                Flow::Normal | Flow::Exit(None) => Ok(Flow::Normal),
                Flow::Exit(Some(name)) if label == Some(name.as_str()) => Ok(Flow::Normal),
                flow => Ok(flow),
            }
        })();

        self.module_floor = previous_floor;
        self.active_scope_key = previous_scope;
        self.locals.pop();
        result
    }

    fn exec_hoisted_module(
        &mut self,
        label: Option<&str>,
        declarations: &[Declaration],
        actions: &[ir::Stmt],
        span: Span,
        scope: Scope<'a>,
    ) -> Result<Flow, Error> {
        let parent = self.current_reach();
        self.locals.push(scope);
        let previous_floor = self.module_floor;
        let previous_scope = self.active_scope_key;
        self.module_floor = self.locals.len() - 1;
        self.active_scope_key = Some(span);

        let result = (|| {
            if !self.locals.last().expect("hoisted scope").seizes_applied {
                self.apply_nested_seizes(declarations, &parent)?;
                self.locals
                    .last_mut()
                    .expect("hoisted scope")
                    .seizes_applied = true;
            }
            self.initialize_declarations(declarations)?;

            let mut frame = Frame {
                result_type: None,
                result: None,
            };
            match self.exec_with_handler(span, actions, &mut frame)? {
                Flow::Normal | Flow::Exit(None) => Ok(Flow::Normal),
                Flow::Exit(Some(name)) if label == Some(name.as_str()) => Ok(Flow::Normal),
                flow => Ok(flow),
            }
        })();

        self.module_floor = previous_floor;
        self.active_scope_key = previous_scope;
        let scope = self.locals.pop().expect("hoisted scope");
        self.nested_scopes.insert(span, scope);
        result
    }

    fn current_reach(&self) -> Scope<'a> {
        if let Some(scope) = self.locals.last() {
            scope.clone()
        } else {
            self.modules[self.active_module].clone()
        }
    }

    fn apply_nested_seizes(
        &mut self,
        declarations: &[Declaration],
        parent: &Scope<'a>,
    ) -> Result<(), Error> {
        let package = self
            .package_names
            .get(self.active_module)
            .and_then(|name| name.clone());
        let module_name = self
            .active_scope_key
            .and_then(|key| self.modules_by_span.get(&key))
            .and_then(|module| module.name.clone());

        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            let old_prefix = program::seize_old_prefix(&visibility.prefix)
                .map_err(|message| Error::new(visibility.span, message))?;

            if visibility.all {
                if old_prefix.is_empty() {
                    for (name, cell) in &parent.vars {
                        self.insert_nested_var(name, cell.clone(), visibility.span)?;
                    }
                    for (name, proc_ref) in &parent.procs {
                        self.insert_nested_proc(name, *proc_ref, visibility.span)?;
                    }
                    for (name, ty) in &parent.modes {
                        self.insert_nested_mode(name, *ty, visibility.span)?;
                    }
                }
                for (name, exports) in &parent.exports {
                    let matching: Vec<&ReachBinding<'a>> = exports
                        .iter()
                        .filter(|binding| {
                            binding.meta.matches(
                                old_prefix,
                                package.as_deref(),
                                module_name.as_deref(),
                            )
                        })
                        .collect();
                    match matching.as_slice() {
                        [] => {}
                        [export] => {
                            self.insert_reach_binding(name, (*export).clone(), visibility.span)?
                        }
                        _ => {
                            return Err(Error::new(
                                visibility.span,
                                format!(
                                    "SEIZE ALL is ambiguous for `{name}`; several sibling modules grant it"
                                ),
                            ));
                        }
                    }
                }
                continue;
            }

            for name in &visibility.names {
                let mut source_prefix = old_prefix.to_vec();
                source_prefix.extend(name.prefix.iter().cloned());

                if source_prefix.is_empty() {
                    if let Some(cell) = parent.vars.get(&name.name) {
                        self.insert_nested_var(&name.name, cell.clone(), name.span)?;
                        continue;
                    }
                    if let Some(proc_ref) = parent.procs.get(&name.name) {
                        self.insert_nested_proc(&name.name, *proc_ref, name.span)?;
                        continue;
                    }
                    if let Some(ty) = parent.modes.get(&name.name) {
                        self.insert_nested_mode(&name.name, *ty, name.span)?;
                        continue;
                    }
                }

                let Some(exports) = parent.exports.get(&name.name) else {
                    return Err(Error::new(
                        name.span,
                        format!(
                            "cannot SEIZE `{}`: no such name in the enclosing reach",
                            name.qualified()
                        ),
                    ));
                };
                let selected = crate::reach::select(
                    exports.iter().map(|binding| (&binding.meta, &binding.kind)),
                    &source_prefix,
                    package.as_deref(),
                    module_name.as_deref(),
                );
                match selected {
                    Ok(kind) => {
                        let binding = ReachBinding {
                            meta: crate::reach::ExportMeta::default(),
                            kind: kind.clone(),
                        };
                        self.insert_reach_binding(&name.name, binding, name.span)?;
                    }
                    Err(crate::reach::SelectError::NotFound) => {
                        return Err(Error::new(
                            name.span,
                            format!(
                                "cannot SEIZE `{}`: no such name in the enclosing reach",
                                name.qualified()
                            ),
                        ));
                    }
                    Err(crate::reach::SelectError::Ambiguous) => {
                        return Err(Error::new(
                            name.span,
                            format!(
                                "SEIZE `{}` is ambiguous; several sibling modules grant it",
                                name.qualified()
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn insert_nested_var(&mut self, name: &str, cell: VarCell, span: Span) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.vars.insert(name.to_owned(), cell);
        Ok(())
    }

    fn insert_nested_proc(
        &mut self,
        name: &str,
        proc_ref: ProcRef<'a>,
        span: Span,
    ) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.procs.insert(name.to_owned(), proc_ref);
        Ok(())
    }

    fn insert_nested_mode(&mut self, name: &str, ty: Type, span: Span) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.modes.insert(name.to_owned(), ty);
        Ok(())
    }

    fn insert_reach_binding(
        &mut self,
        name: &str,
        binding: ReachBinding<'a>,
        span: Span,
    ) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("nested module scope");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
            || scope.exports.contains_key(name)
        {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        match binding.kind {
            ReachKind::Var(cell) => {
                scope.vars.insert(name.to_owned(), cell);
            }
            ReachKind::Proc(proc_ref) => {
                scope.procs.insert(name.to_owned(), proc_ref);
            }
            ReachKind::Mode(ty) => {
                scope.modes.insert(name.to_owned(), ty);
            }
        }
        Ok(())
    }

    fn exec_do(
        &mut self,
        control: &ir::DoControlIr,
        body: &[ir::Stmt],
        frame: &mut Frame,
        span: Span,
    ) -> Result<Flow, Error> {
        match control {
            ir::DoControlIr::None => match self.exec_block(body, span, frame)? {
                Flow::Exit(None) => Ok(Flow::Normal),
                flow => Ok(flow),
            },
            ir::DoControlIr::While(condition) => {
                loop {
                    let value = self.eval_ir(condition)?;
                    if !self.as_bool(value, condition.span())? {
                        break;
                    }
                    match self.exec_block(body, span, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            ir::DoControlIr::ForEver => {
                loop {
                    match self.exec_block(body, span, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            ir::DoControlIr::For {
                counter,
                start,
                end,
                step,
                down,
            } => {
                let start_value = self.eval_ir(start)?;
                let end_value = self.eval_ir(end)?;
                let step_value = match step {
                    Some(expr) => self.eval_ir(expr)?,
                    None => Value::Int {
                        value: 1,
                        kind: None,
                    },
                };
                let step_span = step.as_ref().map(|expr| expr.span()).unwrap_or(span);

                let (start_value, start_kind) = self.expect_int(start_value, start.span())?;
                let (end_value, end_kind) = self.expect_int(end_value, end.span())?;
                let (step_value, step_kind) = self.expect_int(step_value, step_span)?;

                let kind = combine_kinds(start_kind, end_kind, span)?;
                let kind = combine_kinds(kind, step_kind, span)?;

                if step_value <= 0 {
                    return Err(Error::new(step_span, "DO FOR step must be positive"));
                }

                self.make_int(start_value, kind, start.span())?;
                self.make_int(end_value, kind, end.span())?;
                self.make_int(step_value, kind, step_span)?;

                let counter_ty = match kind {
                    Some(kind) => Type::Int(kind),
                    None => Type::IntLiteral,
                };

                self.locals.push(Scope::default());
                let result = (|| -> Result<Flow, Error> {
                    {
                        let scope = self.locals.last_mut().expect("local scope");
                        scope.vars.insert(
                            counter.clone(),
                            Rc::new(RefCell::new(Var {
                                ty: counter_ty,
                                value: Value::Int {
                                    value: start_value,
                                    kind,
                                },
                            })),
                        );
                    }

                    {
                        let mut scope = self.locals.pop().expect("loop counter scope");
                        let mode_names = self.module_modes[self.active_module].clone();
                        self.hoist_block_children(span, &mut scope, &mode_names)?;
                        self.seed_reach_exports(span, &mut scope);
                        self.locals.push(scope);
                    }

                    let mut current = start_value;
                    loop {
                        if !*down && current > end_value {
                            break;
                        }
                        if *down && current < end_value {
                            break;
                        }

                        let cell = self
                            .locals
                            .last()
                            .and_then(|scope| scope.vars.get(counter))
                            .cloned()
                            .expect("loop counter exists");
                        cell.borrow_mut().value = Value::Int {
                            value: current,
                            kind,
                        };

                        match self.exec_stmts(body, frame)? {
                            Flow::Normal => {}
                            Flow::Exit(None) => break,
                            flow => return Ok(flow),
                        }

                        let next = if *down {
                            current.checked_sub(step_value)
                        } else {
                            current.checked_add(step_value)
                        };
                        match next {
                            Some(value) => current = value,
                            // An overflowing next value terminates the loop (§6.5.2).
                            None => break,
                        }
                    }

                    Ok(Flow::Normal)
                })();
                self.locals.pop();
                result
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, Error> {
        let expression = self
            .exprs
            .get(expr.span())
            .ok_or_else(|| Error::new(expr.span(), "internal: missing typed expression"))?;
        self.eval_ir(&expression)
    }

    fn eval_ir(&mut self, expr: &ir::Expr) -> Result<Value, Error> {
        match expr {
            ir::Expr::Integer {
                value, ty, span, ..
            } => self.make_int(*value, type_kind(*ty), *span),
            ir::Expr::Float { value, .. } => Ok(Value::Float(*value)),
            ir::Expr::FloatConversion { arg, span, .. } => {
                let value = self.eval_ir(arg)?;
                match value {
                    Value::Float(value) => Ok(Value::Float(value)),
                    Value::Int { value, .. } => Ok(Value::Float(value as f64)),
                    _ => Err(Error::new(
                        *span,
                        "conversion to FLOAT requires a numeric value",
                    )),
                }
            }
            ir::Expr::Bool { value, .. } => Ok(Value::Bool(*value)),
            ir::Expr::Char { value, .. } => Ok(Value::Char(*value)),
            ir::Expr::String { value, .. } => Ok(Value::String(value.clone())),
            ir::Expr::Name { name, span, .. } => self.read_var(name, *span),
            ir::Expr::Builtin {
                builtin,
                args,
                span,
                ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval_ir(arg)?);
                }
                match builtin {
                    crate::builtins::Builtin::Write => {
                        if let Some(value) = values.first() {
                            let Value::String(text) = value else {
                                return Err(Error::new(*span, "WRITE requires CHARS"));
                            };
                            print!("{text}");
                            use std::io::Write as _;
                            std::io::stdout()
                                .flush()
                                .map_err(|error| Error::new(*span, error.to_string()))?;
                        }
                        Ok(Value::Undefined)
                    }
                    crate::builtins::Builtin::WriteLine => {
                        if let Some(value) = values.first() {
                            let Value::String(text) = value else {
                                return Err(Error::new(*span, "WRITELINE requires CHARS"));
                            };
                            println!("{text}");
                        } else {
                            println!();
                        }
                        Ok(Value::Undefined)
                    }
                    crate::builtins::Builtin::ReadLine => {
                        let mut line = String::new();
                        std::io::stdin()
                            .read_line(&mut line)
                            .map_err(|error| Error::new(*span, error.to_string()))?;
                        let line = line.trim_end_matches(['\n', '\r']).to_owned();
                        Ok(Value::String(line))
                    }
                }
            }
            ir::Expr::TextIo {
                kind,
                target,
                index,
                ops,
                io,
                span,
            } => {
                let mut cursor = match index {
                    Some(index) => {
                        let value = self.eval_ir(index)?;
                        let (index, _) = self.expect_int(value, index.span())?;
                        index
                    }
                    None => 0,
                };
                if cursor < 0 {
                    return Err(textfail(*span, "negative text index"));
                }
                let cell = self
                    .find_var_cell(target)
                    .ok_or_else(|| Error::new(*span, format!("unknown variable `{target}`")))?;
                let source = match &cell.borrow().value {
                    Value::String(text) => text.clone(),
                    _ => return Err(Error::new(*span, "text location must be CHARS")),
                };
                let mut chars: Vec<char> = source.chars().collect();
                let mut io_index = 0;
                match kind {
                    crate::builtins::TextIo::Write => {
                        for op in ops {
                            match op {
                                crate::builtins::FormatOp::Text(text) => {
                                    write_text_at(&mut chars, &mut cursor, text, *span)?;
                                }
                                crate::builtins::FormatOp::Convert(conversion) => {
                                    let width = self.resolve_text_width(
                                        conversion.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    let Some(crate::ir::TextIoArg::Value(argument)) =
                                        io.get(io_index)
                                    else {
                                        return Err(textfail(*span, "missing io list value"));
                                    };
                                    io_index += 1;
                                    let value = self.eval_ir(argument)?;
                                    let representation =
                                        format_value(&value, conversion, width, *span)?;
                                    write_text_at(&mut chars, &mut cursor, &representation, *span)?;
                                }
                                crate::builtins::FormatOp::Edit(editing) => {
                                    let width = self.resolve_text_width(
                                        editing.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    apply_editing_write(
                                        &mut chars,
                                        &mut cursor,
                                        editing,
                                        width,
                                        *span,
                                    )?;
                                }
                            }
                        }
                        let mut var = cell.borrow_mut();
                        let Value::String(text) = &mut var.value else {
                            return Err(Error::new(*span, "text location must be CHARS"));
                        };
                        *text = chars.into_iter().collect();
                    }
                    crate::builtins::TextIo::Read => {
                        for op in ops {
                            match op {
                                crate::builtins::FormatOp::Text(text) => {
                                    read_text_exact(&chars, &mut cursor, text, *span)?;
                                }
                                crate::builtins::FormatOp::Convert(conversion) => {
                                    let width = self.resolve_text_width(
                                        conversion.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    let Some(crate::ir::TextIoArg::Location(name)) =
                                        io.get(io_index)
                                    else {
                                        return Err(textfail(*span, "missing io list location"));
                                    };
                                    io_index += 1;
                                    let target_ty = self
                                        .find_var_cell(name)
                                        .map(|cell| cell.borrow().ty)
                                        .ok_or_else(|| {
                                            Error::new(*span, format!("unknown variable `{name}`"))
                                        })?;
                                    let value = read_conversion(
                                        &chars,
                                        &mut cursor,
                                        conversion,
                                        width,
                                        target_ty,
                                        *span,
                                    )?;
                                    self.assign_var(name, value, *span)?;
                                }
                                crate::builtins::FormatOp::Edit(editing) => {
                                    let width = self.resolve_text_width(
                                        editing.width,
                                        io,
                                        &mut io_index,
                                        *span,
                                    )?;
                                    apply_editing_read(&chars, &mut cursor, editing, width, *span)?;
                                }
                            }
                        }
                    }
                }
                Ok(Value::Undefined)
            }
            ir::Expr::ArrayLiteral { values, array, .. } => {
                let element = array.element.ty();
                let mut lowered = Vec::with_capacity(values.len());
                for node in values {
                    let value = self.eval_ir(node)?;
                    lowered.push(self.convert(value, element, node.span())?);
                }
                Ok(Value::Array {
                    low: array.low,
                    values: lowered,
                })
            }
            ir::Expr::Association {
                op,
                first,
                second,
                value,
                ty,
                span,
            } => self.exec_association(*op, first, second.as_deref(), value.as_ref(), *ty, *span),
            ir::Expr::LabelledArrayLiteral { entries, array, .. } => {
                let element = array.element.ty();
                let mut values =
                    vec![Value::Undefined; (array.high - array.low + 1).max(0) as usize];
                for (indices, node) in entries {
                    if indices.is_empty() {
                        continue;
                    }
                    let value = self.eval_ir(node)?;
                    let value = self.convert(value, element, node.span())?;
                    for index in indices {
                        values[(index - array.low) as usize] = value.clone();
                    }
                }
                Ok(Value::Array {
                    low: array.low,
                    values,
                })
            }
            ir::Expr::ArraySlice {
                name,
                offset,
                count,
                array,
                span,
            } => {
                let value = self.read_var(name, *span)?;
                read_array_slice(value, *offset, *count, array.low, *span)
            }
            ir::Expr::ArraySliceDynamic {
                name,
                first,
                last,
                up,
                low,
                span,
                ..
            } => {
                let value = self.read_var(name, *span)?;
                let len = array_len(&value, *span)?;
                let first_value = self.eval_ir(first)?;
                let (first_value, _) = self.expect_int(first_value, first.span())?;
                let last_value = self.eval_ir(last)?;
                let (last_value, _) = self.expect_int(last_value, last.span())?;
                let (offset, count) =
                    dynamic_slice_range(len, *low, first_value, last_value, *up, *span)?;
                read_array_slice(value, offset, count, *low, *span)
            }
            ir::Expr::Index {
                name,
                indices,
                array,
                span,
                ..
            } => {
                let value = self.read_var(name, *span)?;
                let mut index_values = Vec::with_capacity(indices.len());
                for index in indices {
                    let index_value = self.eval_ir(index)?;
                    let (index, _) = self.expect_int(index_value, index.span())?;
                    index_values.push(index);
                }
                let offset = array_offset(*array, &index_values, *span)?;
                read_array_at(value, offset, *span)
            }
            ir::Expr::Conversion {
                kind, arg, span, ..
            } => {
                let value = self.eval_ir(arg)?;
                let (value, _) = self.expect_int(value, *span)?;
                self.make_int(value, Some(*kind), *span)
            }
            ir::Expr::Call {
                name, args, span, ..
            } => {
                let Some(proc_ref) = self.lookup_proc(name) else {
                    return Err(Error::new(*span, format!("unknown name `{name}`")));
                };
                if proc_ref.def.params.len() != args.len() {
                    return Err(Error::new(
                        *span,
                        format!(
                            "procedure `{name}` expects {} argument(s), found {}",
                            proc_ref.def.params.len(),
                            args.len()
                        ),
                    ));
                }

                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval_ir(arg)?);
                }
                self.call_proc(proc_ref, values, *span)
            }
            ir::Expr::Unary {
                op,
                operand,
                ty,
                span,
            } => {
                let value = self.eval_ir(operand)?;
                match op {
                    UnaryOp::Neg => match value {
                        Value::Float(value) => Ok(Value::Float(-value)),
                        value => {
                            let (value, _) = self.expect_int(value, *span)?;
                            let negated = value
                                .checked_neg()
                                .ok_or_else(|| Error::new(*span, "OVERFLOW in unary `-`"))?;
                            self.make_int(negated, type_kind(*ty), *span)
                        }
                    },
                    UnaryOp::Not => Ok(Value::Bool(!self.as_bool(value, *span)?)),
                }
            }
            ir::Expr::Field {
                base, field, span, ..
            } => {
                let value = self.eval_ir(base)?;
                read_field(value, field, *span)
            }
            ir::Expr::Binary {
                op,
                left,
                right,
                ty,
                span,
            } => match op {
                BinaryOp::AndIf => {
                    let left_value = self.eval_ir(left)?;
                    if !self.as_bool(left_value, *span)? {
                        Ok(Value::Bool(false))
                    } else {
                        let right_value = self.eval_ir(right)?;
                        Ok(Value::Bool(self.as_bool(right_value, *span)?))
                    }
                }
                BinaryOp::OrIf => {
                    let left_value = self.eval_ir(left)?;
                    if self.as_bool(left_value, *span)? {
                        Ok(Value::Bool(true))
                    } else {
                        let right_value = self.eval_ir(right)?;
                        Ok(Value::Bool(self.as_bool(right_value, *span)?))
                    }
                }
                _ => {
                    let left_value = self.eval_ir(left)?;
                    let right_value = self.eval_ir(right)?;
                    self.apply_binary(*op, left_value, right_value, Some(*ty), *span)
                }
            },
        }
    }

    fn call_proc(
        &mut self,
        proc_ref: ProcRef<'a>,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        if self.depth >= 1024 {
            return Err(Error::new(span, "call depth limit exceeded"));
        }

        let previous_module = self.active_module;
        let previous_scope = self.active_scope_key;
        self.active_module = proc_ref.module;
        self.active_scope_key = proc_ref.scope_key;
        self.locals.push(Scope::default());
        self.call_bases.push(self.locals.len() - 1);
        self.depth += 1;

        let result = self.call_proc_inner(proc_ref.def, args, span);

        self.depth -= 1;
        self.call_bases.pop();
        self.locals.pop();
        self.active_module = previous_module;
        self.active_scope_key = previous_scope;
        result
    }

    fn call_proc_inner(
        &mut self,
        proc_def: &'a ProcDef,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        for (param, arg) in proc_def.params.iter().zip(args) {
            let ty = self.resolve_mode(&param.mode)?;
            let value = self.convert(arg, ty, param.span)?;
            let scope = self.locals.last_mut().expect("local scope");
            if scope.vars.contains_key(&param.name) || scope.procs.contains_key(&param.name) {
                return Err(Error::new(
                    param.span,
                    format!("duplicate declaration of `{}`", param.name),
                ));
            }
            scope
                .vars
                .insert(param.name.clone(), Rc::new(RefCell::new(Var { ty, value })));
        }

        self.declare_local(&proc_def.declarations)?;
        self.initialize_declarations(&proc_def.declarations)?;

        let result_type = match &proc_def.result {
            Some(mode) => Some(self.resolve_mode(mode)?),
            None => None,
        };

        let mut frame = Frame {
            result_type,
            result: None,
        };

        let body = self
            .bodies
            .get(proc_def.span)
            .ok_or_else(|| Error::new(proc_def.span, "internal: missing procedure IR body"))?;

        match self.exec_with_handler(proc_def.span, &body, &mut frame)? {
            Flow::Return | Flow::Normal => {}
            Flow::Exit(_) => {
                return Err(Error::new(
                    span,
                    "EXIT outside a loop is not implemented yet",
                ));
            }
        }

        match result_type {
            Some(_) => frame.result.ok_or_else(|| {
                Error::new(
                    span,
                    format!("procedure `{}` returned no result", proc_def.name),
                )
            }),
            None => Ok(Value::Undefined),
        }
    }

    fn apply_binary(
        &self,
        op: BinaryOp,
        left: Value,
        right: Value,
        result_type: Option<Type>,
        span: Span,
    ) -> Result<Value, Error> {
        match op {
            BinaryOp::And | BinaryOp::AndIf => Ok(Value::Bool(
                self.as_bool(left, span)? && self.as_bool(right, span)?,
            )),
            BinaryOp::Or | BinaryOp::OrIf => Ok(Value::Bool(
                self.as_bool(left, span)? || self.as_bool(right, span)?,
            )),
            BinaryOp::Xor => Ok(Value::Bool(
                self.as_bool(left, span)? ^ self.as_bool(right, span)?,
            )),
            BinaryOp::Eq => Ok(Value::Bool(self.values_equal(&left, &right, span)?)),
            BinaryOp::Ne => Ok(Value::Bool(!self.values_equal(&left, &right, span)?)),
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let ordering = self.order_values(&left, &right, span)?;
                let result = match op {
                    BinaryOp::Lt => ordering == Ordering::Less,
                    BinaryOp::Le => ordering != Ordering::Greater,
                    BinaryOp::Gt => ordering == Ordering::Greater,
                    BinaryOp::Ge => ordering != Ordering::Less,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }
            BinaryOp::Concat => match (left, right) {
                (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
                _ => Err(Error::new(span, "`//` requires character strings")),
            },
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Rem
            | BinaryOp::Pow => {
                if let (Value::Float(left), Value::Float(right)) = (&left, &right) {
                    let value = match op {
                        BinaryOp::Add => left + right,
                        BinaryOp::Sub => left - right,
                        BinaryOp::Mul => left * right,
                        BinaryOp::Div => {
                            if *right == 0.0 {
                                return Err(Error::new(span, "division by zero"));
                            }
                            left / right
                        }
                        _ => {
                            return Err(Error::new(
                                span,
                                "this operator does not accept FLOAT operands",
                            ));
                        }
                    };
                    return Ok(Value::Float(value));
                }
                let (left, _) = self.expect_int(left, span)?;
                let (right, _) = self.expect_int(right, span)?;
                let kind = match result_type {
                    Some(Type::Int(kind)) => Some(kind),
                    Some(Type::IntLiteral) | None => None,
                    Some(other) => {
                        return Err(Error::new(
                            span,
                            format!("expected an integer result, found {}", other.describe()),
                        ));
                    }
                };

                let result = match op {
                    BinaryOp::Add => left.checked_add(right),
                    BinaryOp::Sub => left.checked_sub(right),
                    BinaryOp::Mul => left.checked_mul(right),
                    BinaryOp::Div => {
                        if right == 0 {
                            return Err(Error::new(span, "division by zero"));
                        }
                        left.checked_div(right)
                    }
                    BinaryOp::Rem => {
                        if right == 0 {
                            return Err(Error::new(span, "remainder by zero"));
                        }
                        left.checked_rem(right)
                    }
                    BinaryOp::Mod => {
                        if right <= 0 {
                            return Err(Error::new(span, "MOD requires a positive divisor"));
                        }
                        left.checked_rem(right).and_then(|remainder| {
                            if remainder < 0 {
                                remainder.checked_add(right)
                            } else {
                                Some(remainder)
                            }
                        })
                    }
                    BinaryOp::Pow => {
                        if right < 0 {
                            return Err(Error::new(span, "negative exponent"));
                        }
                        let exponent = u32::try_from(right)
                            .map_err(|_| Error::new(span, "exponent is too large"))?;
                        left.checked_pow(exponent)
                    }
                    _ => unreachable!(),
                };

                match result {
                    Some(value) => self.make_int(value, kind, span),
                    None => Err(Error::new(span, "OVERFLOW")),
                }
            }
        }
    }

    fn values_equal(&self, left: &Value, right: &Value, span: Span) -> Result<bool, Error> {
        if matches!(left, Value::Undefined) || matches!(right, Value::Undefined) {
            return Err(Error::new(span, "use of undefined value"));
        }
        match (left, right) {
            (Value::Int { value: left, .. }, Value::Int { value: right, .. }) => Ok(left == right),
            (Value::Float(left), Value::Float(right)) => Ok(left == right),
            (Value::Bool(left), Value::Bool(right)) => Ok(left == right),
            (Value::Char(left), Value::Char(right)) => Ok(left == right),
            (Value::String(left), Value::String(right)) => Ok(left == right),
            (
                Value::Array {
                    low: left_low,
                    values: left_values,
                },
                Value::Array {
                    low: right_low,
                    values: right_values,
                },
            ) => {
                if left_low != right_low || left_values.len() != right_values.len() {
                    return Ok(false);
                }
                for (left, right) in left_values.iter().zip(right_values) {
                    if !self.values_equal(left, right, span)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::Struct(left), Value::Struct(right)) => {
                if left.len() != right.len() {
                    return Err(Error::new(
                        span,
                        "cannot compare structures of different modes",
                    ));
                }
                for ((left_name, left_value), (right_name, right_value)) in left.iter().zip(right) {
                    if left_name != right_name {
                        return Err(Error::new(
                            span,
                            "cannot compare structures of different modes",
                        ));
                    }
                    if !self.values_equal(left_value, right_value, span)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Err(Error::new(span, "cannot compare these values")),
        }
    }

    fn order_values(&self, left: &Value, right: &Value, span: Span) -> Result<Ordering, Error> {
        match (left, right) {
            (Value::Int { value: left, .. }, Value::Int { value: right, .. }) => {
                Ok(left.cmp(right))
            }
            (Value::Char(left), Value::Char(right)) => Ok(left.cmp(right)),
            (Value::Float(left), Value::Float(right)) => left
                .partial_cmp(right)
                .ok_or_else(|| Error::new(span, "cannot order NaN values")),
            (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
            _ => Err(Error::new(span, "cannot order these values")),
        }
    }

    fn as_bool(&self, value: Value, span: Span) -> Result<bool, Error> {
        match value {
            Value::Bool(value) => Ok(value),
            _ => Err(Error::new(span, "expected a BOOL value")),
        }
    }

    fn expect_int(&self, value: Value, span: Span) -> Result<(i128, Option<IntKind>), Error> {
        match value {
            Value::Int { value, kind } => Ok((value, kind)),
            _ => Err(Error::new(span, "expected an integer value")),
        }
    }

    fn make_int(&self, value: i128, kind: Option<IntKind>, span: Span) -> Result<Value, Error> {
        let bounds = match kind {
            Some(kind) => types::int_bounds(kind, self.profile),
            None => types::int_bounds(IntKind::Long, self.profile),
        };

        let Some((low, high)) = bounds else {
            return Err(Error::new(
                span,
                "integer widths above 127 are not supported",
            ));
        };

        if value < low || value > high {
            let target = kind.map(IntKind::name).unwrap_or("&INT");
            return Err(Error::new(
                span,
                format!("OVERFLOW: `{value}` does not fit {target}"),
            ));
        }

        Ok(Value::Int { value, kind })
    }

    fn convert(&self, value: Value, ty: Type, span: Span) -> Result<Value, Error> {
        match ty {
            Type::Float => match value {
                Value::Float(value) => Ok(Value::Float(value)),
                _ => Err(Error::new(span, "expected a FLOAT value")),
            },
            Type::Int(kind) => {
                let (value, _) = self.expect_int(value, span)?;
                self.make_int(value, Some(kind), span)
            }
            Type::Bool => match value {
                Value::Bool(value) => Ok(Value::Bool(value)),
                _ => Err(Error::new(span, "expected a BOOL value")),
            },
            Type::Char => match value {
                Value::Char(value) => Ok(Value::Char(value)),
                _ => Err(Error::new(span, "expected a CHAR value")),
            },
            Type::String => match value {
                Value::String(value) => Ok(Value::String(value)),
                _ => Err(Error::new(span, "expected a character string value")),
            },
            Type::BoundedString { length, varying } => match value {
                Value::String(value) => {
                    let actual = value.chars().count();
                    let fits = if varying {
                        actual <= length
                    } else {
                        actual == length
                    };
                    if fits {
                        Ok(Value::String(value))
                    } else {
                        Err(Error::new(
                            span,
                            format!(
                                "string length {actual} does not fit CHARS({length}){}",
                                if varying { " VARYING" } else { "" }
                            ),
                        ))
                    }
                }
                _ => Err(Error::new(span, "expected a character string value")),
            },
            Type::Struct(_) => match value {
                Value::Struct(_) => Ok(value),
                _ => Err(Error::new(span, "expected a structure value")),
            },
            Type::Array(array) => match value {
                Value::Array { low, values }
                    if low == array.low
                        && values.len() == (array.high - array.low + 1) as usize =>
                {
                    Ok(Value::Array { low, values })
                }
                _ => Err(Error::new(span, "expected an array value")),
            },
            Type::MultiArray(array) => match value {
                Value::Array { low, values }
                    if low == array.dimensions()[0].low
                        && values.len() == array.total_len() as usize =>
                {
                    Ok(Value::Array { low, values })
                }
                _ => Err(Error::new(span, "expected a multidimensional array value")),
            },
            Type::DynamicArray { low, .. } => match value {
                Value::Array {
                    low: value_low,
                    values,
                } if value_low == low => Ok(Value::Array {
                    low: value_low,
                    values,
                }),
                _ => Err(Error::new(span, "expected an array slice value")),
            },
            Type::Association => match value {
                Value::Association(_) => Ok(value),
                _ => Err(Error::new(span, "expected an ASSOCIATION value")),
            },
            Type::Access => match value {
                Value::Access(_) => Ok(value),
                _ => Err(Error::new(span, "expected an ACCESS value")),
            },
            Type::IntLiteral => Err(Error::new(span, "internal: cannot declare a literal type")),
            Type::ArrayTuple => Err(Error::new(span, "array literal has no target array mode")),
        }
    }

    fn resolve_mode(&self, mode: &ModeRef) -> Result<Type, Error> {
        if mode.access {
            return Ok(Type::Access);
        }
        if !mode.arrays.is_empty() {
            let element_mode = ModeRef {
                name: mode.name.clone(),
                arrays: Vec::new(),
                string_length: mode.string_length,
                varying: mode.varying,
                access: mode.access,
                span: mode.span,
            };
            let base = self.resolve_mode(&element_mode)?;
            let Some(element) = crate::types::ArrayElement::from_type(base) else {
                return Err(Error::new(
                    mode.span,
                    format!(
                        "array elements must be scalar or structure modes, found {}",
                        base.describe()
                    ),
                ));
            };
            return crate::types::array_type_from_bounds(&mode.arrays, element)
                .ok_or_else(|| Error::new(mode.span, "unsupported array dimensions"));
        }
        if let Some(length) = mode.string_length {
            return Ok(Type::BoundedString {
                length,
                varying: mode.varying,
            });
        }
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(ty) = scope.modes.get(&mode.name) {
                return Ok(*ty);
            }
        }
        if let Some(scope) = self.active_module_scope() {
            if let Some(ty) = scope.modes.get(&mode.name) {
                return Ok(*ty);
            }
            if let Some(exports) = scope.exports.get(&mode.name) {
                if let [ReachBinding {
                    kind: ReachKind::Mode(ty),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Ok(*ty);
                    }
                }
            }
        }
        resolve_mode_with(mode, &self.modules[self.active_module].modes)
    }

    fn active_module_scope(&self) -> Option<&Scope<'a>> {
        if let Some(key) = self.active_scope_key {
            for scope in self.locals.iter().rev() {
                if scope.key == Some(key) {
                    return Some(scope);
                }
            }
            return self.nested_scopes.get(&key);
        }
        self.modules.get(self.active_module)
    }

    fn target_type(&self, target: &ir::AssignTarget, span: Span) -> Result<Type, Error> {
        match target {
            ir::AssignTarget::Var(name) => {
                let cell = self
                    .find_var_cell(name)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
                let ty = cell.borrow().ty;
                Ok(ty)
            }
            ir::AssignTarget::Field { ty, .. } => Ok(*ty),
            ir::AssignTarget::Index { ty, .. } => Ok(*ty),
            ir::AssignTarget::Slice { ty, .. } | ir::AssignTarget::SliceDynamic { ty, .. } => {
                Ok(*ty)
            }
        }
    }

    fn read_target(&mut self, target: &ir::AssignTarget, span: Span) -> Result<Value, Error> {
        match target {
            ir::AssignTarget::Var(name) => self.read_var(name, span),
            ir::AssignTarget::Field {
                root, index, path, ..
            } => {
                let value = match index {
                    Some(index) => {
                        let value = self.read_var(root, span)?;
                        let index_value = self.eval_ir(index)?;
                        let (index, _) = self.expect_int(index_value, index.span())?;
                        read_array_element(value, index, span)?
                    }
                    None => self.read_var(root, span)?,
                };
                read_path(value, path, span)
            }
            ir::AssignTarget::Index {
                name,
                indices,
                array,
                ..
            } => {
                let value = self.read_var(name, span)?;
                let mut index_values = Vec::with_capacity(indices.len());
                for index in indices {
                    let index_value = self.eval_ir(index)?;
                    let (index, _) = self.expect_int(index_value, index.span())?;
                    index_values.push(index);
                }
                let offset = array_offset(*array, &index_values, span)?;
                read_array_at(value, offset, span)
            }
            ir::AssignTarget::Slice {
                name,
                offset,
                count,
                ty,
            } => {
                let Type::Array(array) = ty else {
                    return Err(Error::new(span, "internal: slice is not an array"));
                };
                let value = self.read_var(name, span)?;
                read_array_slice(value, *offset, *count, array.low, span)
            }
            ir::AssignTarget::SliceDynamic {
                name,
                first,
                last,
                up,
                low,
                ..
            } => {
                let value = self.read_var(name, span)?;
                let len = array_len(&value, span)?;
                let first_value = self.eval_ir(first)?;
                let (first_value, _) = self.expect_int(first_value, first.span())?;
                let last_value = self.eval_ir(last)?;
                let (last_value, _) = self.expect_int(last_value, last.span())?;
                let (offset, count) =
                    dynamic_slice_range(len, *low, first_value, last_value, *up, span)?;
                read_array_slice(value, offset, count, *low, span)
            }
        }
    }

    fn assign_target(
        &mut self,
        target: &ir::AssignTarget,
        value: Value,
        span: Span,
    ) -> Result<(), Error> {
        match target {
            ir::AssignTarget::Var(name) => self.assign_var(name, value, span),
            ir::AssignTarget::Field {
                root,
                index,
                path,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let index = match index {
                    Some(index) => {
                        let index_value = self.eval_ir(index)?;
                        let (index, _) = self.expect_int(index_value, index.span())?;
                        Some(index)
                    }
                    None => None,
                };
                let cell = self
                    .find_var_cell(root)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{root}`")))?;
                let mut var = cell.borrow_mut();
                let target = match index {
                    Some(index) => array_element_mut(&mut var.value, index, span)?,
                    None => &mut var.value,
                };
                assign_path(target, path, value, span)
            }
            ir::AssignTarget::Index {
                name,
                indices,
                array,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let mut index_values = Vec::with_capacity(indices.len());
                for index in indices {
                    let index_value = self.eval_ir(index)?;
                    let (index, _) = self.expect_int(index_value, index.span())?;
                    index_values.push(index);
                }
                let offset = array_offset(*array, &index_values, span)?;
                let cell = self
                    .find_var_cell(name)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
                let mut cell = cell.borrow_mut();
                assign_array_at(&mut cell.value, offset, value, span)
            }
            ir::AssignTarget::Slice {
                name,
                offset,
                count,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let cell = self
                    .find_var_cell(name)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
                let mut var = cell.borrow_mut();
                assign_array_slice(&mut var.value, *offset, *count, value, span)
            }
            ir::AssignTarget::SliceDynamic {
                name,
                first,
                last,
                up,
                low,
                ty,
            } => {
                let value = self.convert(value, *ty, span)?;
                let first_value = self.eval_ir(first)?;
                let (first_value, _) = self.expect_int(first_value, first.span())?;
                let last_value = self.eval_ir(last)?;
                let (last_value, _) = self.expect_int(last_value, last.span())?;
                let cell = self
                    .find_var_cell(name)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
                let mut var = cell.borrow_mut();
                let len = array_len(&var.value, span)?;
                let (offset, count) =
                    dynamic_slice_range(len, *low, first_value, last_value, *up, span)?;
                assign_array_slice(&mut var.value, offset, count, value, span)
            }
        }
    }

    fn read_var(&self, name: &str, span: Span) -> Result<Value, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
        let var = cell.borrow();
        match &var.value {
            Value::Undefined => Err(Error::new(span, format!("use of undefined value `{name}`"))),
            value => Ok(value.clone()),
        }
    }

    fn assign_var(&mut self, name: &str, value: Value, span: Span) -> Result<(), Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
        let ty = cell.borrow().ty;
        let value = self.convert(value, ty, span)?;
        cell.borrow_mut().value = value;
        Ok(())
    }

    fn find_var_cell(&self, name: &str) -> Option<VarCell> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(cell) = scope.vars.get(name) {
                return Some(cell.clone());
            }
            if let Some(exports) = scope.exports.get(name) {
                if let [ReachBinding {
                    kind: ReachKind::Var(cell),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Some(cell.clone());
                    }
                }
            }
        }
        let module = self.active_module_scope()?;
        if let Some(cell) = module.vars.get(name) {
            return Some(cell.clone());
        }
        if let Some(exports) = module.exports.get(name) {
            if let [ReachBinding {
                kind: ReachKind::Var(cell),
                ..
            }] = exports.as_slice()
            {
                return Some(cell.clone());
            }
        }
        None
    }

    fn lookup_proc(&self, name: &str) -> Option<ProcRef<'a>> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(proc_ref) = scope.procs.get(name) {
                return Some(*proc_ref);
            }
        }
        let scope = self.active_module_scope()?;
        if let Some(proc_ref) = scope.procs.get(name) {
            return Some(*proc_ref);
        }
        if let Some(exports) = scope.exports.get(name) {
            if let [ReachBinding {
                kind: ReachKind::Proc(proc_ref),
                meta,
                ..
            }] = exports.as_slice()
            {
                if meta.prefix.is_empty() {
                    return Some(*proc_ref);
                }
            }
        }
        None
    }

    /// Scopes below this index belong to the caller and must not be visible
    /// to a module-level procedure body.
    fn local_floor(&self) -> usize {
        let call_floor = self.call_bases.last().copied().unwrap_or(0);
        call_floor.max(self.module_floor)
    }
}

fn collect_modules_by_span(program: &Program) -> HashMap<Span, &Module> {
    let mut map = HashMap::new();
    for module in &program.modules {
        collect_module_span(module, &mut map);
    }
    map
}

fn collect_module_span<'a>(module: &'a Module, map: &mut HashMap<Span, &'a Module>) {
    map.insert(module.span, module);
    walk_action_modules(&module.actions, &mut |child| {
        collect_module_span(child, map);
    });
}

fn walk_action_modules<'a>(actions: &'a [Action], visit: &mut impl FnMut(&'a Module)) {
    for action in actions {
        match action {
            Action::Module(module) => visit(module),
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    walk_action_modules(&branch.body, visit);
                }
                if let Some(actions) = else_actions {
                    walk_action_modules(actions, visit);
                }
            }
            Action::Do { body, .. } => walk_action_modules(body, visit),
            Action::Case {
                alternatives,
                else_actions,
                ..
            } => {
                for alternative in alternatives {
                    walk_action_modules(&alternative.body, visit);
                }
                if let Some(actions) = else_actions {
                    walk_action_modules(actions, visit);
                }
            }
            Action::Labelled { action, .. } => {
                walk_action_modules(std::slice::from_ref(action), visit)
            }
            Action::Begin { actions, .. } => walk_action_modules(actions, visit),
            Action::Handled { action, .. } => {
                walk_action_modules(std::slice::from_ref(action), visit)
            }
            _ => {}
        }
    }
}

fn declare_into_scope<'a>(
    scope: &mut Scope<'a>,
    declarations: &'a [Declaration],
    module: usize,
    mode_names: &HashMap<String, Type>,
    modes: &crate::modes::ModeTable,
    scope_key: Option<Span>,
) -> Result<(), Error> {
    for declaration in declarations {
        match declaration {
            Declaration::Var(vars) => {
                for var in vars {
                    let ty = resolve_mode_with(&var.mode, mode_names)?;
                    if scope.vars.contains_key(&var.name) || scope.procs.contains_key(&var.name) {
                        return Err(Error::new(
                            var.span,
                            format!("duplicate declaration of `{}`", var.name),
                        ));
                    }
                    scope.vars.insert(
                        var.name.clone(),
                        Rc::new(RefCell::new(Var {
                            ty,
                            value: default_value(modes, ty),
                        })),
                    );
                }
            }
            Declaration::Proc(proc_def) => {
                if scope.vars.contains_key(&proc_def.name)
                    || scope.procs.contains_key(&proc_def.name)
                {
                    return Err(Error::new(
                        proc_def.span,
                        format!("duplicate declaration of `{}`", proc_def.name),
                    ));
                }
                scope.procs.insert(
                    proc_def.name.clone(),
                    ProcRef {
                        def: proc_def,
                        module,
                        scope_key,
                    },
                );
            }
            Declaration::ProcSignature(_)
            | Declaration::Visibility(_)
            | Declaration::Mode(_)
            | Declaration::SpecModule(_) => {}
        }
    }
    Ok(())
}

fn array_offset(array: Type, indices: &[i128], span: Span) -> Result<i128, Error> {
    match array {
        Type::Array(array) => {
            if indices.len() != 1 {
                return Err(Error::new(span, "array element requires one index"));
            }
            let index = indices[0];
            if index < array.low || index > array.high {
                return Err(Error::new(
                    span,
                    format!("array index `{index}` is out of bounds"),
                ));
            }
            Ok(index - array.low)
        }
        Type::MultiArray(array) => {
            if indices.len() != array.rank() {
                return Err(Error::new(
                    span,
                    format!("array element requires {} indices", array.rank()),
                ));
            }
            let mut offset = 0;
            for (index, dim) in indices.iter().zip(array.dimensions()) {
                if *index < dim.low || *index > dim.high {
                    return Err(Error::new(
                        span,
                        format!("array index `{index}` is out of bounds"),
                    ));
                }
                offset = offset * dim.length() + (*index - dim.low);
            }
            Ok(offset)
        }
        _ => Err(Error::new(span, "expected an array value")),
    }
}

fn array_len(value: &Value, span: Span) -> Result<usize, Error> {
    match value {
        Value::Array { values, .. } => Ok(values.len()),
        _ => Err(Error::new(span, "expected an array value")),
    }
}

fn dynamic_slice_range(
    len: usize,
    low: i128,
    first: i128,
    last: i128,
    up: bool,
    span: Span,
) -> Result<(i128, i128), Error> {
    let upper = if up {
        if last < 1 {
            return Err(Error::new(span, "array slice size must be positive"));
        }
        first
            .checked_add(last - 1)
            .ok_or_else(|| Error::new(span, "array slice bounds overflow"))?
    } else {
        last
    };
    let high = low
        .checked_add(len as i128 - 1)
        .ok_or_else(|| Error::new(span, "array slice bounds overflow"))?;
    if first > upper || first < low || upper > high {
        return Err(Error::new(span, "array slice range is out of bounds"));
    }
    Ok((first - low, upper - first + 1))
}

fn read_array_slice(
    value: Value,
    offset: i128,
    count: i128,
    low: i128,
    span: Span,
) -> Result<Value, Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::new(span, "expected an array value"));
    };
    if offset < 0 || count < 0 || offset + count > values.len() as i128 {
        return Err(Error::new(span, "array slice is out of bounds"));
    }
    let start = offset as usize;
    let end = (offset + count) as usize;
    Ok(Value::Array {
        low,
        values: values[start..end].to_vec(),
    })
}

fn read_array_at(value: Value, offset: i128, span: Span) -> Result<Value, Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::new(span, "expected an array value"));
    };
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::new(span, "array index is out of bounds"));
    }
    match &values[offset as usize] {
        Value::Undefined => Err(Error::new(span, "use of undefined array element")),
        value => Ok(value.clone()),
    }
}

fn assign_array_at(
    value: &mut Value,
    offset: i128,
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::new(span, "expected an array value"));
    };
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::new(span, "array index is out of bounds"));
    }
    values[offset as usize] = new_value;
    Ok(())
}

fn assign_array_slice(
    value: &mut Value,
    offset: i128,
    count: i128,
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
    let Value::Array { values, .. } = value else {
        return Err(Error::new(span, "expected an array value"));
    };
    let Value::Array {
        values: new_values, ..
    } = new_value
    else {
        return Err(Error::new(span, "expected an array value"));
    };
    if offset < 0
        || count < 0
        || offset + count > values.len() as i128
        || new_values.len() != count as usize
    {
        return Err(Error::new(span, "array slice is out of bounds"));
    }
    let start = offset as usize;
    for (index, item) in new_values.into_iter().enumerate() {
        values[start + index] = item;
    }
    Ok(())
}

fn read_array_element(value: Value, index: i128, span: Span) -> Result<Value, Error> {
    let Value::Array { low, values } = value else {
        return Err(Error::new(span, "expected an array value"));
    };
    let offset = index - low;
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::new(
            span,
            format!("array index `{index}` is out of bounds"),
        ));
    }
    match &values[offset as usize] {
        Value::Undefined => Err(Error::new(
            span,
            format!("use of undefined array element `{index}`"),
        )),
        value => Ok(value.clone()),
    }
}

fn array_element_mut(value: &mut Value, index: i128, span: Span) -> Result<&mut Value, Error> {
    let Value::Array { low, values } = value else {
        return Err(Error::new(span, "expected an array value"));
    };
    let offset = index - *low;
    if offset < 0 || offset as usize >= values.len() {
        return Err(Error::new(
            span,
            format!("array index `{index}` is out of bounds"),
        ));
    }
    Ok(&mut values[offset as usize])
}

fn read_field(value: Value, field: &str, span: Span) -> Result<Value, Error> {
    let Value::Struct(fields) = value else {
        return Err(Error::new(span, "expected a structure value"));
    };
    let Some((_, value)) = fields.iter().find(|(name, _)| name == field) else {
        return Err(Error::new(span, format!("unknown field `{field}`")));
    };
    match value {
        Value::Undefined => Err(Error::new(
            span,
            format!("use of undefined value `{field}`"),
        )),
        value => Ok(value.clone()),
    }
}

fn read_path(value: Value, path: &[String], span: Span) -> Result<Value, Error> {
    let mut value = value;
    for field in path {
        value = read_field(value, field, span)?;
    }
    Ok(value)
}

fn assign_path(
    value: &mut Value,
    path: &[String],
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
    let Value::Struct(fields) = value else {
        return Err(Error::new(span, "expected a structure value"));
    };
    let Some((_, slot)) = fields.iter_mut().find(|(name, _)| name == &path[0]) else {
        return Err(Error::new(span, format!("unknown field `{}`", path[0])));
    };
    if path.len() == 1 {
        *slot = new_value;
        Ok(())
    } else {
        assign_path(slot, &path[1..], new_value, span)
    }
}

fn resolve_mode_with(mode: &ModeRef, mode_names: &HashMap<String, Type>) -> Result<Type, Error> {
    types::resolve_mode_ref(mode, mode_names).ok_or_else(|| {
        Error::new(
            mode.span,
            format!("unknown or unsupported mode `{}`", mode.name),
        )
    })
}

fn textfail(span: Span, message: &str) -> Error {
    Error::exception("TEXTFAIL", span, format!("TEXTFAIL: {message}"))
}

fn write_text_at(
    chars: &mut Vec<char>,
    cursor: &mut i128,
    text: &str,
    span: Span,
) -> Result<(), Error> {
    if *cursor < 0 {
        return Err(textfail(span, "negative text index"));
    }
    let mut position = *cursor as usize;
    while chars.len() < position {
        chars.push(' ');
    }
    for ch in text.chars() {
        if position < chars.len() {
            chars[position] = ch;
        } else {
            chars.push(ch);
        }
        position += 1;
    }
    *cursor = position as i128;
    Ok(())
}

fn pad_to(chars: &mut Vec<char>, cursor: usize) {
    while chars.len() < cursor {
        chars.push(' ');
    }
}

fn apply_editing_write(
    chars: &mut Vec<char>,
    cursor: &mut i128,
    editing: &crate::builtins::Editing,
    width: usize,
    span: Span,
) -> Result<(), Error> {
    match editing.code {
        crate::builtins::EditingCode::Space => {
            write_text_at(chars, cursor, &" ".repeat(width), span)
        }
        crate::builtins::EditingCode::SkipRight => {
            *cursor += width as i128;
            pad_to(chars, *cursor as usize);
            Ok(())
        }
        crate::builtins::EditingCode::SkipLeft => {
            *cursor -= width as i128;
            if *cursor < 0 {
                return Err(textfail(span, "text index moved before the text"));
            }
            Ok(())
        }
        crate::builtins::EditingCode::Tab => {
            *cursor = width as i128;
            pad_to(chars, *cursor as usize);
            Ok(())
        }
    }
}

fn apply_editing_read(
    chars: &[char],
    cursor: &mut i128,
    editing: &crate::builtins::Editing,
    width: usize,
    span: Span,
) -> Result<(), Error> {
    match editing.code {
        crate::builtins::EditingCode::Space => {
            let start = *cursor as usize;
            let end = start.saturating_add(width);
            if end > chars.len() || !chars[start..end].iter().all(|ch| matches!(ch, ' ' | '\t')) {
                return Err(textfail(span, "expected space characters"));
            }
            *cursor = end as i128;
            Ok(())
        }
        crate::builtins::EditingCode::SkipRight => {
            *cursor += width as i128;
            if *cursor as usize > chars.len() {
                return Err(textfail(span, "text index moved past the text"));
            }
            Ok(())
        }
        crate::builtins::EditingCode::SkipLeft => {
            *cursor -= width as i128;
            if *cursor < 0 {
                return Err(textfail(span, "text index moved before the text"));
            }
            Ok(())
        }
        crate::builtins::EditingCode::Tab => {
            *cursor = width as i128;
            if *cursor as usize > chars.len() {
                return Err(textfail(span, "text index moved past the text"));
            }
            Ok(())
        }
    }
}

fn format_value(
    value: &Value,
    conversion: &crate::builtins::Conversion,
    width: usize,
    span: Span,
) -> Result<String, Error> {
    let representation = match (conversion.code, value) {
        (crate::builtins::ConversionCode::Default, Value::Int { value, .. }) => value.to_string(),
        (crate::builtins::ConversionCode::Binary, Value::Int { value, .. }) => {
            format_integer_base(*value, 2)
        }
        (crate::builtins::ConversionCode::Octal, Value::Int { value, .. }) => {
            format_integer_base(*value, 8)
        }
        (crate::builtins::ConversionCode::Hex, Value::Int { value, .. }) => {
            format_integer_base(*value, 16)
        }
        (crate::builtins::ConversionCode::Default, Value::Bool(value)) => {
            if *value { "TRUE" } else { "FALSE" }.to_owned()
        }
        (crate::builtins::ConversionCode::Default, Value::Char(value)) => value.to_string(),
        (crate::builtins::ConversionCode::Default, Value::String(value)) => value.clone(),
        (crate::builtins::ConversionCode::Default, Value::Float(value)) => {
            match conversion.fractional_width {
                Some(width) => format!("{value:.width$}"),
                None => value.to_string(),
            }
        }
        (crate::builtins::ConversionCode::Scientific, Value::Float(value)) => format_scientific(
            *value,
            conversion.fractional_width,
            conversion.exponent_width,
        ),
        _ => {
            return Err(Error::new(
                span,
                "format conversion is not compatible with the value",
            ));
        }
    };
    Ok(fit_width(representation, width, conversion))
}

fn format_scientific(
    value: f64,
    fractional_width: Option<usize>,
    exponent_width: Option<usize>,
) -> String {
    let formatted = match fractional_width {
        Some(width) => format!("{value:.width$e}"),
        None => format!("{value:e}"),
    };
    let Some((mantissa, exponent)) = formatted.split_once('e') else {
        return formatted;
    };
    let (sign, digits) = if let Some(digits) = exponent.strip_prefix('-') {
        ("-", digits)
    } else if let Some(digits) = exponent.strip_prefix('+') {
        ("+", digits)
    } else {
        ("", exponent)
    };
    let width = exponent_width.unwrap_or(3);
    format!("{mantissa}e{sign}{digits:0>width$}")
}

fn format_integer_base(value: i128, base: u32) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let digits = match base {
        2 => format!("{magnitude:b}"),
        8 => format!("{magnitude:o}"),
        _ => format!("{magnitude:x}"),
    };
    format!("{sign}{digits}")
}

fn fit_width(
    mut representation: String,
    width: usize,
    conversion: &crate::builtins::Conversion,
) -> String {
    if width == 0 {
        return representation;
    }
    let length = representation.chars().count();
    if length > width {
        if conversion.overflow {
            return "*".repeat(width);
        }
        let truncated: String = if conversion.left {
            representation.chars().take(width).collect()
        } else {
            representation.chars().skip(length - width).collect()
        };
        return truncated;
    }
    let padding = width - length;
    if conversion.left {
        representation.extend(std::iter::repeat_n(conversion.pad, padding));
        representation
    } else {
        let mut result: String = std::iter::repeat_n(conversion.pad, padding).collect();
        result.push_str(&representation);
        result
    }
}

fn read_text_exact(chars: &[char], cursor: &mut i128, text: &str, span: Span) -> Result<(), Error> {
    let start = *cursor as usize;
    let expected: Vec<char> = text.chars().collect();
    if start.saturating_add(expected.len()) > chars.len()
        || chars[start..start + expected.len()] != expected[..]
    {
        return Err(textfail(span, "text does not match the format"));
    }
    *cursor += expected.len() as i128;
    Ok(())
}

fn read_conversion(
    chars: &[char],
    cursor: &mut i128,
    conversion: &crate::builtins::Conversion,
    width: usize,
    target: Type,
    span: Span,
) -> Result<Value, Error> {
    let start = *cursor as usize;
    if start > chars.len() {
        return Err(textfail(span, "text index is past the text"));
    }
    if width > 0 {
        let end = start.saturating_add(width);
        if end > chars.len() {
            return Err(textfail(span, "text slice is out of bounds"));
        }
        let field: String = chars[start..end].iter().collect();
        *cursor = end as i128;
        let value = if is_string_type_for_io(target) {
            Value::String(field)
        } else {
            let trimmed = if conversion.left {
                field.trim_end_matches(conversion.pad)
            } else {
                field.trim_start_matches(conversion.pad)
            };
            parse_external(trimmed, conversion, target, span)?
        };
        return Ok(value);
    }

    let value = match target {
        Type::Bool => {
            let available: String = chars[start..].iter().collect();
            let text = available.trim_start_matches([' ', '\t']);
            let skipped = available.len() - text.len();
            *cursor = (start + skipped) as i128;
            let upper = text.to_ascii_uppercase();
            if upper.starts_with("TRUE") {
                *cursor += 4;
                Value::Bool(true)
            } else if upper.starts_with("FALSE") {
                *cursor += 5;
                Value::Bool(false)
            } else {
                return Err(textfail(span, "expected TRUE or FALSE"));
            }
        }
        Type::Char => {
            let Some(ch) = chars.get(start) else {
                return Err(textfail(span, "expected a character"));
            };
            *cursor = (start + 1) as i128;
            Value::Char(*ch)
        }
        Type::String | Type::BoundedString { .. } => {
            let text: String = chars[start..].iter().collect();
            *cursor = chars.len() as i128;
            Value::String(text)
        }
        Type::Float => {
            let mut end = start;
            while end < chars.len() && matches!(chars[end], ' ' | '\t') {
                end += 1;
            }
            let mut token = String::new();
            if end < chars.len() && matches!(chars[end], '+' | '-') {
                token.push(chars[end]);
                end += 1;
            }
            while end < chars.len()
                && (chars[end].is_ascii_digit()
                    || matches!(chars[end], '.' | 'e' | 'E' | '+' | '-'))
            {
                token.push(chars[end]);
                end += 1;
            }
            *cursor = end as i128;
            parse_external(&token, conversion, target, span)?
        }
        Type::Int(_) | Type::IntLiteral => {
            let mut end = start;
            while end < chars.len() && matches!(chars[end], ' ' | '\t') {
                end += 1;
            }
            let mut token = String::new();
            if end < chars.len() && matches!(chars[end], '+' | '-') {
                token.push(chars[end]);
                end += 1;
            }
            while end < chars.len() && is_digit_for(chars[end], &conversion.code) {
                token.push(chars[end]);
                end += 1;
            }
            *cursor = end as i128;
            parse_external(&token, conversion, target, span)?
        }
        _ => {
            return Err(Error::new(
                span,
                "format conversion is not compatible with the target mode",
            ));
        }
    };
    Ok(value)
}

fn is_string_type_for_io(ty: Type) -> bool {
    matches!(ty, Type::String | Type::BoundedString { .. })
}

fn is_digit_for(ch: char, code: &crate::builtins::ConversionCode) -> bool {
    match code {
        crate::builtins::ConversionCode::Binary => matches!(ch, '0' | '1'),
        crate::builtins::ConversionCode::Octal => ('0'..='7').contains(&ch),
        crate::builtins::ConversionCode::Hex => ch.is_ascii_hexdigit(),
        crate::builtins::ConversionCode::Default | crate::builtins::ConversionCode::Scientific => {
            ch.is_ascii_digit()
        }
    }
}

fn parse_external(
    text: &str,
    conversion: &crate::builtins::Conversion,
    target: Type,
    span: Span,
) -> Result<Value, Error> {
    match target {
        Type::Int(kind) => {
            let (negative, digits) = if let Some(rest) = text.strip_prefix('-') {
                (true, rest)
            } else if let Some(rest) = text.strip_prefix('+') {
                (false, rest)
            } else {
                (false, text)
            };
            let base = match conversion.code {
                crate::builtins::ConversionCode::Binary => 2,
                crate::builtins::ConversionCode::Octal => 8,
                crate::builtins::ConversionCode::Hex => 16,
                crate::builtins::ConversionCode::Default
                | crate::builtins::ConversionCode::Scientific => 10,
            };
            let magnitude = i128::from_str_radix(digits, base)
                .map_err(|_| textfail(span, "invalid integer in text"))?;
            let value = if negative { -magnitude } else { magnitude };
            Ok(Value::Int {
                value,
                kind: Some(kind),
            })
        }
        Type::Float => text
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| textfail(span, "invalid floating-point value in text")),
        Type::Bool => match text.to_ascii_uppercase().as_str() {
            "TRUE" => Ok(Value::Bool(true)),
            "FALSE" => Ok(Value::Bool(false)),
            _ => Err(textfail(span, "expected TRUE or FALSE")),
        },
        Type::Char => {
            let mut chars = text.chars();
            let Some(ch) = chars.next() else {
                return Err(textfail(span, "expected a character"));
            };
            if chars.next().is_some() {
                return Err(textfail(span, "expected one character"));
            }
            Ok(Value::Char(ch))
        }
        Type::String | Type::BoundedString { .. } => Ok(Value::String(text.to_owned())),
        _ => Err(Error::new(span, "unsupported text conversion target")),
    }
}

fn default_value(modes: &crate::modes::ModeTable, ty: Type) -> Value {
    match ty {
        Type::Struct(id) => Value::Struct(
            modes
                .get(id)
                .fields
                .iter()
                .map(|field| (field.name.clone(), default_value(modes, field.ty)))
                .collect(),
        ),
        Type::Array(array) => {
            let element = array.element.ty();
            let value = if matches!(element, Type::Struct(_)) {
                default_value(modes, element)
            } else {
                Value::Undefined
            };
            Value::Array {
                low: array.low,
                values: vec![value; (array.high - array.low + 1).max(0) as usize],
            }
        }
        Type::Association => Value::Association(None),
        Type::Access => Value::Access(None),
        Type::MultiArray(array) => {
            let element = array.element.ty();
            let value = if matches!(element, Type::Struct(_)) {
                default_value(modes, element)
            } else {
                Value::Undefined
            };
            Value::Array {
                low: array.dimensions()[0].low,
                values: vec![value; array.total_len().max(0) as usize],
            }
        }
        _ => Value::Undefined,
    }
}

fn binary_for_assign(op: AssignOp) -> BinaryOp {
    match op {
        AssignOp::Assign => BinaryOp::Eq,
        AssignOp::Add => BinaryOp::Add,
        AssignOp::Sub => BinaryOp::Sub,
        AssignOp::Mul => BinaryOp::Mul,
        AssignOp::Div => BinaryOp::Div,
        AssignOp::Concat => BinaryOp::Concat,
    }
}

fn explicit_label_matches(label: CaseLabel, value: i128) -> bool {
    match label {
        CaseLabel::Value(label_value) => label_value == value,
        CaseLabel::Range { low, high } => low <= value && value <= high,
        CaseLabel::Else | CaseLabel::Any => false,
    }
}

fn type_kind(ty: Type) -> Option<IntKind> {
    match ty {
        Type::Int(kind) => Some(kind),
        Type::IntLiteral => None,
        _ => None,
    }
}

fn combine_kinds(
    left: Option<IntKind>,
    right: Option<IntKind>,
    span: Span,
) -> Result<Option<IntKind>, Error> {
    match (left, right) {
        (Some(left), Some(right)) if left == right => Ok(Some(left)),
        (Some(left), Some(right)) => Err(Error::new(
            span,
            format!(
                "integer mode mismatch: {} and {}",
                left.name(),
                right.name()
            ),
        )),
        (Some(kind), None) | (None, Some(kind)) => Ok(Some(kind)),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::manifest::WidthProfile;
    use crate::parser;

    fn run_source(source: &str) -> Result<(), Error> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        run(&program, WidthProfile::default())
    }

    #[test]
    fn runs_hello_example() {
        run_source(include_str!("../examples/hello/src/main.ch")).unwrap();
    }

    #[test]
    fn assert_failure_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  ASSERT 1 = 2;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0501");
        assert_eq!(error.message, "ASSERTFAIL");
    }

    #[test]
    fn integer_overflow_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  DCL x SHORT_INT INIT := 32767;
  ASSERT x + 1 = 32768;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("OVERFLOW"));
    }

    #[test]
    fn procedure_result_is_returned() {
        run_source(
            r#"
M: MODULE
  Add: PROC (a INT, b INT) RETURNS (INT);
    RETURN (a + b);
  END Add;
  ASSERT Add(2, 3) = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_across_modules() {
        run_source(
            r#"
A: MODULE
  GRANT x, F;
  DCL x INT INIT := 41;
  DCL secret INT INIT := 1;
  F: PROC () RETURNS (INT);
    RETURN (x + secret);
  END F;
END A;

B: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
  x := x + 1;
END B;

C: MODULE
  SEIZE x;
  ASSERT x = 42;
END C;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_prefixed_exports() {
        run_source(
            r#"
A: MODULE
  GRANT ALL PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE P!a;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_nested_modules_and_shares_seized_locations() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  Helper: MODULE
    SEIZE x;
    x := x + 1;
  END Helper;
  ASSERT x = 2;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_procedures_and_modes_can_be_granted_outward() {
        run_source(
            r#"
M: MODULE
  GRANT P, S;
  A: MODULE
    GRANT P, S;
    NEWMODE S = STRUCT(x INT);
    P: PROC () RETURNS (INT);
      RETURN 1;
    END P;
  END A;
END M;

N: MODULE
  SEIZE P, S;
  DCL s S;
  s.x := 1;
  ASSERT s.x = 1;
  ASSERT P() = 1;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_modules_can_define_procedures() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  A: MODULE
    SEIZE x;
    Add: PROC () RETURNS (INT);
      RETURN x + 1;
    END Add;
    ASSERT Add() = 2;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn program_normalization_is_idempotent() {
        let tokens = lexer::tokenize(
            r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
"#,
        )
        .unwrap();
        let program = parser::parse(&tokens).unwrap();
        let once = crate::program::normalize(&program).unwrap();
        let twice = crate::program::normalize(&once).unwrap();
        assert_eq!(once.modules.len(), twice.modules.len());
    }

    #[test]
    fn direct_entry_points_apply_virtual_grants() {
        run_source(
            r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_modules_can_define_modes() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    NEWMODE S = STRUCT(x INT);
    DCL s S;
    s.x := 1;
    ASSERT s.x = 1;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_modules_can_seize_parent_modes() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  GRANT S;
  A: MODULE
    SEIZE S;
    DCL s S;
    s.x := 2;
    ASSERT s.x = 2;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_nested_modules_with_contexts() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 3;
  CONTEXT
    DCL x INT;
  FOR
  Helper: MODULE
    SEIZE x;
    ASSERT x = 3;
  END Helper;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_regrants_a_child_grant_at_runtime() {
        run_source(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
END M;

N: MODULE
  SEIZE x;
  ASSERT x = 1;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_regrants_a_grandchild_grant_at_runtime() {
        run_source(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    B: MODULE
      GRANT x;
      DCL x INT INIT := 2;
    END B;
  END A;
END M;

N: MODULE
  SEIZE x;
  ASSERT x = 2;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_reach_can_use_a_child_grant() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
  ASSERT x = 1;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_parent_can_use_a_grandchild_grant() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    B: MODULE
      GRANT x;
      DCL x INT INIT := 2;
    END B;
    ASSERT x = 2;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_a_begin_share_the_block_reach() {
        run_source(
            r#"
M: MODULE
  BEGIN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  END;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_a_do_loop_share_the_body_reach() {
        run_source(
            r#"
M: MODULE
  DO FOR i := 1 TO 1;
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  OD;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_a_case_alternative_share_its_reach() {
        run_source(
            r#"
M: MODULE
  DCL k INT INIT := 1;
  CASE k OF
    (1):
      A: MODULE
        GRANT x;
        DCL x INT INIT := 1;
      END A;
      B: MODULE
        SEIZE x;
        ASSERT x = 1;
      END B;
    (ELSE): ;
  ESAC;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_an_if_branch_share_the_branch_reach() {
        run_source(
            r#"
M: MODULE
  DCL flag BOOL INIT := TRUE;
  IF flag THEN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  FI;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grant_prefixes_are_seizable_at_runtime() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x PREFIXED P;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x PREFIXED P;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grant_to_friends_works_at_runtime() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x TO B;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grants_are_shared_with_siblings() {
        run_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;

  B: MODULE
    SEIZE x;
    x := 2;
  END B;

  C: MODULE
    SEIZE x;
    ASSERT x = 2;
  END C;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_module_locals_are_scoped() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 5;
  Helper: MODULE
    DCL y INT INIT := 2;
    ASSERT y = 2;
  END Helper;
  ASSERT x = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn context_visibility_has_no_runtime_effect() {
        run_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 4;
END A;

CONTEXT
  DCL x INT;
FOR
CONTEXT
  SEIZE x;
FOR
B: MODULE
  SEIZE x;
  ASSERT x = 4;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn contexts_have_no_runtime_effect() {
        run_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 4;
END A;

CONTEXT
  DCL x INT;
FOR
B: MODULE
  SEIZE x;
  ASSERT x = 4;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn compares_structure_values_recursively() {
        run_source(
            r#"
M: MODULE
  NEWMODE Inner = STRUCT(v INT);
  NEWMODE Outer = STRUCT(inner Inner, tag BOOL);
  DCL a Outer, b Outer;
  a.inner.v := 1;
  a.tag := TRUE;
  b := a;
  ASSERT a = b;
  ASSERT NOT (a /= b);
  b.inner.v := 2;
  ASSERT a /= b;
  ASSERT NOT (a = b);
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn comparing_undefined_structure_field_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL a S, b S;
  ASSERT a = b;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0504");
        assert!(error.message.contains("undefined value"));
    }

    #[test]
    fn seizes_with_prefixed_clause() {
        run_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE a PREFIXED P;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_with_old_prefix_rename() {
        run_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 5;
END A;

B: MODULE
  SEIZE ( P -> ) ! a;
  ASSERT a = 5;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_all_with_prefixed_clause() {
        run_source(
            r#"
A: MODULE
  GRANT a, b PREFIXED P;
  DCL a INT INIT := 1;
  DCL b INT INIT := 2;
END A;

B: MODULE
  SEIZE ALL PREFIXED P;
  ASSERT a + b = 3;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seizes_qualified_exports_and_all() {
        run_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

B: MODULE
  GRANT y;
  DCL y INT INIT := 2;
END B;

C: MODULE
  SEIZE A!x;
  ASSERT x = 1;
END C;

D: MODULE
  SEIZE ALL;
  ASSERT x = 1;
  ASSERT y = 2;
END D;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_spec_and_body() {
        run_source(
            r#"
Lib: SPEC MODULE
  GRANT x, F;
  DCL x INT;
  F: PROC () RETURNS (INT) END;
END Lib;

Lib: MODULE
  DCL x INT INIT := 41;
  F: PROC () RETURNS (INT);
    RETURN (x + 1);
  END F;
END Lib;

Client: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
END Client;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_do_for_ascending_and_descending() {
        run_source(
            r#"
M: MODULE
  DCL sum INT INIT := 0;
  DCL count INT INIT := 0;
  DO FOR i := 1 TO 4;
    sum := sum + i;
  OD;
  ASSERT sum = 10;

  DO FOR i := 4 BY 1 DOWN TO 1;
    count := count + 1;
  OD;
  ASSERT count = 4;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn runs_case_actions() {
        run_source(
            r#"
M: MODULE
  DCL grade INT INIT := 2;
  DCL score INT INIT := 0;

  CASE grade OF
    (1): score := 10;
    (2, 3): score := 20;
    (4:6): score := 30;
    (ELSE): score := 99;
  ESAC;
  ASSERT score = 20;

  grade := 5;
  CASE grade OF
    (1): score := 10;
    (4:6): score := 30;
  ESAC;
  ASSERT score = 30;

  grade := 7;
  CASE grade OF
    (1): score := 10;
    ELSE
      score := 0;
  ESAC;
  ASSERT score = 0;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn labelled_exit_leaves_outer_loop() {
        run_source(
            r#"
M: MODULE
  DCL count INT INIT := 0;
Outer: DO FOR i := 1 TO 3;
    DO FOR j := 1 TO 3;
      count := count + 1;
      IF count = 4 THEN
        EXIT Outer;
      FI;
    OD;
  OD;
  ASSERT count = 4;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn bare_exit_leaves_begin_not_loop() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 0;
  DO FOR i := 1 TO 3;
    BEGIN
      x := x + 1;
      EXIT;
    END;
    x := x + 10;
  OD;
  ASSERT x = 33;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn writetext_and_readtext_on_chars_locations() {
        run_source(
            r#"
M: MODULE
  DCL s CHARS INIT := "hello";
  DCL t CHARS INIT := "";
  DCL u CHARS INIT := "abc";
  WRITETEXT(s, 1, "i");
  ASSERT s = "hillo";
  READTEXT(s, 1, "i");
  WRITETEXT(t, 2, "x");
  ASSERT t = "  x";
  WRITETEXT(u, "xy");
  ASSERT u = "xyc";
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn readtext_mismatch_raises_textfail() {
        run_source(
            r#"
M: MODULE
  DCL s CHARS INIT := "abc";
  READTEXT(s, 1, "z");
  ASSERT FALSE;
END
ON (TEXTFAIL):
  ASSERT TRUE;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn module_handler_catches_cause() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 0;
  CAUSE boom;
  x := 1;
END
ON (boom):
  x := 2;
  ASSERT x = 2;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn procedure_handler_can_deliver_a_result() {
        run_source(
            r#"
M: MODULE
  P: PROC () RETURNS (INT);
    CAUSE fail;
    RETURN 1;
  END
  ON (fail):
    RESULT 7;
  END P;
  ASSERT P() = 7;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn handler_catches_overflow() {
        run_source(
            r#"
M: MODULE
  DCL a SHORT_INT INIT := 32767;
  a +:= 1;
  ASSERT FALSE;
END
ON (OVERFLOW):
  ASSERT TRUE;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn action_handler_runs_in_the_begin_reach() {
        run_source(
            r#"
M: MODULE
  BEGIN
    DCL outer INT INIT := 1;
    BEGIN
      DCL inner INT INIT := 2;
      CAUSE fail;
    END
    ON (fail):
      ASSERT outer + inner = 3;
    END;
  END;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn action_handler_only_handles_matching_exceptions() {
        let error = run_source(
            r#"
M: MODULE
  BEGIN
    CAUSE unexpected;
  END
  ON (handled):
    ASSERT TRUE;
  END;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0509");
        assert!(error.message.contains("unexpected"));
    }

    #[test]
    fn unhandled_exception_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  CAUSE boom;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0509");
        assert!(error.message.contains("unhandled exception `boom`"));
    }

    #[test]
    fn nested_exception_propagates_to_the_enclosing_module_handler() {
        run_source(
            r#"
M: MODULE
  DCL x INT INIT := 0;
  A: MODULE
    CAUSE boom;
  END A;
  x := 1;
END
ON (boom):
  x := 3;
  ASSERT x = 3;
END;
"#,
        )
        .unwrap();
    }

    #[test]
    fn arrays_support_indexing_bounds_and_scalars() {
        run_source(
            r#"
M: MODULE
  DCL a ARRAY (1:3) INT;
  DCL s ARRAY (0:1) CHARS;
  DCL n ARRAY (-1:1) INT;
  a(1) := 10;
  a(2) := 20;
  a(3) := a(1) + a(2);
  ASSERT a(3) = 30;
  s(0) := "x";
  ASSERT s(0) = "x";
  n(-1) := 5;
  ASSERT n(-1) = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn array_index_out_of_bounds_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  DCL a ARRAY (1:2) INT;
  a(0) := 1;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("out of bounds"), "{}", error.message);
    }

    #[test]
    fn char_and_chars_modes_work() {
        run_source(
            r#"
M: MODULE
  DCL c CHAR INIT := 'A';
  DCL s CHARS INIT := "hi";
  s := s // "!";
  ASSERT s = "hi!";
  ASSERT c = 'A';
  c := 'B';
  ASSERT c = 'B';
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn structure_fields_can_be_assigned_and_read() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT, y BOOL);
  DCL s S;
  s.x := 2;
  s.y := TRUE;
  ASSERT s.x = 2;
  ASSERT s.y;
  s.x := s.x + 3;
  ASSERT s.x = 5;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_structure_fields_and_whole_value_copy() {
        run_source(
            r#"
M: MODULE
  NEWMODE Inner = STRUCT(v INT);
  NEWMODE Outer = STRUCT(inner Inner, tag BOOL);
  DCL a Outer, b Outer;
  a.inner.v := 7;
  a.tag := TRUE;
  b := a;
  b.inner.v := 9;
  ASSERT a.inner.v = 7;
  ASSERT b.inner.v = 9;
  ASSERT b.tag;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seized_structure_mode_is_usable_at_runtime() {
        run_source(
            r#"
A: MODULE
  NEWMODE S = STRUCT(x INT);
  GRANT S;
END A;

B: MODULE
  SEIZE S;
  DCL s S;
  s.x := 4;
  ASSERT s.x = 4;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn procedures_can_return_structures() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);

  Make: PROC () RETURNS (S);
    DCL tmp S;
    tmp.x := 11;
    RETURN tmp;
  END Make;

  DCL s S INIT := Make();
  ASSERT s.x = 11;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn structure_field_compound_assignment() {
        run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL s S;
  s.x := 1;
  s.x +:= 2;
  ASSERT s.x = 3;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn undefined_structure_field_is_reported() {
        let error = run_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL s S;
  ASSERT s.x = 1;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0504");
        assert!(error.message.contains("undefined value `x`"));
    }
}
