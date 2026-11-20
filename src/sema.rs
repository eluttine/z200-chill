use crate::ast::*;
use crate::diagnostics::Diagnostic;
use crate::lexer::{SourceId, Span};
use crate::manifest::WidthProfile;
use crate::program::{self, Export, ExportKind};
use crate::types::ProcType;
use crate::types::{self, IntKind, Type};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub fn check(program: &Program, profile: WidthProfile) -> Result<(), Error> {
    check_with_externals(program, profile, &[])
}

pub fn check_with_externals(
    program: &Program,
    profile: WidthProfile,
    external: &[program::ExportInfo],
) -> Result<(), Error> {
    let diagnostics = check_all_with_externals(program, profile, external);
    match diagnostics.into_iter().next() {
        Some(diagnostic) => {
            let span = diagnostic.span.unwrap_or_else(|| {
                program
                    .modules
                    .first()
                    .map(|module| module.span)
                    .unwrap_or(Span::new(SourceId(0), 0, 0))
            });
            Err(Error::new(span, diagnostic.message))
        }
        None => Ok(()),
    }
}

pub fn check_all(program: &Program, profile: WidthProfile) -> Vec<Diagnostic> {
    check_all_with_externals(program, profile, &[])
}

pub fn check_all_with_externals(
    program: &Program,
    profile: WidthProfile,
    external: &[program::ExportInfo],
) -> Vec<Diagnostic> {
    let info = match program::analyze(program) {
        Ok(info) => info,
        Err(error) => {
            return vec![Diagnostic::new("E0400", Some(error.span), error.message)];
        }
    };
    let mut exports = info.exports;
    let fallback_span = program
        .modules
        .first()
        .map(|module| module.span)
        .unwrap_or(Span::new(SourceId(0), 0, 0));

    for info in external {
        if exports.contains_key(&info.name) {
            return vec![Diagnostic::new(
                "E0400",
                Some(fallback_span),
                format!("name `{}` is already exported", info.name),
            )];
        }

        let kind = match &info.kind {
            program::ExportInfoKind::Var(ty) => ExportKind::Var(*ty),
            program::ExportInfoKind::Proc { params, result } => ExportKind::Proc(ProcType {
                params: params.clone(),
                result: *result,
            }),
        };
        exports.insert(
            info.name.clone(),
            Export {
                module: usize::MAX,
                kind,
            },
        );
    }

    let mut checker = Checker {
        profile,
        scopes: Vec::new(),
        results: Vec::new(),
        exports,
        current_module: 0,
        labels: Vec::new(),
        diagnostics: Vec::new(),
    };
    for (index, module) in program.modules.iter().enumerate() {
        checker.current_module = index;
        let result = if module.kind == ModuleKind::Spec {
            checker.check_spec_module(module)
        } else {
            checker.check_module(module)
        };
        if let Err(error) = result {
            checker.push_diagnostic(error);
        }
    }

    checker.diagnostics
}

fn map_program_error(error: program::Error) -> Error {
    Error::new(error.span, error.message)
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Type>,
    procs: HashMap<String, ProcType>,
    readonly: HashSet<String>,
}

struct Checker {
    profile: WidthProfile,
    scopes: Vec<Scope>,
    results: Vec<Option<Type>>,
    exports: HashMap<String, Export>,
    current_module: usize,
    labels: Vec<String>,
    diagnostics: Vec<Diagnostic>,
}

impl Checker {
    fn push_diagnostic(&mut self, error: Error) {
        if self.diagnostics.iter().any(|diagnostic| {
            diagnostic.span == Some(error.span) && diagnostic.message == error.message
        }) {
            return;
        }
        self.diagnostics
            .push(Diagnostic::new("E0400", Some(error.span), error.message));
    }

