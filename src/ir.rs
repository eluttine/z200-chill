use crate::ast::{
    Action, CaseLabel, Declaration, DoControl, Expr as AstExpr, Module, Passing, Program, UnaryOp,
};
use crate::lexer::Span;
use crate::sema::{ApplyResolution, CheckResult};
use crate::types::{IntKind, Type};
use std::collections::HashMap;
use std::rc::Rc;

pub type Node = Rc<Expr>;

#[derive(Debug)]
pub enum TextIoArg {
    Value(Node),
    Location(String),
}

#[derive(Debug)]
pub enum Expr {
    Integer {
        value: i128,
        base: u32,
        ty: Type,
        span: Span,
    },
    Float {
        value: f64,
        ty: Type,
        span: Span,
    },
    FloatConversion {
        arg: Node,
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
    Bits {
        value: String,
        ty: Type,
        span: Span,
    },
    Null {
        ty: Type,
        span: Span,
    },
    AddressOf {
        name: String,
        ty: Type,
        span: Span,
    },
    Deref {
        base: Node,
        ty: Type,
        span: Span,
    },
    Allocate {
        mode: Type,
        value: Option<Node>,
        ty: Type,
        span: Span,
    },
    Name {
        name: String,
        ty: Type,
        span: Span,
    },
    SetValue {
        value: i128,
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
        passing: Vec<Passing>,
        targets: Vec<Option<AssignTarget>>,
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
        indices: Vec<Node>,
        array: Type,
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
        ops: Vec<crate::builtins::FormatOp>,
        io: Vec<TextIoArg>,
        span: Span,
    },
    ArrayLiteral {
        values: Vec<Node>,
        array: crate::types::ArrayType,
        span: Span,
    },
    ArraySlice {
        name: String,
        offset: i128,
        count: i128,
        array: crate::types::ArrayType,
        span: Span,
    },
    ArraySliceDynamic {
        name: String,
        first: Node,
        last: Node,
        up: bool,
        low: i128,
        element: crate::types::ArrayElement,
        span: Span,
    },
    LabelledArrayLiteral {
        entries: Vec<(Vec<i128>, Node)>,
        array: crate::types::ArrayType,
        span: Span,
    },
    PowerSetLiteral {
        entries: Vec<(Node, Option<Node>)>,
        ty: Type,
        span: Span,
    },
    MethodCall {
        object: String,
        proc_span: Span,
        args: Vec<Node>,
        passing: Vec<Passing>,
        targets: Vec<Option<AssignTarget>>,
        ty: Option<Type>,
        span: Span,
    },
    Association {
        op: crate::builtins::AssociationOp,
        first: String,
        second: Option<String>,
        index: Option<Node>,
        value: Option<Node>,
        ty: Option<Type>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Integer { span, .. }
            | Self::Float { span, .. }
            | Self::FloatConversion { span, .. }
            | Self::Bool { span, .. }
            | Self::Char { span, .. }
            | Self::String { span, .. }
            | Self::Bits { span, .. }
            | Self::Null { span, .. }
            | Self::AddressOf { span, .. }
            | Self::Deref { span, .. }
            | Self::Allocate { span, .. }
            | Self::Name { span, .. }
            | Self::SetValue { span, .. }
            | Self::Conversion { span, .. }
            | Self::Call { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::Field { span, .. }
            | Self::Index { span, .. }
            | Self::Builtin { span, .. }
            | Self::TextIo { span, .. }
            | Self::ArrayLiteral { span, .. }
            | Self::ArraySlice { span, .. }
            | Self::ArraySliceDynamic { span, .. }
            | Self::LabelledArrayLiteral { span, .. }
            | Self::PowerSetLiteral { span, .. }
            | Self::MethodCall { span, .. }
            | Self::Association { span, .. } => *span,
        }
    }

    pub fn ty(&self) -> Option<Type> {
        match self {
            Self::Integer { ty, .. }
            | Self::Float { ty, .. }
            | Self::FloatConversion { ty, .. }
            | Self::Bool { ty, .. }
            | Self::Char { ty, .. }
            | Self::String { ty, .. }
            | Self::Bits { ty, .. }
            | Self::Null { ty, .. }
            | Self::AddressOf { ty, .. }
            | Self::Deref { ty, .. }
            | Self::Allocate { ty, .. }
            | Self::Name { ty, .. }
            | Self::SetValue { ty, .. }
            | Self::Conversion { ty, .. }
            | Self::Unary { ty, .. }
            | Self::Binary { ty, .. }
            | Self::Field { ty, .. }
            | Self::Index { ty, .. } => Some(*ty),
            Self::ArrayLiteral { array, .. }
            | Self::ArraySlice { array, .. }
            | Self::LabelledArrayLiteral { array, .. } => Some(Type::Array(*array)),
            Self::ArraySliceDynamic { low, element, .. } => Some(Type::DynamicArray {
                low: *low,
                element: *element,
            }),
            Self::PowerSetLiteral { ty, .. } => Some(*ty),
            Self::MethodCall { ty, .. } => *ty,
            Self::Call { ty, .. } | Self::Builtin { ty, .. } | Self::Association { ty, .. } => *ty,
            Self::TextIo { .. } => None,
        }
    }
}

/// An assignment location: a variable, or a field path rooted at a variable.
#[derive(Debug, Clone)]
pub enum AssignTarget {
    Var(String),
    Field {
        root: String,
        /// When present, the field path starts from this array element.
        index: Option<Node>,
        path: Vec<String>,
        ty: Type,
    },
    Index {
        name: String,
        indices: Vec<Node>,
        array: Type,
        ty: Type,
    },
    Slice {
        name: String,
        offset: i128,
        count: i128,
        ty: Type,
    },
    SliceDynamic {
        name: String,
        first: Node,
        last: Node,
        up: bool,
        low: i128,
        ty: Type,
    },
    Deref {
        reference: Node,
        ty: Type,
    },
    DerefField {
        reference: Node,
        path: Vec<String>,
        ty: Type,
    },
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
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
    /// An action together with its exception handler, keyed by `span` in the
    /// body table. BEGIN blocks keep their handler keyed by the block span so the
    /// handler runs in the block's reach.
    Handled {
        action: Box<Stmt>,
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
                Declaration::Mode(mode_def) => {
                    if let crate::ast::ModeDefinitionBody::Moreta {
                        declarations,
                        invariant,
                        ..
                    } = &mode_def.body
                    {
                        self.lower_declarations(declarations)?;
                        if let Some(invariant) = invariant {
                            self.lower_expr(invariant)?;
                        }
                    }
                }
                Declaration::ProcSignature(_)
                | Declaration::Visibility(_)
                | Declaration::SpecModule(_)
                | Declaration::GenericProc(_)
                | Declaration::GenericProcInstance(_) => {}
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
            } => {
                self.lower_declarations(declarations)?;
                Ok(Stmt::Begin {
                    declarations: declarations.clone(),
                    actions: self.lower_actions(actions)?,
                    span: *span,
                })
            }
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
            Action::Handled {
                action,
                handler,
                span,
            } => {
                let handler_ir = self.lower_handler(handler)?;
                let inner = self.lower_action(action)?;
                let handler_key = match &inner {
                    Stmt::Begin { span, .. } => *span,
                    _ => *span,
                };
                self.bodies.insert_handler(handler_key, handler_ir);
                Ok(Stmt::Handled {
                    action: Box::new(inner),
                    span: *span,
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
                let Some(ApplyResolution::ArrayIndex(array)) = self.check.resolutions.get(span)
                else {
                    return Err(Error::new(
                        *span,
                        "assignment target must be a variable, field, or array element",
                    ));
                };
                let ty = self.check.types.get(span).copied().ok_or_else(|| {
                    Error::new(*span, "internal: array element has no checked mode")
                })?;
                let indices = args
                    .iter()
                    .map(|index| self.lower_expr(index))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(AssignTarget::Index {
                    name: name.clone(),
                    indices,
                    array: *array,
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
                if let Some((reference, path)) = flatten_deref_field(target) {
                    return Ok(AssignTarget::DerefField {
                        reference: self.lower_expr(reference)?,
                        path,
                        ty,
                    });
                }
                let (root, index, path) = flatten_field_location(target).ok_or_else(|| {
                    Error::new(
                        target.span(),
                        "assignment target must be a variable, array element, or field of one",
                    )
                })?;
                let index = match index {
                    Some(index) => Some(self.lower_expr(index)?),
                    None => None,
                };
                Ok(AssignTarget::Field {
                    root,
                    index,
                    path,
                    ty,
                })
            }
            AstExpr::Deref { base, span, .. } => {
                let ty = self.check.types.get(span).copied().ok_or_else(|| {
                    Error::new(*span, "internal: dereference has no checked mode")
                })?;
                Ok(AssignTarget::Deref {
                    reference: self.lower_expr(base)?,
                    ty,
                })
            }
            AstExpr::ArraySlice {
                name,
                first,
                last,
                up,
                span,
            } => {
                let ty = self
                    .check
                    .types
                    .get(span)
                    .copied()
                    .ok_or_else(|| Error::new(*span, "internal: slice has no checked mode"))?;
                match ty {
                    Type::Array(array) => {
                        let first_value = crate::sema::constant_int(first).ok_or_else(|| {
                            Error::new(first.span(), "array slice bounds must be constant")
                        })?;
                        let last_value = crate::sema::constant_int(last).ok_or_else(|| {
                            Error::new(last.span(), "array slice bounds must be constant")
                        })?;
                        let upper = if *up {
                            first_value
                                .checked_add(last_value - 1)
                                .ok_or_else(|| Error::new(*span, "array slice bounds overflow"))?
                        } else {
                            last_value
                        };
                        Ok(AssignTarget::Slice {
                            name: name.clone(),
                            offset: first_value - array.low,
                            count: upper - first_value + 1,
                            ty,
                        })
                    }
                    Type::DynamicArray { low, .. } => Ok(AssignTarget::SliceDynamic {
                        name: name.clone(),
                        first: self.lower_expr(first)?,
                        last: self.lower_expr(last)?,
                        up: *up,
                        low,
                        ty,
                    }),
                    _ => Err(Error::new(*span, "internal: slice is not an array")),
                }
            }
            _ => Err(Error::new(
                target.span(),
                "assignment target must be a variable or a field of a variable",
            )),
        }
    }

    fn lower_expr(&mut self, expr: &AstExpr) -> Result<Node, Error> {
        let span = expr.span();

        if let AstExpr::Apply {
            name,
            args: ast_args,
            ..
        } = expr
        {
            let args = ast_args
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
                Some(ApplyResolution::FloatConversion) => {
                    let ty = ty.ok_or_else(|| {
                        Error::new(span, "internal: FLOAT conversion has no checked mode")
                    })?;
                    Rc::new(Expr::FloatConversion {
                        arg: args
                            .into_iter()
                            .next()
                            .ok_or_else(|| Error::new(span, "FLOAT conversion has no argument"))?,
                        ty,
                        span,
                    })
                }
                Some(ApplyResolution::Procedure(signature)) => {
                    let targets = signature
                        .passing
                        .iter()
                        .zip(ast_args)
                        .map(|(passing, arg)| {
                            if *passing == Passing::In {
                                Ok(None)
                            } else {
                                Ok(Some(self.lower_target(arg)?))
                            }
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    Rc::new(Expr::Call {
                        name: name.clone(),
                        args,
                        passing: signature.passing.clone(),
                        targets,
                        ty,
                        span,
                    })
                }
                Some(ApplyResolution::TextIo {
                    kind,
                    format,
                    indexed,
                }) => {
                    let AstExpr::Name {
                        name: target_name, ..
                    } = &ast_args[0]
                    else {
                        return Err(Error::new(
                            ast_args[0].span(),
                            "internal: text argument is not a location",
                        ));
                    };
                    let mut lowered = args.into_iter();
                    let _target = lowered.next();
                    let index = if *indexed { lowered.next() } else { None };
                    let _format = lowered.next();
                    let io_values: Vec<Node> = lowered.collect();
                    let io = match kind {
                        crate::builtins::TextIo::Read => {
                            let io_start = if *indexed { 3 } else { 2 };
                            let items = crate::builtins::format_items(format);
                            items
                                .iter()
                                .zip(io_values)
                                .enumerate()
                                .map(|(offset, (item, node))| match item {
                                    crate::builtins::FormatItem::Width => {
                                        Ok(TextIoArg::Value(node))
                                    }
                                    crate::builtins::FormatItem::Conversion(_) => {
                                        let AstExpr::Name { name, .. } =
                                            &ast_args[io_start + offset]
                                        else {
                                            return Err(Error::new(
                                                ast_args[io_start + offset].span(),
                                                "internal: READTEXT conversion argument is not a location",
                                            ));
                                        };
                                        Ok(TextIoArg::Location(name.clone()))
                                    }
                                })
                                .collect::<Result<Vec<_>, _>>()?
                        }
                        crate::builtins::TextIo::Write => {
                            io_values.into_iter().map(TextIoArg::Value).collect()
                        }
                    };
                    Rc::new(Expr::TextIo {
                        kind: *kind,
                        target: target_name.clone(),
                        index,
                        ops: format.clone(),
                        io,
                        span,
                    })
                }
                Some(ApplyResolution::Association { op, index, target }) => {
                    use crate::builtins::AssociationOp;
                    let AstExpr::Name { name: first, .. } = &ast_args[0] else {
                        return Err(Error::new(
                            ast_args[0].span(),
                            "internal: association argument is not a location",
                        ));
                    };
                    let mut lowered = args.into_iter();
                    let _first = lowered.next();
                    let rest: Vec<Node> = lowered.collect();
                    let target_name = |position: usize| -> Result<String, Error> {
                        let AstExpr::Name { name, .. } = &ast_args[position] else {
                            return Err(Error::new(
                                ast_args[position].span(),
                                "internal: association argument is not a location",
                            ));
                        };
                        Ok(name.clone())
                    };
                    let (second, index_node, value) = match op {
                        AssociationOp::Associate => (None, None, rest.first().cloned()),
                        AssociationOp::Connect => (
                            Some(target_name(1)?),
                            if *index { rest.get(1).cloned() } else { None },
                            None,
                        ),
                        AssociationOp::ReadRecord => {
                            let target_position = if *index { 2 } else { 1 };
                            (
                                if *target {
                                    Some(target_name(target_position)?)
                                } else {
                                    None
                                },
                                if *index { rest.first().cloned() } else { None },
                                None,
                            )
                        }
                        AssociationOp::WriteRecord => (
                            None,
                            if *index { rest.first().cloned() } else { None },
                            rest.last().cloned(),
                        ),
                        _ => (None, None, None),
                    };
                    Rc::new(Expr::Association {
                        op: *op,
                        first: first.clone(),
                        second,
                        index: index_node,
                        value,
                        ty,
                        span,
                    })
                }
                Some(ApplyResolution::Builtin(builtin)) => Rc::new(Expr::Builtin {
                    builtin: *builtin,
                    args,
                    ty,
                    span,
                }),
                Some(ApplyResolution::ArrayIndex(array)) => {
                    let ty = ty.ok_or_else(|| {
                        Error::new(span, "internal: array index has no checked mode")
                    })?;
                    Rc::new(Expr::Index {
                        name: name.clone(),
                        indices: args,
                        array: *array,
                        ty,
                        span,
                    })
                }
                Some(ApplyResolution::SetElement { .. }) => {
                    return Err(Error::new(span, "internal: set element used as a call"));
                }
                Some(ApplyResolution::Method { .. }) => {
                    return Err(Error::new(span, "internal: method call used as an apply"));
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
            AstExpr::Float { value, .. } => Rc::new(Expr::Float {
                value: *value,
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
            AstExpr::WideChar { value, .. } => Rc::new(Expr::Char {
                value: *value,
                ty,
                span,
            }),
            AstExpr::WideString { value, .. } => Rc::new(Expr::String {
                value: value.clone(),
                ty,
                span,
            }),
            AstExpr::Bits { value, .. } => Rc::new(Expr::Bits {
                value: value.clone(),
                ty,
                span,
            }),
            AstExpr::Null { .. } => Rc::new(Expr::Null { ty, span }),
            AstExpr::AddressOf { target, span } => {
                let AstExpr::Name { name, .. } = target.as_ref() else {
                    return Err(Error::new(
                        *span,
                        "referencing currently requires a simple variable",
                    ));
                };
                Rc::new(Expr::AddressOf {
                    name: name.clone(),
                    ty,
                    span: *span,
                })
            }
            AstExpr::Deref { base, span, .. } => Rc::new(Expr::Deref {
                base: self.lower_expr(base)?,
                ty,
                span: *span,
            }),
            AstExpr::Allocate { value, span, .. } => {
                let Type::Ref(target) = ty else {
                    return Err(Error::new(
                        *span,
                        "internal: ALLOCATE has no reference type",
                    ));
                };
                Rc::new(Expr::Allocate {
                    mode: target.ty(),
                    value: value
                        .as_ref()
                        .map(|value| self.lower_expr(value))
                        .transpose()?,
                    ty,
                    span: *span,
                })
            }
            AstExpr::Name { name, .. } => {
                if let Some(ApplyResolution::SetElement { ty, value }) =
                    self.check.resolutions.get(&span)
                {
                    Rc::new(Expr::SetValue {
                        value: *value,
                        ty: *ty,
                        span,
                    })
                } else {
                    Rc::new(Expr::Name {
                        name: name.clone(),
                        ty,
                        span,
                    })
                }
            }
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
            AstExpr::ArraySlice {
                name,
                first,
                last,
                up,
                span: slice_span,
            } => {
                let checked = self.check.types.get(slice_span).copied().ok_or_else(|| {
                    Error::new(*slice_span, "internal: slice has no checked mode")
                })?;
                match checked {
                    Type::Array(array) => {
                        let first_value = crate::sema::constant_int(first).ok_or_else(|| {
                            Error::new(first.span(), "array slice bounds must be constant")
                        })?;
                        let last_value = crate::sema::constant_int(last).ok_or_else(|| {
                            Error::new(last.span(), "array slice bounds must be constant")
                        })?;
                        let upper = if *up {
                            first_value.checked_add(last_value - 1).ok_or_else(|| {
                                Error::new(*slice_span, "array slice bounds overflow")
                            })?
                        } else {
                            last_value
                        };
                        Rc::new(Expr::ArraySlice {
                            name: name.clone(),
                            offset: first_value - array.low,
                            count: upper - first_value + 1,
                            array,
                            span: *slice_span,
                        })
                    }
                    Type::DynamicArray { low, element } => {
                        let first = self.lower_expr(first)?;
                        let last = self.lower_expr(last)?;
                        Rc::new(Expr::ArraySliceDynamic {
                            name: name.clone(),
                            first,
                            last,
                            up: *up,
                            low,
                            element,
                            span: *slice_span,
                        })
                    }
                    _ => {
                        return Err(Error::new(*slice_span, "internal: slice is not an array"));
                    }
                }
            }
            AstExpr::MethodCall {
                base,
                args,
                span: call_span,
                ..
            } => {
                let AstExpr::Name { name: object, .. } = base.as_ref() else {
                    return Err(Error::new(
                        base.span(),
                        "internal: method call object is not a name",
                    ));
                };
                let Some(ApplyResolution::Method {
                    proc_span,
                    result,
                    passing,
                }) = self.check.resolutions.get(call_span)
                else {
                    return Err(Error::new(
                        *call_span,
                        "internal: method call has no resolution",
                    ));
                };
                let targets = passing
                    .iter()
                    .zip(args)
                    .map(|(passing, arg)| {
                        if *passing == Passing::In {
                            Ok(None)
                        } else {
                            Ok(Some(self.lower_target(arg)?))
                        }
                    })
                    .collect::<Result<Vec<_>, Error>>()?;
                let args = args
                    .iter()
                    .map(|arg| self.lower_expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Rc::new(Expr::MethodCall {
                    object: object.clone(),
                    proc_span: *proc_span,
                    args,
                    passing: passing.clone(),
                    targets,
                    ty: *result,
                    span: *call_span,
                })
            }
            AstExpr::Field { base, field, .. } => Rc::new(Expr::Field {
                base: self.lower_expr(base)?,
                field: field.clone(),
                ty,
                span,
            }),
            AstExpr::ArrayTuple { elements, .. } => {
                let checked = self
                    .check
                    .types
                    .get(&span)
                    .copied()
                    .unwrap_or(Type::ArrayTuple);
                if matches!(checked, Type::PowerSet(_)) {
                    let entries = elements
                        .iter()
                        .map(|element| {
                            Ok((
                                self.lower_expr(&element.value)?,
                                match &element.range_end {
                                    Some(end) => Some(self.lower_expr(end)?),
                                    None => None,
                                },
                            ))
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    return Ok(Rc::new(Expr::PowerSetLiteral {
                        entries,
                        ty: checked,
                        span,
                    }));
                }
                let Type::Array(array) = checked else {
                    return Err(Error::new(
                        span,
                        "array literal requires a one-dimensional array target mode",
                    ));
                };
                if let Some(indices) = self.check.labelled_tuples.get(&span) {
                    let entries = elements
                        .iter()
                        .zip(indices)
                        .map(|(element, indices)| {
                            Ok((indices.clone(), self.lower_expr(&element.value)?))
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    Rc::new(Expr::LabelledArrayLiteral {
                        entries,
                        array,
                        span,
                    })
                } else {
                    let values = elements
                        .iter()
                        .map(|element| self.lower_expr(&element.value))
                        .collect::<Result<Vec<_>, _>>()?;
                    Rc::new(Expr::ArrayLiteral {
                        values,
                        array,
                        span,
                    })
                }
            }
        };

        self.exprs.insert(&lowered);
        Ok(lowered)
    }
}

/// Flatten `a.b.c` or `a(i).b.c` into the root name, optional array index, and
/// field path.
fn flatten_deref_field(expr: &AstExpr) -> Option<(&AstExpr, Vec<String>)> {
    match expr {
        AstExpr::Field { base, field, .. } => match base.as_ref() {
            AstExpr::Deref { base, .. } => Some((base.as_ref(), vec![field.clone()])),
            _ => {
                let (reference, mut path) = flatten_deref_field(base)?;
                path.push(field.clone());
                Some((reference, path))
            }
        },
        _ => None,
    }
}

fn flatten_field_location(expr: &AstExpr) -> Option<(String, Option<&AstExpr>, Vec<String>)> {
    match expr {
        AstExpr::Name { name, .. } => Some((name.clone(), None, Vec::new())),
        AstExpr::Apply { name, args, .. } if args.len() == 1 => {
            Some((name.clone(), Some(&args[0]), Vec::new()))
        }
        AstExpr::Field { base, field, .. } => {
            let (root, index, mut path) = flatten_field_location(base)?;
            path.push(field.clone());
            Some((root, index, path))
        }
        _ => None,
    }
}
