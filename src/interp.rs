use crate::ast::*;
use crate::lexer::Span;
use crate::manifest::WidthProfile;
use crate::types::{self, IntKind, Type};
use std::cmp::Ordering;
use std::collections::HashMap;

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

pub fn run(program: &Program, profile: WidthProfile) -> Result<(), Error> {
    let mut interpreter = Interpreter {
        profile,
        scopes: Vec::new(),
        depth: 0,
    };
    for module in &program.modules {
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

#[derive(Default)]
struct Scope<'a> {
    vars: HashMap<String, Var>,
    procs: HashMap<String, &'a ProcDef>,
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
    scopes: Vec<Scope<'a>>,
    depth: usize,
}

impl<'a> Interpreter<'a> {
    fn run_module(&mut self, module: &'a Module) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = self.run_module_body(module);
        self.scopes.pop();
        result
    }

    fn run_module_body(&mut self, module: &'a Module) -> Result<(), Error> {
        self.collect_declarations(&module.declarations)?;
        let mut frame = Frame {
            result_type: None,
            result: None,
        };
        self.exec_actions(&module.actions, &mut frame)?;
        Ok(())
    }

    fn collect_declarations(&mut self, declarations: &'a [Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let ty = self.mode_type(&var.mode)?;
                        let scope = self.scopes.last_mut().expect("scope stack is never empty");
                        if scope.vars.contains_key(&var.name) || scope.procs.contains_key(&var.name)
                        {
                            return Err(Error::new(
                                var.span,
                                format!("duplicate declaration of `{}`", var.name),
                            ));
                        }
                        scope.vars.insert(
                            var.name.clone(),
                            Var {
                                ty,
                                value: Value::Undefined,
                            },
                        );
                    }
                }
                Declaration::Proc(proc_def) => {
                    let scope = self.scopes.last_mut().expect("scope stack is never empty");
                    if scope.vars.contains_key(&proc_def.name)
                        || scope.procs.contains_key(&proc_def.name)
                    {
                        return Err(Error::new(
                            proc_def.span,
                            format!("duplicate declaration of `{}`", proc_def.name),
                        ));
                    }
                    scope.procs.insert(proc_def.name.clone(), proc_def);
                }
            }
        }

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

    fn mode_type(&self, mode: &ModeRef) -> Result<Type, Error> {
        types::resolve_mode(&mode.name)
            .ok_or_else(|| Error::new(mode.span, format!("unknown mode `{}`", mode.name)))
    }

    fn exec_actions(&mut self, actions: &'a [Action], frame: &mut Frame) -> Result<Flow, Error> {
        for action in actions {
            match action {
                Action::Assign {
                    target,
                    op,
                    value,
                    span,
                } => {
                    let Expr::Name {
                        name,
                        span: target_span,
                    } = target
                    else {
                        return Err(Error::new(*span, "assignment target must be a name in v0"));
                    };

                    let value = if *op == AssignOp::Assign {
                        self.eval_expr(value)?
                    } else {
                        let current = self.read_var(name, *target_span)?;
                        let right = self.eval_expr(value)?;
                        self.apply_binary(binary_for_assign(*op), current, right, *span)?
                    };

                    self.assign_var(name, value, *span)?;
                }
                Action::Assert { condition, span } => {
                    let value = self.eval_expr(condition)?;
                    if !self.as_bool(value, *span)? {
                        return Err(Error::new(*span, "ASSERTFAIL"));
                    }
                }
                Action::Call { callee, .. } => {
                    self.eval_expr(callee)?;
                }
                Action::Return { value, span } => {
                    if let Some(value) = value {
                        let result_type = frame.result_type.ok_or_else(|| {
                            Error::new(
                                *span,
                                "RETURN with a value in a procedure that has no result",
                            )
                        })?;
                        let value = self.eval_expr(value)?;
                        frame.result = Some(self.convert(value, result_type, *span)?);
                    }
                    return Ok(Flow::Return);
                }
                Action::Result { value, span } => {
                    let result_type = frame
                        .result_type
                        .ok_or_else(|| Error::new(*span, "RESULT outside a value procedure"))?;
                    let value = self.eval_expr(value)?;
                    frame.result = Some(self.convert(value, result_type, *span)?);
                }
                Action::If {
                    branches,
                    else_actions,
                    ..
                } => {
                    for branch in branches {
                        let condition = self.eval_expr(&branch.condition)?;
                        if self.as_bool(condition, branch.condition.span())? {
                            return self.exec_actions(&branch.body, frame);
                        }
                    }
                    if let Some(actions) = else_actions {
                        return self.exec_actions(actions, frame);
                    }
                }
                Action::Do {
                    control,
                    body,
                    span,
                } => return self.exec_do(control, body, frame, *span),
                Action::Exit { label, .. } => return Ok(Flow::Exit(label.clone())),
                Action::Begin {
                    declarations,
                    actions,
                    ..
                } => {
                    self.scopes.push(Scope::default());
                    let result = (|| {
                        self.collect_declarations(declarations)?;
                        self.exec_actions(actions, frame)
                    })();
                    self.scopes.pop();
                    return result;
                }
                Action::Empty { .. } => {}
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_do(
        &mut self,
        control: &'a DoControl,
        body: &'a [Action],
        frame: &mut Frame,
        span: Span,
    ) -> Result<Flow, Error> {
        match control {
            DoControl::None => match self.exec_actions(body, frame)? {
                Flow::Exit(None) => Ok(Flow::Normal),
                flow => Ok(flow),
            },
            DoControl::While(condition) => {
                loop {
                    let value = self.eval_expr(condition)?;
                    if !self.as_bool(value, condition.span())? {
                        break;
                    }
                    match self.exec_actions(body, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            DoControl::ForEver => {
                loop {
                    match self.exec_actions(body, frame)? {
                        Flow::Normal => {}
                        Flow::Exit(None) => break,
                        flow => return Ok(flow),
                    }
                }
                Ok(Flow::Normal)
            }
            DoControl::For { .. } => Err(Error::new(
                span,
                "DO FOR is not implemented yet; use DO WHILE",
            )),
        }
    }

    fn eval_expr(&mut self, expr: &'a Expr) -> Result<Value, Error> {
        match expr {
            Expr::Integer { value, span, .. } => self.make_int(*value, None, *span),
            Expr::Bool { value, .. } => Ok(Value::Bool(*value)),
            Expr::Char { value, .. } => Ok(Value::Char(*value)),
            Expr::String { value, .. } => Ok(Value::String(value.clone())),
            Expr::Name { name, span } => self.read_var(name, *span),
            Expr::Apply { name, args, span } => self.eval_apply(name, args, *span),
            Expr::Unary { op, operand, span } => {
                let value = self.eval_expr(operand)?;
                match op {
                    UnaryOp::Neg => {
                        let (value, kind) = self.expect_int(value, *span)?;
                        let negated = value
                            .checked_neg()
                            .ok_or_else(|| Error::new(*span, "OVERFLOW in unary `-`"))?;
                        self.make_int(negated, kind, *span)
                    }
                    UnaryOp::Not => Ok(Value::Bool(!self.as_bool(value, *span)?)),
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => match op {
                BinaryOp::AndIf => {
                    let left_value = self.eval_expr(left)?;
                    if !self.as_bool(left_value, *span)? {
                        Ok(Value::Bool(false))
                    } else {
                        let right_value = self.eval_expr(right)?;
                        Ok(Value::Bool(self.as_bool(right_value, *span)?))
                    }
                }
                BinaryOp::OrIf => {
                    let left_value = self.eval_expr(left)?;
                    if self.as_bool(left_value, *span)? {
                        Ok(Value::Bool(true))
                    } else {
                        let right_value = self.eval_expr(right)?;
                        Ok(Value::Bool(self.as_bool(right_value, *span)?))
                    }
                }
                _ => {
                    let left_value = self.eval_expr(left)?;
                    let right_value = self.eval_expr(right)?;
                    self.apply_binary(*op, left_value, right_value, *span)
                }
            },
        }
    }

    fn eval_apply(&mut self, name: &str, args: &'a [Expr], span: Span) -> Result<Value, Error> {
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
            let value = self.eval_expr(&args[0])?;
            let (value, _) = self.expect_int(value, args[0].span())?;
            return self.make_int(value, Some(kind), span);
        }

        let Some(proc_def) = self.lookup_proc(name) else {
            return Err(Error::new(span, format!("unknown name `{name}`")));
        };
        if proc_def.params.len() != args.len() {
            return Err(Error::new(
                span,
                format!(
                    "procedure `{name}` expects {} argument(s), found {}",
                    proc_def.params.len(),
                    args.len()
                ),
            ));
        }

        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_expr(arg)?);
        }

        self.scopes.push(Scope::default());
        let result = self.call_proc(proc_def, values, span);
        self.scopes.pop();
        result
    }

    fn call_proc(
        &mut self,
        proc_def: &'a ProcDef,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        if self.depth >= 1024 {
            return Err(Error::new(span, "call depth limit exceeded"));
        }
        self.depth += 1;
        let result = self.call_proc_inner(proc_def, args, span);
        self.depth -= 1;
        result
    }

    fn call_proc_inner(
        &mut self,
        proc_def: &'a ProcDef,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        for (param, arg) in proc_def.params.iter().zip(args) {
            let ty = self.mode_type(&param.mode)?;
            let value = self.convert(arg, ty, param.span)?;
            let scope = self.scopes.last_mut().expect("scope stack is never empty");
            if scope.vars.contains_key(&param.name) || scope.procs.contains_key(&param.name) {
                return Err(Error::new(
                    param.span,
                    format!("duplicate declaration of `{}`", param.name),
                ));
            }
            scope.vars.insert(param.name.clone(), Var { ty, value });
        }

        self.collect_declarations(&proc_def.declarations)?;

        let result_type = match &proc_def.result {
            Some(mode) => Some(self.mode_type(mode)?),
            None => None,
        };

        let mut frame = Frame {
            result_type,
            result: None,
        };

        match self.exec_actions(&proc_def.actions, &mut frame)? {
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
                let (left, left_kind) = self.expect_int(left, span)?;
                let (right, right_kind) = self.expect_int(right, span)?;
                let kind = combine_kinds(left_kind, right_kind, span)?;

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

    fn read_var(&self, name: &str, span: Span) -> Result<Value, Error> {
        for scope in self.scopes.iter().rev() {
            if let Some(var) = scope.vars.get(name) {
                return match &var.value {
                    Value::Undefined => {
                        Err(Error::new(span, format!("use of undefined value `{name}`")))
                    }
                    value => Ok(value.clone()),
                };
            }
        }
        Err(Error::new(span, format!("unknown variable `{name}`")))
    }

    fn assign_var(&mut self, name: &str, value: Value, span: Span) -> Result<(), Error> {
        let ty = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.vars.get(name).map(|var| var.ty))
            .ok_or_else(|| Error::new(span, format!("unknown variable `{name}`")))?;

        let value = self.convert(value, ty, span)?;

        for scope in self.scopes.iter_mut().rev() {
            if let Some(var) = scope.vars.get_mut(name) {
                var.value = value;
                return Ok(());
            }
        }

        Err(Error::new(span, format!("unknown variable `{name}`")))
    }

    fn lookup_proc(&self, name: &str) -> Option<&'a ProcDef> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.procs.get(name).copied())
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
}