    fn check_module(&mut self, module: &Module) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = self.check_module_body(module);
        self.scopes.pop();
        result
    }

    fn check_spec_module(&mut self, module: &Module) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = (|| {
            self.collect_declarations(&module.declarations)?;
            self.apply_seizes(&module.declarations)?;

            for declaration in &module.declarations {
                let Declaration::Var(vars) = declaration else {
                    continue;
                };
                for var in vars {
                    if var.init.is_some() {
                        return Err(Error::new(
                            var.span,
                            "initialization is not allowed in a spec module",
                        ));
                    }
                }
            }

            Ok(())
        })();
        self.scopes.pop();
        result
    }

    fn check_module_body(&mut self, module: &Module) -> Result<(), Error> {
        self.collect_declarations(&module.declarations)?;
        self.apply_seizes(&module.declarations)?;
        self.check_initializers(&module.declarations)?;
        self.check_proc_bodies(&module.declarations)?;

        let has_module_label = module.name.is_some();
        if let Some(name) = &module.name {
            self.labels.push(name.clone());
        }
        let result = self.check_actions(&module.actions);
        if has_module_label {
            self.labels.pop();
        }
        result
    }

    fn apply_seizes(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }

            for name in &visibility.names {
                let Some(export) = self.exports.get(&name.name) else {
                    return Err(Error::new(
                        name.span,
                        format!("cannot SEIZE `{}`: no module grants it", name.name),
                    ));
                };

                if export.module == self.current_module {
                    return Err(Error::new(
                        name.span,
                        format!("cannot SEIZE `{}` from the same module", name.name),
                    ));
                }

                match &export.kind {
                    ExportKind::Var(ty) => {
                        self.declare_var(&name.name, *ty, name.span)?;
                    }
                    ExportKind::Proc(signature) => {
                        self.declare_proc(&name.name, signature.clone(), name.span)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn collect_declarations(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let ty = resolve_mode(&var.mode)?;
                        self.declare_var(&var.name, ty, var.span)?;
                    }
                }
                Declaration::Proc(proc_def) => {
                    let signature =
                        program::signature_from_params(&proc_def.params, &proc_def.result)
                            .map_err(map_program_error)?;
                    self.declare_proc(&proc_def.name, signature, proc_def.span)?;
                }
                Declaration::ProcSignature(signature) => {
                    let proc_sig =
                        program::signature_from_params(&signature.params, &signature.result)
                            .map_err(map_program_error)?;
                    self.declare_proc(&signature.name, proc_sig, signature.span)?;
                }
                Declaration::Visibility(_) => {}
            }
        }
        Ok(())
    }

    fn check_initializers(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let mut first_error = None;

        for declaration in declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                let result = (|| -> Result<(), Error> {
                    let ty = resolve_mode(&var.mode)?;
                    let Some(init) = &var.init else {
                        return Ok(());
                    };
                    let value_ty = self.check_expr(&init.value)?;
                    self.ensure_assignable(ty, value_ty, init.value.span())?;
                    self.check_constant_range(&init.value, ty)
                })();

                if let Err(error) = result {
                    self.push_diagnostic(error.clone());
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn check_proc_bodies(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let mut first_error = None;

        for declaration in declarations {
            if let Declaration::Proc(proc_def) = declaration {
                if let Err(error) = self.check_proc(proc_def) {
                    self.push_diagnostic(error.clone());
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn check_proc(&mut self, proc_def: &ProcDef) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = self.check_proc_body(proc_def);
        self.scopes.pop();
        result
    }

    fn check_proc_body(&mut self, proc_def: &ProcDef) -> Result<(), Error> {
        let mut seen = HashSet::new();
        for param in &proc_def.params {
            if !seen.insert(param.name.as_str()) {
                return Err(Error::new(
                    param.span,
                    format!("duplicate parameter `{}`", param.name),
                ));
            }
            let ty = resolve_mode(&param.mode)?;
            self.declare_var(&param.name, ty, param.span)?;
        }

        self.collect_declarations(&proc_def.declarations)?;
        reject_nested_procs(&proc_def.declarations)?;
        self.check_initializers(&proc_def.declarations)?;
        self.check_proc_bodies(&proc_def.declarations)?;

        let result_type = match &proc_def.result {
            Some(mode) => Some(resolve_mode(mode)?),
            None => None,
        };

        self.results.push(result_type);
        let result = self.check_actions(&proc_def.actions);
        self.results.pop();
        result
    }

    fn declare_var(&mut self, name: &str, ty: Type, span: Span) -> Result<(), Error> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name) || scope.procs.contains_key(name) {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.vars.insert(name.to_owned(), ty);
        Ok(())
    }

    fn declare_proc(&mut self, name: &str, signature: ProcType, span: Span) -> Result<(), Error> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name) || scope.procs.contains_key(name) {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.procs.insert(name.to_owned(), signature);
        Ok(())
    }

    fn lookup_var(&self, name: &str) -> Option<Type> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.vars.get(name).copied())
    }

    fn lookup_proc(&self, name: &str) -> Option<ProcType> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.procs.get(name).cloned())
    }

    fn check_actions(&mut self, actions: &[Action]) -> Result<(), Error> {
        let mut first_error = None;

        for action in actions {
            if let Err(error) = self.check_action(action) {
                self.push_diagnostic(error.clone());
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn check_action(&mut self, action: &Action) -> Result<(), Error> {
        match action {
            Action::Assign {
                target,
                op,
                value,
                span,
            } => {
                let target_ty = self.location_type(target)?;
                let value_ty = self.check_expr(value)?;

                if *op == AssignOp::Assign {
                    self.ensure_assignable(target_ty, value_ty, *span)?;
                } else {
                    let binary = binary_for_assign(*op);
                    let result_ty = self.check_binary_types(binary, target_ty, value_ty, *span)?;
                    self.ensure_assignable(target_ty, result_ty, *span)?;
                }

                self.check_constant_range(value, target_ty)
            }
            Action::Assert { condition, .. } => {
                let ty = self.check_expr(condition)?;
                if ty != Type::Bool {
                    return Err(Error::new(
                        condition.span(),
                        format!("ASSERT requires BOOL, found {}", ty.describe()),
                    ));
                }
                Ok(())
            }
            Action::Call { callee, span } => {
                let Expr::Apply { name, args, .. } = callee else {
                    return Err(Error::new(*span, "expected a procedure call"));
                };
                self.check_apply(name, args, *span, false)?;
                Ok(())
            }
            Action::Return { value, span } => self.check_return(value.as_ref(), *span),
            Action::Result { value, span } => self.check_result(value, *span),
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    let ty = self.check_expr(&branch.condition)?;
                    if ty != Type::Bool {
                        return Err(Error::new(
                            branch.condition.span(),
                            format!("IF condition must be BOOL, found {}", ty.describe()),
                        ));
                    }
                    self.check_actions(&branch.body)?;
                }
                if let Some(actions) = else_actions {
                    self.check_actions(actions)?;
                }
                Ok(())
            }
            Action::Do {
                control,
                body,
                span,
            } => {
                match control {
                    DoControl::None | DoControl::ForEver => {}
                    DoControl::While(condition) => {
                        let ty = self.check_expr(condition)?;
                        if ty != Type::Bool {
                            return Err(Error::new(
                                condition.span(),
                                format!("DO WHILE condition must be BOOL, found {}", ty.describe()),
                            ));
                        }
                    }
                    DoControl::For {
                        counter,
                        start,
                        end,
                        step,
                        ..
                    } => {
                        let start_ty = self.check_expr(start)?;
                        let end_ty = self.check_expr(end)?;
                        if !start_ty.is_int() || !end_ty.is_int() {
                            return Err(Error::new(*span, "DO FOR bounds must be integers"));
                        }
                        let mut counter_ty = self.combine_int(start_ty, end_ty, *span)?;

                        if let Some(step_expr) = step {
                            let step_ty = self.check_expr(step_expr)?;
                            if !step_ty.is_int() {
                                return Err(Error::new(
                                    step_expr.span(),
                                    "DO FOR step must be an integer",
                                ));
                            }
                            counter_ty = self.combine_int(counter_ty, step_ty, step_expr.span())?;
                            if let Some(value) = constant_int(step_expr) {
                                if value <= 0 {
                                    return Err(Error::new(
                                        step_expr.span(),
                                        "DO FOR step must be positive",
                                    ));
                                }
                            }
                        }

                        self.scopes.push(Scope::default());
                        let result = (|| {
                            self.declare_var(counter, counter_ty, *span)?;
                            self.scopes
                                .last_mut()
                                .expect("scope stack is never empty")
                                .readonly
                                .insert(counter.clone());
                            self.check_actions(body)
                        })();
                        self.scopes.pop();
                        return result;
                    }
                }
                self.check_actions(body)
            }
            Action::Exit {
                label: Some(name),
                span,
            } => {
                if !self.labels.iter().any(|label| label == name) {
                    return Err(Error::new(*span, format!("unknown EXIT label `{name}`")));
                }
                Ok(())
            }
            Action::Exit { label: None, .. } => Ok(()),
            Action::Case {
                selector,
                alternatives,
                else_actions,
                ..
            } => {
                let selector_ty = self.check_expr(selector)?;
                if !selector_ty.is_int() {
                    return Err(Error::new(
                        selector.span(),
                        "CASE selector must be an integer in v0",
                    ));
                }

                for alternative in alternatives {
                    for label in &alternative.labels {
                        match label {
                            CaseLabel::Value(value) => {
                                self.check_case_label_value(*value, selector_ty, alternative.span)?;
                            }
                            CaseLabel::Range { low, high } => {
                                self.check_case_label_value(*low, selector_ty, alternative.span)?;
                                self.check_case_label_value(*high, selector_ty, alternative.span)?;
                            }
                            CaseLabel::Else | CaseLabel::Any => {}
                        }
                    }
                    self.check_actions(&alternative.body)?;
                }

                if let Some(actions) = else_actions {
                    self.check_actions(actions)?;
                }
                Ok(())
            }
            Action::Labelled { label, action, .. } => {
                if self.labels.iter().any(|existing| existing == label) {
                    return Err(Error::new(
                        action.span(),
                        format!("duplicate label `{label}`"),
                    ));
                }
                self.labels.push(label.clone());
                let result = self.check_action(action);
                self.labels.pop();
                result
            }
            Action::Begin {
                declarations,
                actions,
                ..
            } => {
                self.scopes.push(Scope::default());
                let result = (|| {
                    self.collect_declarations(declarations)?;
                    reject_nested_procs(declarations)?;
                    self.check_initializers(declarations)?;
                    self.check_proc_bodies(declarations)?;
                    self.check_actions(actions)
                })();
                self.scopes.pop();
                result
            }
            Action::Empty { .. } => Ok(()),
        }
    }

    fn check_return(&mut self, value: Option<&Expr>, span: Span) -> Result<(), Error> {
        match self.results.last().copied() {
            None => Err(Error::new(span, "RETURN outside a procedure")),
            Some(None) => {
                if value.is_some() {
                    Err(Error::new(
                        span,
                        "RETURN with a value in a procedure that has no result",
                    ))
                } else {
                    Ok(())
                }
            }
            Some(Some(result_ty)) => {
                if let Some(value) = value {
                    let value_ty = self.check_expr(value)?;
                    self.ensure_assignable(result_ty, value_ty, value.span())?;
                }
                Ok(())
            }
        }
    }

    fn check_result(&mut self, value: &Expr, span: Span) -> Result<(), Error> {
        match self.results.last().copied() {
            Some(Some(result_ty)) => {
                let value_ty = self.check_expr(value)?;
                self.ensure_assignable(result_ty, value_ty, span)
            }
            _ => Err(Error::new(span, "RESULT outside a value procedure")),
        }
    }

    fn location_type(&self, target: &Expr) -> Result<Type, Error> {
        match target {
            Expr::Name { name, span } => {
                if self.is_readonly(name) {
                    return Err(Error::new(
                        *span,
                        format!("`{name}` is a loop counter and is not a location"),
                    ));
                }
                self.lookup_var(name)
                    .ok_or_else(|| Error::new(*span, format!("unknown variable `{name}`")))
            }
            _ => Err(Error::new(
                target.span(),
                "assignment target must be a name in v0",
            )),
        }
    }

    fn is_readonly(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if scope.vars.contains_key(name) {
                return scope.readonly.contains(name);
            }
        }
        false
    }

    fn check_expr(&mut self, expr: &Expr) -> Result<Type, Error> {
        match expr {
            Expr::Integer { value, span, .. } => {
                let long = IntKind::Long;
                let Some((low, high)) = types::int_bounds(long, self.profile) else {
                    return Err(Error::new(
                        *span,
                        "integer widths above 127 are not supported",
                    ));
                };
                if !(*value >= low && *value <= high) {
                    return Err(Error::new(
                        *span,
                        format!("integer literal `{value}` is outside the range of &INT"),
                    ));
                }
                Ok(Type::IntLiteral)
            }
            Expr::Bool { .. } => Ok(Type::Bool),
            Expr::Char { .. } => Ok(Type::Char),
            Expr::String { .. } => Ok(Type::String),
            Expr::Name { name, span } => {
                if let Some(ty) = self.lookup_var(name) {
                    Ok(ty)
                } else if self.lookup_proc(name).is_some() {
                    Err(Error::new(
                        *span,
                        format!("procedure `{name}` must be called with `()`"),
                    ))
                } else {
                    Err(Error::new(*span, format!("unknown name `{name}`")))
                }
            }
            Expr::Apply { name, args, span } => match self.check_apply(name, args, *span, true)? {
                Some(ty) => Ok(ty),
                None => Err(Error::new(
                    *span,
                    format!("procedure `{name}` does not return a value"),
                )),
            },
            Expr::Unary { op, operand, span } => {
                let ty = self.check_expr(operand)?;
                match op {
                    UnaryOp::Neg => {
                        if ty.is_int() {
                            Ok(ty)
                        } else {
                            Err(Error::new(
                                *span,
                                format!("unary `-` requires an integer, found {}", ty.describe()),
                            ))
                        }
                    }
                    UnaryOp::Not => {
                        if ty == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            Err(Error::new(
                                *span,
                                format!("NOT requires BOOL, found {}", ty.describe()),
                            ))
                        }
                    }
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let left_ty = self.check_expr(left)?;
                let right_ty = self.check_expr(right)?;
                self.check_binary_types(*op, left_ty, right_ty, *span)
            }
        }
    }

    fn check_binary_types(
        &self,
        op: BinaryOp,
        left: Type,
        right: Type,
        span: Span,
    ) -> Result<Type, Error> {
        match op {
            BinaryOp::And | BinaryOp::AndIf | BinaryOp::Or | BinaryOp::OrIf | BinaryOp::Xor => {
                self.expect_bool_pair(left, right, span)?;
                Ok(Type::Bool)
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)?;
                    Ok(Type::Bool)
                } else if left == right && matches!(left, Type::Bool | Type::Char | Type::String) {
                    Ok(Type::Bool)
                } else {
                    Err(Error::new(
                        span,
                        format!(
                            "cannot compare {} with {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)?;
                    Ok(Type::Bool)
                } else if (left == Type::Char && right == Type::Char)
                    || (left == Type::String && right == Type::String)
                {
                    Ok(Type::Bool)
                } else {
                    Err(Error::new(
                        span,
                        format!(
                            "cannot order {} against {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Rem
            | BinaryOp::Pow => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)
                } else {
                    Err(Error::new(
                        span,
                        format!(
                            "arithmetic requires integers, found {} and {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::Concat => {
                if left == Type::String && right == Type::String {
                    Ok(Type::String)
                } else {
                    Err(Error::new(
                        span,
                        format!(
                            "`//` requires character strings, found {} and {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
        }
    }

    fn expect_bool_pair(&self, left: Type, right: Type, span: Span) -> Result<(), Error> {
        if left == Type::Bool && right == Type::Bool {
            Ok(())
        } else {
            Err(Error::new(
                span,
                format!(
                    "boolean operator requires BOOL, found {} and {}",
                    left.describe(),
                    right.describe()
                ),
            ))
        }
    }

    fn combine_int(&self, left: Type, right: Type, span: Span) -> Result<Type, Error> {
        match (left, right) {
            (Type::IntLiteral, Type::IntLiteral) => Ok(Type::IntLiteral),
            (Type::Int(kind), Type::IntLiteral) | (Type::IntLiteral, Type::Int(kind)) => {
                Ok(Type::Int(kind))
            }
            (Type::Int(left_kind), Type::Int(right_kind)) if left_kind == right_kind => {
                Ok(Type::Int(left_kind))
            }
            (Type::Int(left_kind), Type::Int(right_kind)) => Err(Error::new(
                span,
                format!(
                    "integer mode mismatch: {} and {}; use an explicit conversion",
                    left_kind.name(),
                    right_kind.name()
                ),
            )),
            _ => Err(Error::new(
                span,
                format!(
                    "mixing {} with {} is not allowed",
                    left.describe(),
                    right.describe()
                ),
            )),
        }
    }

    fn check_apply(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        require_result: bool,
    ) -> Result<Option<Type>, Error> {
        if let Some(mode) = types::resolve_mode(name) {
            let Type::Int(kind) = mode else {
                return Err(Error::new(
                    span,
                    format!("conversion to {} is not supported yet", mode.describe()),
                ));
            };
            if args.len() != 1 {
                return Err(Error::new(
                    span,
                    format!("conversion to {} takes exactly one argument", kind.name()),
                ));
            }
            let arg_ty = self.check_expr(&args[0])?;
            if !arg_ty.is_int() {
                return Err(Error::new(
                    args[0].span(),
                    format!(
                        "conversion to {} requires an integer, found {}",
                        kind.name(),
                        arg_ty.describe()
                    ),
                ));
            }
            return Ok(Some(Type::Int(kind)));
        }

        let Some(signature) = self.lookup_proc(name) else {
            return Err(Error::new(span, format!("unknown name `{name}`")));
        };

        if signature.params.len() != args.len() {
            return Err(Error::new(
                span,
                format!(
                    "procedure `{name}` expects {} argument(s), found {}",
                    signature.params.len(),
                    args.len()
                ),
            ));
        }

        for (arg, param_ty) in args.iter().zip(&signature.params) {
            let arg_ty = self.check_expr(arg)?;
            self.ensure_assignable(*param_ty, arg_ty, arg.span())?;
        }

        if require_result && signature.result.is_none() {
            return Err(Error::new(
                span,
                format!("procedure `{name}` does not return a value"),
            ));
        }

        Ok(signature.result)
    }

    fn ensure_assignable(&self, target: Type, value: Type, span: Span) -> Result<(), Error> {
        let ok = match target {
            Type::Int(target_kind) => match value {
                Type::Int(value_kind) => value_kind == target_kind,
                Type::IntLiteral => true,
                _ => false,
            },
            Type::Bool => value == Type::Bool,
            Type::Char => value == Type::Char,
            Type::String => value == Type::String,
            Type::IntLiteral => false,
        };

        if ok {
            Ok(())
        } else {
            Err(Error::new(
                span,
                format!(
                    "cannot assign {} to {}",
                    value.describe(),
                    target.describe()
                ),
            ))
        }
    }

    fn check_case_label_value(
        &self,
        value: i128,
        selector_ty: Type,
        span: Span,
    ) -> Result<(), Error> {
        let fits = match selector_ty {
            Type::Int(kind) => types::contains(kind, value, self.profile),
            Type::IntLiteral => types::contains(IntKind::Long, value, self.profile),
            _ => false,
        };

        if fits {
            Ok(())
        } else {
            Err(Error::new(
                span,
                format!("CASE label `{value}` is outside the range of the selector"),
            ))
        }
    }

    fn check_constant_range(&self, expr: &Expr, target: Type) -> Result<(), Error> {
        let Type::Int(kind) = target else {
            return Ok(());
        };
        let Some(value) = constant_int(expr) else {
            return Ok(());
        };
        if !types::contains(kind, value, self.profile) {
            return Err(Error::new(
                expr.span(),
                format!("value `{value}` is outside the range of {}", kind.name()),
            ));
        }
        Ok(())
    }
}

fn resolve_mode(mode: &ModeRef) -> Result<Type, Error> {
    program::resolve_mode(mode).map_err(map_program_error)
}

fn reject_nested_procs(declarations: &[Declaration]) -> Result<(), Error> {
    for declaration in declarations {
        if let Declaration::Proc(proc_def) = declaration {
            return Err(Error::new(
                proc_def.span,
                "procedure definitions inside procedures are not implemented yet",
            ));
        }
    }
    Ok(())
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

fn constant_int(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Integer { value, .. } => Some(*value),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
            ..
        } => constant_int(operand).and_then(i128::checked_neg),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::manifest::WidthProfile;
    use crate::parser;

    fn check_source(source: &str) -> Result<(), Error> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        check(&program, WidthProfile::default())
    }

    fn check_all_source(source: &str) -> Vec<Diagnostic> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        check_all(&program, WidthProfile::default())
    }

    #[test]
    fn collects_multiple_action_errors() {
        let diagnostics = check_all_source(
            r#"
M: MODULE
  DCL a SHORT_INT;
  DCL b LONG_INT;
  a := b;
  a := b;
END M;
"#,
        );

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("cannot assign LONG_INT")));
    }

    #[test]
    fn hello_example_checks() {
        check_source(include_str!("../examples/hello/src/main.ch")).unwrap();
    }

    #[test]
    fn rejects_mode_mismatch() {
        let error = check_source(
            r#"
M: MODULE
  DCL a SHORT_INT;
  DCL b LONG_INT;
  a := b;
END M;
"#,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("cannot assign LONG_INT to SHORT_INT"));
    }

    #[test]
    fn rejects_assigning_bool_to_integer() {
        let error = check_source(
            r#"
M: MODULE
  DCL x INT;
  x := TRUE;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("cannot assign BOOL to INT"));
    }

    #[test]
    fn rejects_unknown_mode() {
        let error = check_source(
            r#"
M: MODULE
  DCL x NOPE;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("unknown mode `NOPE`"));
    }

    #[test]
    fn rejects_literal_out_of_range() {
        let error = check_source(
            r#"
M: MODULE
  DCL x SHORT_INT INIT := 100_000;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("outside the range of SHORT_INT"));
    }

    #[test]
    fn seizes_granted_variables_and_procedures() {
        check_source(
            r#"
A: MODULE
  GRANT x, F;
  DCL x INT INIT := 41;
  F: PROC () RETURNS (INT);
    RETURN (x + 1);
  END F;
END A;

B: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
  x := x + 1;
  ASSERT x = 42;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_seize_of_ungranted_name() {
        let error = check_source(
            r#"
A: MODULE
  DCL secret INT;
END A;

B: MODULE
  SEIZE secret;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("no module grants it"));
    }

    #[test]
    fn rejects_grant_of_undeclared_name() {
        let error = check_source(
            r#"
A: MODULE
  GRANT missing;
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("not declared in this module"));
    }

    #[test]
    fn rejects_duplicate_grants() {
        let error = check_source(
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
        .unwrap_err();
        assert!(error.message.contains("granted by more than one module"));
    }

    #[test]
    fn rejects_seize_from_same_module() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x;
  SEIZE x;
  DCL x INT;
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("from the same module"));
    }

    #[test]
    fn private_names_stay_private() {
        let error = check_source(
            r#"
A: MODULE
  DCL secret INT;
END A;

B: MODULE
  ASSERT secret = 1;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("unknown name `secret`"));
    }

    #[test]
    fn spec_and_body_interfaces_match() {
        check_source(
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
    fn spec_requires_a_body() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  GRANT F;
  F: PROC () END;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("has no body"));
    }

    #[test]
    fn spec_signature_must_match_body() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  F: PROC (a INT) RETURNS (INT) END;
END Lib;

Lib: MODULE
  F: PROC (a INT) RETURNS (BOOL);
    RETURN (TRUE);
  END F;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("different signature"));
    }

    #[test]
    fn body_with_spec_cannot_grant() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  F: PROC () END;
END Lib;

Lib: MODULE
  GRANT F;
  F: PROC ();
  END F;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("GRANT must be in the spec module"));
    }

    #[test]
    fn spec_module_rejects_initializers() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  DCL x INT INIT := 1;
  F: PROC () END;
END Lib;

Lib: MODULE
  DCL x INT;
  F: PROC ();
  END F;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("initialization is not allowed in a spec module"));
    }

    #[test]
    fn do_for_step_must_be_positive() {
        let error = check_source(
            r#"
M: MODULE
  DO FOR i := 1 BY 0 TO 3;
  OD;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("DO FOR step must be positive"));
    }

    #[test]
    fn rejects_unknown_exit_label() {
        let error = check_source(
            r#"
M: MODULE
  EXIT Nowhere;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("unknown EXIT label `Nowhere`"));
    }

    #[test]
    fn case_selector_must_be_integer() {
        let error = check_source(
            r#"
M: MODULE
  CASE TRUE OF
    (1): ;
  ESAC;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("CASE selector must be an integer"));
    }

    #[test]
    fn case_label_must_fit_selector() {
        let error = check_source(
            r#"
M: MODULE
  DCL x SHORT_INT;
  CASE x OF
    (100_000): ;
  ESAC;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("outside the range of the selector"));
    }

    #[test]
    fn loop_counter_is_not_assignable() {
        let error = check_source(
            r#"
M: MODULE
  DO FOR i := 1 TO 3;
    i := 99;
  OD;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("loop counter and is not a location"));
    }
}
