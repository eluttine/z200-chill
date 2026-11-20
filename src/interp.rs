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
    external: &[program::ExportInfo],
) -> Result<(), Error> {
    let info = program::analyze(program)
        .map_err(|error| Error::coded(error.code, error.span, error.message))?;
    let mut exports = info.exports;
    for info in external {
        let kind = match &info.kind {
            program::ExportInfoKind::Var(ty) => ExportKind::Var(*ty),
            program::ExportInfoKind::Proc { params, result } => ExportKind::Proc(types::ProcType {
                params: params.clone(),
                result: *result,
            }),
        };
        exports.push(program::ExportEntry {
            module: usize::MAX,
            package: usize::MAX,
            package_name: info.package_name.clone(),
            module_name: info.module_name.clone(),
            name: info.name.clone(),
            kind,
            friends: None,
        });
    }
    let lowered = ir::lower(program, check)
        .map_err(|error| Error::coded("E0400", error.span, error.message))?;
    let mut interpreter = Interpreter {
        profile,
        modules: Vec::new(),
        locals: Vec::new(),
        call_bases: Vec::new(),
        active_module: 0,
        depth: 0,
        exports,
        exprs: lowered.exprs,
        bodies: lowered.bodies,
    };

    interpreter.declare_modules(program)?;
    interpreter.apply_seizes(program)?;

    for (index, module) in program.modules.iter().enumerate() {
        if module.kind == ModuleKind::Spec {
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
}

struct Var {
    ty: Type,
    value: Value,
}

type VarCell = Rc<RefCell<Var>>;

#[derive(Clone, Copy)]
struct ProcRef<'a> {
    def: &'a ProcDef,
    module: usize,
}

#[derive(Default)]
struct Scope<'a> {
    vars: HashMap<String, VarCell>,
    procs: HashMap<String, ProcRef<'a>>,
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
    depth: usize,
    exports: Vec<program::ExportEntry>,
    exprs: ir::ExprTable,
    bodies: ir::BodyTable,
}

impl<'a> Interpreter<'a> {
    fn declare_modules(&mut self, program: &'a Program) -> Result<(), Error> {
        for (index, module) in program.modules.iter().enumerate() {
            let mut scope = Scope::default();
            if module.kind == ModuleKind::Body {
                declare_into_scope(&mut scope, &module.declarations, index)?;
            }
            self.modules.push(scope);
        }
        Ok(())
    }

