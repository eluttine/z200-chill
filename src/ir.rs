use crate::ast::{
    Action, CaseLabel, Declaration, DoControl, Expr as AstExpr, Module, Program, UnaryOp,
};
use crate::lexer::Span;
use crate::sema::{ApplyResolution, CheckResult};
use crate::types::{IntKind, Type};
use std::collections::HashMap;
use std::rc::Rc;

pub type Node = Rc<Expr>;

#[derive(Debug)]
pub enum Expr {
    Integer {
        value: i128,
        base: u32,
        ty: Type,
        span: Span,
    },
    Bool {
        value: bool,
        ty: Type,
        span: Span,
    },
    Char {
        value: char,
        ty: Type,
        span: Span,
    },
    String {
        value: String,
        ty: Type,
        span: Span,
    },
    Name {
        name: String,
        ty: Type,
        span: Span,
    },
    Conversion {
        kind: IntKind,
        arg: Node,
        ty: Type,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<Node>,
        ty: Option<Type>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Node,
        ty: Type,
        span: Span,
    },
    Binary {
        op: crate::ast::BinaryOp,
        left: Node,
        right: Node,
        ty: Type,
        span: Span,
    },
    Field {
        base: Node,
        field: String,
        ty: Type,
        span: Span,
    },
    Index {
        name: String,
        index: Node,
        ty: Type,
        span: Span,
    },
    Builtin {
        builtin: crate::builtins::Builtin,
        args: Vec<Node>,
        ty: Option<Type>,
        span: Span,
    },
    TextIo {
        kind: crate::builtins::TextIo,
        target: String,
        index: Option<Node>,
        format: Node,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Integer { span, .. }
            | Self::Bool { span, .. }
            | Self::Char { span, .. }
            | Self::String { span, .. }
            | Self::Name { span, .. }
            | Self::Conversion { span, .. }
            | Self::Call { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::Field { span, .. }
            | Self::Index { span, .. }
            | Self::Builtin { span, .. }
            | Self::TextIo { span, .. } => *span,
        }
    }

    pub fn ty(&self) -> Option<Type> {
        match self {
            Self::Integer { ty, .. }
            | Self::Bool { ty, .. }
            | Self::Char { ty, .. }
            | Self::String { ty, .. }
            | Self::Name { ty, .. }
            | Self::Conversion { ty, .. }
            | Self::Unary { ty, .. }
            | Self::Binary { ty, .. }
            | Self::Field { ty, .. }
            | Self::Index { ty, .. } => Some(*ty),
            Self::Call { ty, .. } | Self::Builtin { ty, .. } => *ty,
            Self::TextIo { .. } => None,
        }
    }
}

/// An assignment location: a variable, or a field path rooted at a variable.
#[derive(Debug)]
pub enum AssignTarget {
    Var(String),
    Field {
        root: String,
        path: Vec<String>,
        ty: Type,
    },
    Index {
        name: String,
        index: Node,
        ty: Type,
    },
}

#[derive(Debug)]
pub enum Stmt {
    Assign {
        target: AssignTarget,
        op: crate::ast::AssignOp,
        value: Node,
        span: Span,
    },
    Assert {
        condition: Node,
        span: Span,
    },
    Call {
        call: Node,
        span: Span,
    },
    Return {
        value: Option<Node>,
        span: Span,
    },
    Result {
        value: Node,
        span: Span,
    },
    If {
        branches: Vec<BranchIr>,
        else_actions: Option<Vec<Stmt>>,
        else_span: Option<Span>,
        span: Span,
    },
    Do {
        control: DoControlIr,
        body: Vec<Stmt>,
        span: Span,
    },
    Exit {
        label: Option<String>,
        span: Span,
    },
    Case {
        selector: Node,
        alternatives: Vec<CaseAlternativeIr>,
        else_actions: Option<Vec<Stmt>>,
        else_span: Option<Span>,
        span: Span,
    },
    Labelled {
        label: String,
        action: Box<Stmt>,
        span: Span,
    },
    Begin {
        declarations: Vec<Declaration>,
        actions: Vec<Stmt>,
        span: Span,
    },
    Empty {
        span: Span,
    },
    Cause {
        name: String,
        span: Span,
    },
    Module {
        label: Option<String>,
        declarations: Vec<Declaration>,
        actions: Vec<Stmt>,
        span: Span,
    },
}

