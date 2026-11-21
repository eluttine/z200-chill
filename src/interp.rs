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
        module_modes_by_span: info.module_modes_by_span,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Undefined,
    Int { value: i128, kind: Option<IntKind> },
    Bool(bool),
    Char(char),
    String(String),
    Struct(Vec<(String, Value)>),
    Array { low: i128, values: Vec<Value> },
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
    module_modes_by_span: HashMap<Span, HashMap<String, Type>>,
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
        let modes = Rc::clone(&self.modes);
        self.hoist_children(&module.actions, &mut parent, index, &mode_names, &modes)?;
        self.modules[index] = parent;
        Ok(())
    }

    fn hoist_children(
        &mut self,
        actions: &'a [Action],
        parent: &mut Scope<'a>,
        top: usize,
        mode_names: &HashMap<String, Type>,
        modes: &crate::modes::ModeTable,
    ) -> Result<(), Error> {
        for action in actions {
            let Action::Module(child) = action else {
                continue;
            };
            let mut child_modes = self
                .module_modes_by_span
                .get(&child.span)
                .cloned()
                .unwrap_or_default();
            child_modes.extend(parent.modes.clone());
            child_modes.extend(mode_names.clone());
            let mut scope = Scope {
                modes: self
                    .module_modes_by_span
                    .get(&child.span)
                    .cloned()
                    .unwrap_or_default(),
                key: Some(child.span),
                ..Scope::default()
            };
            declare_into_scope(
                &mut scope,
                &child.declarations,
                top,
                &child_modes,
                modes,
                Some(child.span),
            )?;
            self.hoist_children(&child.actions, &mut scope, top, mode_names, modes)?;
            register_hoisted_grants(child, &scope, parent)?;
            self.nested_scopes.insert(child.span, scope);
        }
        Ok(())
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
                    ..
                } => {
                    for branch in branches {
                        let condition = self.eval_ir(&branch.condition)?;
                        if self.as_bool(condition, branch.condition.span())? {
                            return self.exec_block(&branch.body, frame);
                        }
                    }
                    if let Some(actions) = else_actions {
                        return self.exec_block(actions, frame);
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
                            return self.exec_block(&alternative.body, frame);
                        }
                    }

                    if let Some(actions) = else_actions {
                        return self.exec_block(actions, frame);
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
                    ..
                } => {
                    self.locals.push(Scope::default());
                    let result = (|| {
                        self.declare_local_vars(declarations)?;
                        self.initialize_declarations(declarations)?;
                        let mut scope = self.locals.pop().expect("begin scope");
                        let mode_names = self.module_modes[self.active_module].clone();
                        self.hoist_ir_children(actions, &mut scope, &mode_names)?;
                        self.locals.push(scope);
                        self.exec_stmts(actions, frame)
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
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_block(&mut self, stmts: &[ir::Stmt], frame: &mut Frame) -> Result<Flow, Error> {
        let mut scope = Scope::default();
        let mode_names = self.module_modes[self.active_module].clone();
        self.hoist_ir_children(stmts, &mut scope, &mode_names)?;
        self.locals.push(scope);
        let result = self.exec_stmts(stmts, frame);
        self.locals.pop();
        result
    }

    fn hoist_ir_children(
        &mut self,
        stmts: &[ir::Stmt],
        parent: &mut Scope<'a>,
        base_modes: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        for stmt in stmts {
            let ir::Stmt::Module { span, .. } = stmt else {
                continue;
            };
            let Some(module) = self.modules_by_span.get(span).copied() else {
                continue;
            };
            let modes = Rc::clone(&self.modes);
            let top = self.active_module;
            let mut child_modes = parent.modes.clone();
            child_modes.extend(base_modes.clone());
            let mut scope = Scope {
                modes: self
                    .module_modes_by_span
                    .get(span)
                    .cloned()
                    .unwrap_or_default(),
                key: Some(*span),
                ..Scope::default()
            };
            child_modes.extend(scope.modes.clone());
            declare_into_scope(
                &mut scope,
                &module.declarations,
                top,
                &child_modes,
                &modes,
                Some(*span),
            )?;
            self.hoist_children(&module.actions, &mut scope, top, base_modes, &modes)?;
            register_hoisted_grants(module, &scope, parent)?;
            self.nested_scopes.insert(*span, scope);
        }
        Ok(())
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

        let parent_is_local = !self.locals.is_empty();
        let parent = self.current_reach();
        self.locals.push(Scope {
            modes: self
                .module_modes_by_span
                .get(&span)
                .cloned()
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
            self.register_nested_grants(declarations, parent_is_local)?;

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

    fn register_nested_grants(
        &mut self,
        declarations: &[Declaration],
        parent_is_local: bool,
    ) -> Result<(), Error> {
        let module_name = self
            .active_scope_key
            .and_then(|key| self.modules_by_span.get(&key))
            .and_then(|module| module.name.clone());
        let scope = self.locals.last().cloned().unwrap_or_default();
        let mut grantable: HashMap<String, ReachKind<'a>> = HashMap::new();
        for (name, cell) in &scope.vars {
            grantable.insert(name.clone(), ReachKind::Var(cell.clone()));
        }
        for (name, proc_ref) in &scope.procs {
            grantable.insert(name.clone(), ReachKind::Proc(*proc_ref));
        }
        for (name, ty) in &scope.modes {
            grantable.insert(name.clone(), ReachKind::Mode(*ty));
        }
        for (name, exports) in &scope.exports {
            if let [export] = exports.as_slice() {
                if export.meta.prefix.is_empty() {
                    grantable
                        .entry(name.clone())
                        .or_insert_with(|| export.kind.clone());
                }
            }
        }

        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Grant {
                continue;
            }
            let prefix = reach_grant_prefix(visibility, module_name.as_deref())?;
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
                let Some(kind) = grantable.get(&name).cloned() else {
                    return Err(Error::new(
                        span,
                        format!("cannot GRANT `{name}`: not a declaration of this module"),
                    ));
                };
                let binding = ReachBinding {
                    kind,
                    meta: crate::reach::ExportMeta {
                        prefix: prefix.clone(),
                        friends: friends.clone(),
                    },
                };
                let parent_index = self.locals.len().checked_sub(2);
                let parent_scope = if parent_is_local {
                    let index = parent_index.expect("parent scope exists");
                    &mut self.locals[index]
                } else {
                    &mut self.modules[self.active_module]
                };
                parent_scope
                    .exports
                    .entry(name.clone())
                    .or_default()
                    .push(binding);
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
            ir::DoControlIr::None => match self.exec_block(body, frame)? {
                Flow::Exit(None) => Ok(Flow::Normal),
                flow => Ok(flow),
            },
            ir::DoControlIr::While(condition) => {
                loop {
                    let value = self.eval_ir(condition)?;
                    if !self.as_bool(value, condition.span())? {
                        break;
                    }
                    match self.exec_block(body, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            ir::DoControlIr::ForEver => {
                loop {
                    match self.exec_block(body, frame)? {
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
                        self.hoist_ir_children(body, &mut scope, &mode_names)?;
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
            ir::Expr::Bool { value, .. } => Ok(Value::Bool(*value)),
            ir::Expr::Char { value, .. } => Ok(Value::Char(*value)),
            ir::Expr::String { value, .. } => Ok(Value::String(value.clone())),
            ir::Expr::Name { name, span, .. } => self.read_var(name, *span),
            ir::Expr::Index {
                name, index, span, ..
            } => {
                let value = self.read_var(name, *span)?;
                let index_value = self.eval_ir(index)?;
                let (index, _) = self.expect_int(index_value, index.span())?;
                read_array_element(value, index, *span)
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
                    UnaryOp::Neg => {
                        let (value, _) = self.expect_int(value, *span)?;
                        let negated = value
                            .checked_neg()
                            .ok_or_else(|| Error::new(*span, "OVERFLOW in unary `-`"))?;
                        self.make_int(negated, type_kind(*ty), *span)
                    }
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
            Type::IntLiteral => Err(Error::new(span, "internal: cannot declare a literal type")),
        }
    }

    fn resolve_mode(&self, mode: &ModeRef) -> Result<Type, Error> {
        if let Some(bounds) = mode.array {
            let element_mode = ModeRef {
                name: mode.name.clone(),
                array: None,
                span: mode.span,
            };
            let base = self.resolve_mode(&element_mode)?;
            let Some(element) = base.scalar() else {
                return Err(Error::new(
                    mode.span,
                    format!(
                        "array elements must be scalar modes, found {}",
                        base.describe()
                    ),
                ));
            };
            return Ok(Type::Array(crate::types::ArrayType {
                low: bounds.low,
                high: bounds.high,
                element,
            }));
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
        }
    }

    fn read_target(&mut self, target: &ir::AssignTarget, span: Span) -> Result<Value, Error> {
        match target {
            ir::AssignTarget::Var(name) => self.read_var(name, span),
            ir::AssignTarget::Field { root, path, .. } => self.read_field_path(root, path, span),
            ir::AssignTarget::Index { name, index, .. } => {
                let value = self.read_var(name, span)?;
                let index_value = self.eval_ir(index)?;
                let (index, _) = self.expect_int(index_value, index.span())?;
                read_array_element(value, index, span)
            }
        }
    }

    fn read_field_path(&self, root: &str, path: &[String], span: Span) -> Result<Value, Error> {
        let cell = self
            .find_var_cell(root)
            .ok_or_else(|| Error::new(span, format!("unknown variable `{root}`")))?;
        let value = match &cell.borrow().value {
            Value::Undefined => {
                return Err(Error::new(span, format!("use of undefined value `{root}`")));
            }
            value => value.clone(),
        };
        read_path(value, path, span)
    }

    fn assign_target(
        &mut self,
        target: &ir::AssignTarget,
        value: Value,
        span: Span,
    ) -> Result<(), Error> {
        match target {
            ir::AssignTarget::Var(name) => self.assign_var(name, value, span),
            ir::AssignTarget::Field { root, path, ty } => {
                let value = self.convert(value, *ty, span)?;
                let cell = self
                    .find_var_cell(root)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{root}`")))?;
                let mut var = cell.borrow_mut();
                assign_path(&mut var.value, path, value, span)
            }
            ir::AssignTarget::Index { name, index, ty } => {
                let value = self.convert(value, *ty, span)?;
                let index_value = self.eval_ir(index)?;
                let (index, _) = self.expect_int(index_value, index.span())?;
                let cell = self
                    .find_var_cell(name)
                    .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
                let mut var = cell.borrow_mut();
                assign_array_element(&mut var.value, index, value, span)
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

fn register_hoisted_grants<'a>(
    module: &Module,
    scope: &Scope<'a>,
    parent: &mut Scope<'a>,
) -> Result<(), Error> {
    register_grants(&module.declarations, module.name.as_deref(), scope, parent)
}

fn register_grants<'a>(
    declarations: &[Declaration],
    module_name: Option<&str>,
    scope: &Scope<'a>,
    parent: &mut Scope<'a>,
) -> Result<(), Error> {
    let mut grantable: HashMap<String, ReachKind<'a>> = HashMap::new();
    for (name, cell) in &scope.vars {
        grantable.insert(name.clone(), ReachKind::Var(cell.clone()));
    }
    for (name, proc_ref) in &scope.procs {
        grantable.insert(name.clone(), ReachKind::Proc(*proc_ref));
    }
    for (name, ty) in &scope.modes {
        grantable.insert(name.clone(), ReachKind::Mode(*ty));
    }
    for (name, exports) in &scope.exports {
        if let [export] = exports.as_slice() {
            if export.meta.prefix.is_empty() {
                grantable
                    .entry(name.clone())
                    .or_insert_with(|| export.kind.clone());
            }
        }
    }

    for declaration in declarations {
        let Declaration::Visibility(visibility) = declaration else {
            continue;
        };
        if visibility.kind != VisibilityKind::Grant {
            continue;
        }
        let prefix = reach_grant_prefix(visibility, module_name)?;
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
            let Some(kind) = grantable.get(&name).cloned() else {
                return Err(Error::new(
                    span,
                    format!(
                        "cannot GRANT `{name}`: not a declaration or child grant in this reach"
                    ),
                ));
            };
            parent.exports.entry(name).or_default().push(ReachBinding {
                kind,
                meta: crate::reach::ExportMeta {
                    prefix: prefix.clone(),
                    friends: friends.clone(),
                },
            });
        }
    }
    Ok(())
}

fn reach_grant_prefix(
    visibility: &crate::ast::Visibility,
    module_name: Option<&str>,
) -> Result<Vec<String>, Error> {
    match &visibility.prefix {
        None => Ok(Vec::new()),
        Some(crate::ast::PrefixClause::Prefixed(Some(prefix))) => Ok(prefix.clone()),
        Some(crate::ast::PrefixClause::Prefixed(None)) => module_name
            .map(|name| vec![name.to_owned()])
            .ok_or_else(|| {
                Error::new(
                    visibility.span,
                    "GRANT ... PREFIXED without a prefix requires a named module",
                )
            }),
        Some(crate::ast::PrefixClause::Rename { old, new }) => {
            if old.is_empty() {
                Ok(new.clone())
            } else {
                Err(Error::new(
                    visibility.span,
                    "non-empty old prefixes in a GRANT are not supported here",
                ))
            }
        }
    }
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

fn assign_array_element(
    value: &mut Value,
    index: i128,
    new_value: Value,
    span: Span,
) -> Result<(), Error> {
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
    values[offset as usize] = new_value;
    Ok(())
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
        Type::Array(array) => Value::Array {
            low: array.low,
            values: vec![Value::Undefined; (array.high - array.low + 1).max(0) as usize],
        },
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