    fn apply_seizes(&mut self, program: &'a Program) -> Result<(), Error> {
        let mut imports = Vec::new();

        for (current, module) in program.modules.iter().enumerate() {
            if module.kind == ModuleKind::Spec {
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
                    let mut accessible: Vec<&program::ExportEntry> = self
                        .exports
                        .iter()
                        .filter(|export| export.module != current)
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
                            matches!(&export.kind, ExportKind::Proc(_)),
                        ));
                    }
                    continue;
                }

                for name in &visibility.names {
                    let export =
                        program::resolve_export(&self.exports, name, package_name, module_name)
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
                        name.name.clone(),
                        name.span,
                        export.module,
                        matches!(&export.kind, ExportKind::Proc(_)),
                    ));
                }
            }
        }

        for (current, name, span, source, is_proc) in imports {
            if is_proc {
                let proc_ref = *self.modules[source].procs.get(&name).ok_or_else(|| {
                    Error::new(span, format!("cannot SEIZE `{name}`: not a procedure"))
                })?;
                if self.modules[current]
                    .procs
                    .insert(name.clone(), proc_ref)
                    .is_some()
                {
                    return Err(Error::new(
                        span,
                        format!("duplicate declaration of `{name}`"),
                    ));
                }
            } else {
                let cell = self.modules[source]
                    .vars
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| {
                        Error::new(span, format!("cannot SEIZE `{name}`: not a location"))
                    })?;
                if self.modules[current]
                    .vars
                    .insert(name.clone(), cell)
                    .is_some()
                {
                    return Err(Error::new(
                        span,
                        format!("duplicate declaration of `{name}`"),
                    ));
                }
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
        let scope = self.locals.last_mut().expect("local scope");
        declare_into_scope(scope, declarations, module)
    }

    fn declare_local_vars(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let scope = self.locals.last_mut().expect("local scope");
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let ty = mode_type(&var.mode)?;
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
                                value: Value::Undefined,
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
                Declaration::ProcSignature(_) | Declaration::Visibility(_) => {}
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
        match self.exec_stmts(&body, &mut frame)? {
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

    fn exec_stmts(&mut self, stmts: &[ir::Stmt], frame: &mut Frame) -> Result<Flow, Error> {
        for stmt in stmts {
            match stmt {
                ir::Stmt::Assign {
                    name,
                    op,
                    value,
                    span,
                } => {
                    let value = if *op == AssignOp::Assign {
                        self.eval_ir(value)?
                    } else {
                        let current = self.read_var(name, *span)?;
                        let right = self.eval_ir(value)?;
                        let target_ty = self.var_type(name, *span)?;
                        self.apply_binary(
                            binary_for_assign(*op),
                            current,
                            right,
                            Some(target_ty),
                            *span,
                        )?
                    };
                    self.assign_var(name, value, *span)?;
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
                            return self.exec_stmts(&branch.body, frame);
                        }
                    }
                    if let Some(actions) = else_actions {
                        return self.exec_stmts(actions, frame);
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
                            return self.exec_stmts(&alternative.body, frame);
                        }
                    }

                    if let Some(actions) = else_actions {
                        return self.exec_stmts(actions, frame);
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
                        self.exec_stmts(actions, frame)
                    })();
                    self.locals.pop();
                    return match result {
                        Ok(Flow::Exit(None)) => Ok(Flow::Normal),
                        other => other,
                    };
                }
                ir::Stmt::Empty { .. } => {}
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_do(
        &mut self,
        control: &ir::DoControlIr,
        body: &[ir::Stmt],
        frame: &mut Frame,
        span: Span,
    ) -> Result<Flow, Error> {
        match control {
            ir::DoControlIr::None => match self.exec_stmts(body, frame)? {
                Flow::Exit(None) => Ok(Flow::Normal),
                flow => Ok(flow),
            },
            ir::DoControlIr::While(condition) => {
                loop {
                    let value = self.eval_ir(condition)?;
                    if !self.as_bool(value, condition.span())? {
                        break;
                    }
                    match self.exec_stmts(body, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            ir::DoControlIr::ForEver => {
                loop {
                    match self.exec_stmts(body, frame)? {
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
        self.active_module = proc_ref.module;
        self.locals.push(Scope::default());
        self.call_bases.push(self.locals.len() - 1);
        self.depth += 1;

        let result = self.call_proc_inner(proc_ref.def, args, span);

        self.depth -= 1;
        self.call_bases.pop();
        self.locals.pop();
        self.active_module = previous_module;
        result
    }

    fn call_proc_inner(
        &mut self,
        proc_def: &'a ProcDef,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        for (param, arg) in proc_def.params.iter().zip(args) {
            let ty = mode_type(&param.mode)?;
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
            Some(mode) => Some(mode_type(mode)?),
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

        match self.exec_stmts(&body, &mut frame)? {
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
        match (left, right) {
            (Value::Int { value: left, .. }, Value::Int { value: right, .. }) => Ok(left == right),
            (Value::Bool(left), Value::Bool(right)) => Ok(left == right),
            (Value::Char(left), Value::Char(right)) => Ok(left == right),
            (Value::String(left), Value::String(right)) => Ok(left == right),
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
            Type::IntLiteral => Err(Error::new(span, "internal: cannot declare a literal type")),
        }
    }

    fn var_type(&self, name: &str, span: Span) -> Result<Type, Error> {
        let cell = self
            .find_var_cell(name)
            .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;
        let ty = cell.borrow().ty;
        Ok(ty)
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
        }
        self.modules
            .get(self.active_module)?
            .vars
            .get(name)
            .cloned()
    }

    fn lookup_proc(&self, name: &str) -> Option<ProcRef<'a>> {
        let floor = self.local_floor();
        for scope in self.locals[floor..].iter().rev() {
            if let Some(proc_ref) = scope.procs.get(name) {
                return Some(*proc_ref);
            }
        }
        self.modules
            .get(self.active_module)?
            .procs
            .get(name)
            .copied()
    }

    /// Scopes below this index belong to the caller and must not be visible
    /// to a module-level procedure body.
    fn local_floor(&self) -> usize {
        self.call_bases.last().copied().unwrap_or(0)
    }
}

fn declare_into_scope<'a>(
    scope: &mut Scope<'a>,
    declarations: &'a [Declaration],
    module: usize,
) -> Result<(), Error> {
    for declaration in declarations {
        match declaration {
            Declaration::Var(vars) => {
                for var in vars {
                    let ty = mode_type(&var.mode)?;
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
                            value: Value::Undefined,
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
                    },
                );
            }
            Declaration::ProcSignature(_) => {}
            Declaration::Visibility(_) => {}
        }
    }
    Ok(())
}

fn mode_type(mode: &ModeRef) -> Result<Type, Error> {
    types::resolve_mode(&mode.name)
        .ok_or_else(|| Error::new(mode.span, format!("unknown mode `{}`", mode.name)))
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
}