#[derive(Debug)]
pub struct BranchIr {
    pub condition: Node,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub struct CaseAlternativeIr {
    pub labels: Vec<CaseLabel>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug)]
pub enum DoControlIr {
    None,
    While(Node),
    ForEver,
    For {
        counter: String,
        start: Node,
        end: Node,
        step: Option<Node>,
        down: bool,
    },
}

#[derive(Debug)]
pub struct HandlerAlternativeIr {
    pub exceptions: Vec<String>,
    pub actions: Vec<Stmt>,
}

#[derive(Debug)]
pub struct HandlerIr {
    pub alternatives: Vec<HandlerAlternativeIr>,
    pub else_actions: Option<Vec<Stmt>>,
}

#[derive(Debug, Default)]
pub struct BodyTable {
    bodies: HashMap<Span, Rc<Vec<Stmt>>>,
    handlers: HashMap<Span, Rc<HandlerIr>>,
}

impl BodyTable {
    pub fn get(&self, span: Span) -> Option<Rc<Vec<Stmt>>> {
        self.bodies.get(&span).cloned()
    }

    fn insert(&mut self, span: Span, body: Vec<Stmt>) {
        self.bodies.insert(span, Rc::new(body));
    }

    pub fn handler(&self, span: Span) -> Option<Rc<HandlerIr>> {
        self.handlers.get(&span).cloned()
    }

    fn insert_handler(&mut self, span: Span, handler: HandlerIr) {
        self.handlers.insert(span, Rc::new(handler));
    }
}

#[derive(Debug)]
pub struct Lowered {
    pub exprs: ExprTable,
    pub bodies: BodyTable,
}

#[derive(Debug, Default)]
pub struct ExprTable {
    exprs: HashMap<Span, Node>,
}

impl ExprTable {
    pub fn get(&self, span: Span) -> Option<Node> {
        self.exprs.get(&span).cloned()
    }

