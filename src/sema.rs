use crate::ast::*;
use crate::lexer::Span;
use crate::manifest::WidthProfile;
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
    let exports = build_exports(program)?;
    let mut checker = Checker {
        profile,
        scopes: Vec::new(),
        results: Vec::new(),
        exports,
        current_module: 0,
    };
    for (index, module) in program.modules.iter().enumerate() {
        checker.current_module = index;
        checker.check_module(module)?;
    }
    Ok(())
}

#[derive(Clone)]
struct ProcSig {
    params: Vec<Type>,
    result: Option<Type>,
}

#[derive(Clone)]
enum ExportKind {
    Var(Type),
    Proc(ProcSig),
}

#[derive(Clone)]
struct Export {
    module: usize,
    kind: ExportKind,
}

/// Collect every `GRANT`ed name across the program. Grants are static, so this
/// pass runs before modules are checked and is independent of textual order.
fn build_exports(program: &Program) -> Result<HashMap<String, Export>, Error> {
    let mut exports: HashMap<String, Export> = HashMap::new();

    for (index, module) in program.modules.iter().enumerate() {
        let mut vars = HashMap::new();
        let mut procs = HashMap::new();
        let mut grants = Vec::new();

        for declaration in &module.declarations {
            match declaration {
                Declaration::Var(list) => {
                    for var in list {
                        let ty = resolve_mode(&var.mode)?;
                        if vars.insert(var.name.clone(), ty).is_some()
                            || procs.contains_key(&var.name)
                        {
                            return Err(Error::new(
                                var.span,
                                format!("duplicate declaration of `{}`", var.name),
                            ));
                        }
                    }
                }
                Declaration::Proc(proc_def) => {
                    let signature = proc_signature(proc_def)?;
                    if procs.insert(proc_def.name.clone(), signature).is_some()
                        || vars.contains_key(&proc_def.name)
                    {
                        return Err(Error::new(
                            proc_def.span,
                            format!("duplicate declaration of `{}`", proc_def.name),
                        ));
                    }
                }
                Declaration::Visibility(visibility) if visibility.kind == VisibilityKind::Grant => {
                    for name in &visibility.names {
                        grants.push((name.name.clone(), name.span));
                    }
                }
                _ => {}
            }
        }

        for (name, span) in grants {
            let kind = if let Some(ty) = vars.get(&name) {
                ExportKind::Var(*ty)
            } else if let Some(signature) = procs.get(&name) {
                ExportKind::Proc(signature.clone())
            } else {
                return Err(Error::new(
                    span,
                    format!("cannot GRANT `{name}`: not declared in this module"),
                ));
            };

            if let Some(previous) = exports.get(&name) {
                let message = if previous.module == index {
                    format!("`{name}` is granted more than once")
                } else {
                    format!("`{name}` is granted by more than one module")
                };
                return Err(Error::new(span, message));
            }

            exports.insert(
                name,
                Export {
                    module: index,
                    kind,
                },
            );
        }
    }

    Ok(exports)
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Type>,
    procs: HashMap<String, ProcSig>,
}

struct Checker {
    profile: WidthProfile,
    scopes: Vec<Scope>,
    results: Vec<Option<Type>>,
    exports: HashMap<String, Export>,
    current_module: usize,
}

impl Checker {
    fn check_module(&mut self, module: &Module) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = self.check_module_body(module);
        self.scopes.pop();
        result
    }

    fn check_module_body(&mut self, module: &Module) -> Result<(), Error> {
        self.collect_declarations(&module.declarations)?;
        self.apply_seizes(&module.declarations)?;
        self.check_initializers(&module.declarations)?;
        self.check_proc_bodies(&module.declarations)?;
        self.check_actions(&module.actions)
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
                    let signature = proc_signature(proc_def)?;
                    self.declare_proc(&proc_def.name, signature, proc_def.span)?;
                }
                Declaration::Visibility(_) => {}
            }
        }
        Ok(())
    }

    fn check_initializers(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                let ty = resolve_mode(&var.mode)?;
                let Some(init) = &var.init else {
                    continue;
                };
                let value_ty = self.check_expr(&init.value)?;
                self.ensure_assignable(ty, value_ty, init.value.span())?;
                self.check_constant_range(&init.value, ty)?;
            }
        }
        Ok(())
    }

    fn check_proc_bodies(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            if let Declaration::Proc(proc_def) = declaration {
                self.check_proc(proc_def)?;
            }
        }
        Ok(())
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

    fn declare_proc(&mut self, name: &str, signature: ProcSig, span: Span) -> Result<(), Error> {
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

    fn lookup_proc(&self, name: &str) -> Option<ProcSig> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.procs.get(name).cloned())
    }

    fn check_actions(&mut self, actions: &[Action]) -> Result<(), Error> {
        for action in actions {
            self.check_action(action)?;
        }
        Ok(())
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
                    DoControl::For { .. } => {
                        return Err(Error::new(
                            *span,
                            "DO FOR is not implemented yet; use DO WHILE",
                        ));
                    }
                }
                self.check_actions(body)
            }
            Action::Exit { label, span } => {
                if label.is_some() {
                    return Err(Error::new(*span, "EXIT labels are not implemented yet"));
                }
                Ok(())
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
            Expr::Name { name, span } => self
                .lookup_var(name)
                .ok_or_else(|| Error::new(*span, format!("unknown variable `{name}`"))),
            _ => Err(Error::new(
                target.span(),
                "assignment target must be a name in v0",
            )),
        }
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

fn proc_signature(proc_def: &ProcDef) -> Result<ProcSig, Error> {
    let mut params = Vec::with_capacity(proc_def.params.len());
    let mut seen = HashSet::new();
    for param in &proc_def.params {
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
        params.push(resolve_mode(&param.mode)?);
    }

    let result = match &proc_def.result {
        Some(mode) => Some(resolve_mode(mode)?),
        None => None,
    };

    Ok(ProcSig { params, result })
}

fn resolve_mode(mode: &ModeRef) -> Result<Type, Error> {
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
}
