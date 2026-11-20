use crate::ast::{Action, Declaration, DoControl, Expr as AstExpr, Module, Program, UnaryOp};
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
            | Self::Binary { span, .. } => *span,
        }
    }
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

pub fn lower(program: &Program, check: &CheckResult) -> Result<ExprTable, Error> {
    let mut lowerer = Lowerer {
        check,
        table: ExprTable::default(),
    };

    for module in &program.modules {
        lowerer.lower_module(module)?;
    }

    Ok(lowerer.table)
}

struct Lowerer<'a> {
    check: &'a CheckResult,
    table: ExprTable,
}

impl Lowerer<'_> {
    fn lower_module(&mut self, module: &Module) -> Result<(), Error> {
        self.lower_declarations(&module.declarations)?;
        self.lower_actions(&module.actions)
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
                    self.lower_actions(&proc_def.actions)?;
                }
                Declaration::ProcSignature(_) | Declaration::Visibility(_) => {}
            }
        }
        Ok(())
    }

    fn lower_actions(&mut self, actions: &[Action]) -> Result<(), Error> {
        for action in actions {
            match action {
                Action::Assign { target, value, .. } => {
                    self.lower_expr(target)?;
                    self.lower_expr(value)?;
                }
                Action::Assert { condition, .. } => {
                    self.lower_expr(condition)?;
                }
                Action::Call { callee, .. } => {
                    self.lower_expr(callee)?;
                }
                Action::Return { value, .. } => {
                    if let Some(value) = value {
                        self.lower_expr(value)?;
                    }
                }
                Action::Result { value, .. } => {
                    self.lower_expr(value)?;
                }
                Action::If {
                    branches,
                    else_actions,
                    ..
                } => {
                    for branch in branches {
                        self.lower_expr(&branch.condition)?;
                        self.lower_actions(&branch.body)?;
                    }
                    if let Some(actions) = else_actions {
                        self.lower_actions(actions)?;
                    }
                }
                Action::Do { control, body, .. } => {
                    match control {
                        DoControl::None | DoControl::ForEver => {}
                        DoControl::While(condition) => {
                            self.lower_expr(condition)?;
                        }
                        DoControl::For {
                            start, end, step, ..
                        } => {
                            self.lower_expr(start)?;
                            self.lower_expr(end)?;
                            if let Some(step) = step {
                                self.lower_expr(step)?;
                            }
                        }
                    }
                    self.lower_actions(body)?;
                }
                Action::Exit { .. } | Action::Empty { .. } => {}
                Action::Case {
                    selector,
                    alternatives,
                    else_actions,
                    ..
                } => {
                    self.lower_expr(selector)?;
                    for alternative in alternatives {
                        self.lower_actions(&alternative.body)?;
                    }
                    if let Some(actions) = else_actions {
                        self.lower_actions(actions)?;
                    }
                }
                Action::Labelled { action, .. } => {
                    self.lower_actions(std::slice::from_ref(action.as_ref()))?;
                }
                Action::Begin {
                    declarations,
                    actions,
                    ..
                } => {
                    self.lower_declarations(declarations)?;
                    self.lower_actions(actions)?;
                }
            }
        }
        Ok(())
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
                None => {
                    return Err(Error::new(span, "internal: call/apply has no resolution"));
                }
            };

            self.table.insert(&lowered);
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
        };

        self.table.insert(&lowered);
        Ok(lowered)
    }
}