    fn insert(&mut self, expr: &Node) {
        self.exprs.insert(expr.span(), expr.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub span: Span,
    pub message: String,
}

impl Error {
    fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

pub fn lower(program: &Program, check: &CheckResult) -> Result<Lowered, Error> {
    let mut lowerer = Lowerer {
        check,
        exprs: ExprTable::default(),
        bodies: BodyTable::default(),
    };

    for module in &program.modules {
        lowerer.lower_module(module)?;
    }

    Ok(Lowered {
        exprs: lowerer.exprs,
        bodies: lowerer.bodies,
    })
}

struct Lowerer<'a> {
    check: &'a CheckResult,
    exprs: ExprTable,
    bodies: BodyTable,
}

impl Lowerer<'_> {
    fn lower_module(&mut self, module: &Module) -> Result<(), Error> {
        self.lower_declarations(&module.declarations)?;
        let actions = self.lower_actions(&module.actions)?;
        self.bodies.insert(module.span, actions);
        if let Some(handler) = &module.handler {
            let handler = self.lower_handler(handler)?;
            self.bodies.insert_handler(module.span, handler);
        }
        Ok(())
    }

    fn lower_declarations(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        if let Some(init) = &var.init {
                            self.lower_expr(&init.value)?;
                        }
                    }
                }
                Declaration::Proc(proc_def) => {
                    self.lower_declarations(&proc_def.declarations)?;
                    let actions = self.lower_actions(&proc_def.actions)?;
                    self.bodies.insert(proc_def.span, actions);
                    if let Some(handler) = &proc_def.handler {
                        let handler = self.lower_handler(handler)?;
                        self.bodies.insert_handler(proc_def.span, handler);
                    }
                }
                Declaration::ProcSignature(_)
                | Declaration::Visibility(_)
                | Declaration::Mode(_)
                | Declaration::SpecModule(_) => {}
            }
        }
        Ok(())
    }

    fn lower_actions(&mut self, actions: &[Action]) -> Result<Vec<Stmt>, Error> {
        let mut lowered = Vec::with_capacity(actions.len());
        for action in actions {
            lowered.push(self.lower_action(action)?);
        }
        Ok(lowered)
    }

    fn lower_action(&mut self, action: &Action) -> Result<Stmt, Error> {
        match action {
            Action::Assign {
                target,
                op,
                value,
                span,
            } => Ok(Stmt::Assign {
                target: self.lower_target(target)?,
                op: *op,
                value: self.lower_expr(value)?,
                span: *span,
            }),
            Action::Assert { condition, span } => Ok(Stmt::Assert {
                condition: self.lower_expr(condition)?,
                span: *span,
            }),
            Action::Call { callee, span } => Ok(Stmt::Call {
                call: self.lower_expr(callee)?,
                span: *span,
            }),
            Action::Return { value, span } => Ok(Stmt::Return {
                value: match value {
                    Some(value) => Some(self.lower_expr(value)?),
                    None => None,
                },
                span: *span,
            }),
            Action::Result { value, span } => Ok(Stmt::Result {
                value: self.lower_expr(value)?,
                span: *span,
            }),
            Action::If {
                branches,
                else_actions,
                else_span,
                span,
            } => {
                let mut lowered_branches = Vec::with_capacity(branches.len());
                for branch in branches {
                    lowered_branches.push(BranchIr {
                        condition: self.lower_expr(&branch.condition)?,
                        body: self.lower_actions(&branch.body)?,
                        span: branch.span,
                    });
                }
                let else_actions = match else_actions {
                    Some(actions) => Some(self.lower_actions(actions)?),
                    None => None,
                };
                Ok(Stmt::If {
                    branches: lowered_branches,
                    else_actions,
                    else_span: *else_span,
                    span: *span,
                })
            }
            Action::Do {
                control,
                body,
                span,
            } => {
                let control = match control {
                    DoControl::None => DoControlIr::None,
                    DoControl::ForEver => DoControlIr::ForEver,
                    DoControl::While(condition) => DoControlIr::While(self.lower_expr(condition)?),
                    DoControl::For {
                        counter,
                        start,
                        end,
                        step,
                        down,
                    } => DoControlIr::For {
                        counter: counter.clone(),
                        start: self.lower_expr(start)?,
                        end: self.lower_expr(end)?,
                        step: match step {
                            Some(step) => Some(self.lower_expr(step)?),
                            None => None,
                        },
                        down: *down,
                    },
                };
                Ok(Stmt::Do {
                    control,
                    body: self.lower_actions(body)?,
                    span: *span,
                })
            }
            Action::Exit { label, span } => Ok(Stmt::Exit {
                label: label.clone(),
                span: *span,
            }),
            Action::Case {
                selector,
                alternatives,
                else_actions,
                else_span,
                span,
            } => {
                let selector = self.lower_expr(selector)?;
                let mut lowered_alternatives = Vec::with_capacity(alternatives.len());
                for alternative in alternatives {
                    lowered_alternatives.push(CaseAlternativeIr {
                        labels: alternative.labels.clone(),
                        body: self.lower_actions(&alternative.body)?,
                        span: alternative.span,
                    });
                }
                let else_actions = match else_actions {
                    Some(actions) => Some(self.lower_actions(actions)?),
                    None => None,
                };
                Ok(Stmt::Case {
                    selector,
                    alternatives: lowered_alternatives,
                    else_actions,
                    else_span: *else_span,
                    span: *span,
                })
            }
            Action::Labelled {
                label,
                action,
                span,
            } => Ok(Stmt::Labelled {
                label: label.clone(),
                action: Box::new(self.lower_action(action)?),
                span: *span,
            }),
            Action::Begin {
                declarations,
                actions,
                span,
            } => Ok(Stmt::Begin {
                declarations: declarations.clone(),
                actions: self.lower_actions(actions)?,
                span: *span,
            }),
            Action::Empty { span } => Ok(Stmt::Empty { span: *span }),
            Action::Cause { name, span } => Ok(Stmt::Cause {
                name: name.clone(),
                span: *span,
            }),
            Action::Module(module) => {
                self.lower_declarations(&module.declarations)?;
                if let Some(handler) = &module.handler {
                    let handler = self.lower_handler(handler)?;
                    self.bodies.insert_handler(module.span, handler);
                }
                Ok(Stmt::Module {
                    label: module.name.clone(),
                    declarations: module.declarations.clone(),
                    actions: self.lower_actions(&module.actions)?,
                    span: module.span,
                })
            }
        }
    }

    fn lower_handler(&mut self, handler: &crate::ast::Handler) -> Result<HandlerIr, Error> {
        let mut alternatives = Vec::with_capacity(handler.alternatives.len());
        for alternative in &handler.alternatives {
            alternatives.push(HandlerAlternativeIr {
                exceptions: alternative.exceptions.clone(),
                actions: self.lower_actions(&alternative.actions)?,
            });
        }
        let else_actions = match &handler.else_actions {
            Some(actions) => Some(self.lower_actions(actions)?),
            None => None,
        };
        Ok(HandlerIr {
            alternatives,
            else_actions,
        })
    }

    fn lower_target(&mut self, target: &AstExpr) -> Result<AssignTarget, Error> {
        match target {
            AstExpr::Name { name, .. } => Ok(AssignTarget::Var(name.clone())),
            AstExpr::Apply { name, args, span } => {
                if !matches!(
                    self.check.resolutions.get(span),
                    Some(ApplyResolution::ArrayIndex)
                ) {
                    return Err(Error::new(
                        *span,
                        "assignment target must be a variable, field, or array element",
                    ));
                }
                let ty = self.check.types.get(span).copied().ok_or_else(|| {
                    Error::new(*span, "internal: array element has no checked mode")
                })?;
                let index = args
                    .first()
                    .ok_or_else(|| Error::new(*span, "array index has no operand"))?;
                Ok(AssignTarget::Index {
                    name: name.clone(),
                    index: self.lower_expr(index)?,
                    ty,
                })
            }
            AstExpr::Field { .. } => {
                let ty = self
                    .check
                    .types
                    .get(&target.span())
                    .copied()
                    .ok_or_else(|| {
                        Error::new(target.span(), "internal: field has no checked mode")
                    })?;
                let (root, path) = flatten_field(target).ok_or_else(|| {
                    Error::new(
                        target.span(),
                        "assignment target must be a variable or a field of a variable",
                    )
                })?;
                Ok(AssignTarget::Field { root, path, ty })
            }
            _ => Err(Error::new(
                target.span(),
                "assignment target must be a variable or a field of a variable",
            )),
        }
    }

    fn lower_expr(&mut self, expr: &AstExpr) -> Result<Node, Error> {
        let span = expr.span();

        if let AstExpr::Apply { name, args, .. } = expr {
            let args = args
                .iter()
                .map(|arg| self.lower_expr(arg))
                .collect::<Result<Vec<_>, _>>()?;
            let ty = self.check.types.get(&span).copied();

            let lowered = match self.check.resolutions.get(&span) {
                Some(ApplyResolution::Conversion(kind)) => {
                    let ty = ty.ok_or_else(|| {
                        Error::new(span, "internal: conversion has no checked mode")
                    })?;
                    Rc::new(Expr::Conversion {
                        kind: *kind,
                        arg: args
                            .into_iter()
                            .next()
                            .ok_or_else(|| Error::new(span, "conversion has no argument"))?,
                        ty,
                        span,
                    })
                }
                Some(ApplyResolution::Procedure) => Rc::new(Expr::Call {
                    name: name.clone(),
                    args,
                    ty,
                    span,
                }),
                Some(ApplyResolution::TextIo(kind)) => {
                    let (target, index, format) = match args.as_slice() {
                        [target, format] => (target, None, format),
                        [target, index, format] => (target, Some(index), format),
                        _ => {
                            return Err(Error::new(span, "internal: text I/O has the wrong arity"));
                        }
                    };
                    let Expr::Name {
                        name: target_name, ..
                    } = target.as_ref()
                    else {
                        return Err(Error::new(
                            target.span(),
                            "internal: text argument is not a location",
                        ));
                    };
                    Rc::new(Expr::TextIo {
                        kind: *kind,
                        target: target_name.clone(),
                        index: index.cloned(),
                        format: format.clone(),
                        span,
                    })
                }
                Some(ApplyResolution::Builtin(builtin)) => Rc::new(Expr::Builtin {
                    builtin: *builtin,
                    args,
                    ty,
                    span,
                }),
                Some(ApplyResolution::ArrayIndex) => {
                    let ty = ty.ok_or_else(|| {
                        Error::new(span, "internal: array index has no checked mode")
                    })?;
                    Rc::new(Expr::Index {
                        name: name.clone(),
                        index: args
                            .into_iter()
                            .next()
                            .ok_or_else(|| Error::new(span, "array index has no operand"))?,
                        ty,
                        span,
                    })
                }
                None => {
                    return Err(Error::new(span, "internal: call/apply has no resolution"));
                }
            };

            self.exprs.insert(&lowered);
            return Ok(lowered);
        }

        let ty = self
            .check
            .types
            .get(&span)
            .copied()
            .ok_or_else(|| Error::new(span, "internal: expression has no checked mode"))?;

        let lowered: Node = match expr {
            AstExpr::Integer { value, base, .. } => Rc::new(Expr::Integer {
                value: *value,
                base: *base,
                ty,
                span,
            }),
            AstExpr::Bool { value, .. } => Rc::new(Expr::Bool {
                value: *value,
                ty,
                span,
            }),
            AstExpr::Char { value, .. } => Rc::new(Expr::Char {
                value: *value,
                ty,
                span,
            }),
            AstExpr::String { value, .. } => Rc::new(Expr::String {
                value: value.clone(),
                ty,
                span,
            }),
            AstExpr::Name { name, .. } => Rc::new(Expr::Name {
                name: name.clone(),
                ty,
                span,
            }),
            AstExpr::Unary { op, operand, .. } => {
                let operand = self.lower_expr(operand)?;
                Rc::new(Expr::Unary {
                    op: *op,
                    operand,
                    ty,
                    span,
                })
            }
            AstExpr::Binary {
                op, left, right, ..
            } => {
                let left = self.lower_expr(left)?;
                let right = self.lower_expr(right)?;
                Rc::new(Expr::Binary {
                    op: *op,
                    left,
                    right,
                    ty,
                    span,
                })
            }
            AstExpr::Apply { .. } => unreachable!("handled above"),
            AstExpr::Field { base, field, .. } => Rc::new(Expr::Field {
                base: self.lower_expr(base)?,
                field: field.clone(),
                ty,
                span,
            }),
        };

        self.exprs.insert(&lowered);
        Ok(lowered)
    }
}

/// Flatten `a.b.c` into root `a` and path `["b", "c"]`.
fn flatten_field(expr: &AstExpr) -> Option<(String, Vec<String>)> {
    match expr {
        AstExpr::Name { name, .. } => Some((name.clone(), Vec::new())),
        AstExpr::Field { base, field, .. } => {
            let (root, mut path) = flatten_field(base)?;
            path.push(field.clone());
            Some((root, path))
        }
        _ => None,
    }
}
