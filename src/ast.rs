use crate::lexer::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub modules: Vec<Module>,
    /// Remote program units parsed from `REMOTE`/`SPEC REMOTE`.
    pub remotes: Vec<RemoteUnit>,
}

#[derive(Debug, Clone)]
pub struct PieceDesignator {
    pub kind: PieceDesignatorKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PieceDesignatorKind {
    Empty,
    Literal(String),
    TextRef(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    Module,
    SpecModule,
    ProgramUnit,
    ContextModule,
}

#[derive(Debug, Clone)]
pub struct RemoteUnit {
    pub kind: RemoteKind,
    pub name: Option<String>,
    pub designator: PieceDesignator,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub name: Option<String>,
    pub kind: ModuleKind,
    /// Package that owns this program unit. Assigned by the loader.
    pub package: usize,
    /// Package name, assigned by the loader.
    pub package_name: Option<String>,
    /// `CONTEXT ... FOR` quasi reaches in front of the module.
    pub contexts: Vec<Context>,
    pub declarations: Vec<Declaration>,
    pub actions: Vec<Action>,
    pub handler: Option<Handler>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Handler {
    pub alternatives: Vec<HandlerAlternative>,
    pub else_actions: Option<Vec<Action>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HandlerAlternative {
    pub exceptions: Vec<String>,
    pub actions: Vec<Action>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Context {
    pub declarations: Vec<Declaration>,
    /// `CONTEXT REMOTE <designator> ... FOR`
    pub remote: Option<PieceDesignator>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    Body,
    Spec,
    Region,
    SpecRegion,
}

impl ModuleKind {
    pub fn is_spec(self) -> bool {
        matches!(self, Self::Spec | Self::SpecRegion)
    }

    pub fn is_region(self) -> bool {
        matches!(self, Self::Region | Self::SpecRegion)
    }

    pub fn is_body(self) -> bool {
        !self.is_spec()
    }
}

#[derive(Debug, Clone)]
pub enum Declaration {
    Var(Vec<VarDecl>),
    Proc(ProcDef),
    ProcSignature(ProcSignature),
    Visibility(Visibility),
    Mode(ModeDefinition),
    /// A nested `SPEC MODULE` inside a spec module or context.
    SpecModule(Box<Module>),
}

#[derive(Debug, Clone)]
pub struct ModeDefinition {
    pub name: String,
    pub body: ModeDefinitionBody,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ModeDefinitionBody {
    Struct(Vec<FieldDef>),
    Alias(ModeRef),
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub mode: ModeRef,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProcSignature {
    pub name: String,
    pub params: Vec<Param>,
    pub result: Option<ModeRef>,
    pub exceptions: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Visibility {
    pub kind: VisibilityKind,
    pub names: Vec<NameRef>,
    pub all: bool,
    /// `TO` friend modules for a GRANT.
    pub friends: Vec<NameRef>,
    /// Prefix clause or prefix rename clause (§12.2.3.3).
    pub prefix: Option<PrefixClause>,
    /// `FORBID` restriction on a granted structure mode.
    pub forbid: Option<Forbid>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixClause {
    /// `PREFIXED [prefix]`. `None` uses the granting module's name.
    Prefixed(Option<Vec<String>>),
    /// `(old -> new) ! postfix`
    Rename { old: Vec<String>, new: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct Forbid {
    pub fields: Vec<String>,
    pub all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityKind {
    Grant,
    Seize,
}

#[derive(Debug, Clone)]
pub struct NameRef {
    /// Prefix parts before `!`, outermost first.
    pub prefix: Vec<String>,
    pub name: String,
    pub span: Span,
}

impl NameRef {
    pub fn qualified(&self) -> String {
        if self.prefix.is_empty() {
            self.name.clone()
        } else {
            format!("{}!{}", self.prefix.join("!"), self.name)
        }
    }
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: String,
    pub mode: ModeRef,
    pub init: Option<Initialization>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Initialization {
    pub kind: InitKind,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitKind {
    ReachBound,
    LifetimeBound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeRef {
    pub name: String,
    /// Inline `ARRAY (low:high) mode` bounds.
    pub array: Option<ArrayBounds>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayBounds {
    pub low: i128,
    pub high: i128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Passing {
    In,
    Out,
    InOut,
    Loc,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub mode: ModeRef,
    pub passing: Passing,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProcDef {
    pub name: String,
    pub params: Vec<Param>,
    pub result: Option<ModeRef>,
    pub exceptions: Vec<String>,
    pub declarations: Vec<Declaration>,
    pub actions: Vec<Action>,
    pub handler: Option<Handler>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Action {
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    Assert {
        condition: Expr,
        span: Span,
    },
    Call {
        callee: Expr,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Result {
        value: Expr,
        span: Span,
    },
    If {
        branches: Vec<Branch>,
        else_actions: Option<Vec<Action>>,
        else_span: Option<Span>,
        span: Span,
    },
    Do {
        control: DoControl,
        body: Vec<Action>,
        span: Span,
    },
    Exit {
        label: Option<String>,
        span: Span,
    },
    Case {
        selector: Expr,
        alternatives: Vec<CaseAlternative>,
        else_actions: Option<Vec<Action>>,
        else_span: Option<Span>,
        span: Span,
    },
    Labelled {
        label: String,
        action: Box<Action>,
        span: Span,
    },
    Begin {
        declarations: Vec<Declaration>,
        actions: Vec<Action>,
        span: Span,
    },
    Empty {
        span: Span,
    },
    Cause {
        name: String,
        span: Span,
    },
    /// A module used as an action (a nested modulion).
    Module(Box<Module>),
    /// An action with an exception handler appended (Z.200 8.2).
    Handled {
        action: Box<Action>,
        handler: Handler,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub condition: Expr,
    pub body: Vec<Action>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CaseAlternative {
    pub labels: Vec<CaseLabel>,
    pub body: Vec<Action>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseLabel {
    Value(i128),
    Range { low: i128, high: i128 },
    Else,
    Any,
}

#[derive(Debug, Clone)]
pub enum DoControl {
    None,
    While(Expr),
    ForEver,
    For {
        counter: String,
        start: Expr,
        end: Expr,
        step: Option<Expr>,
        down: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Concat,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Integer {
        value: i128,
        base: u32,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Char {
        value: char,
        span: Span,
    },
    String {
        value: String,
        span: Span,
    },
    Name {
        name: String,
        span: Span,
    },
    /// A procedure call or a mode conversion. The mode checker resolves which.
    Apply {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    Field {
        base: Box<Expr>,
        field: String,
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
            | Self::Apply { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::Field { span, .. } => *span,
        }
    }
}

impl Action {
    pub fn span(&self) -> Span {
        match self {
            Self::Assign { span, .. }
            | Self::Assert { span, .. }
            | Self::Call { span, .. }
            | Self::Return { span, .. }
            | Self::Result { span, .. }
            | Self::If { span, .. }
            | Self::Do { span, .. }
            | Self::Exit { span, .. }
            | Self::Case { span, .. }
            | Self::Labelled { span, .. }
            | Self::Begin { span, .. }
            | Self::Empty { span }
            | Self::Cause { span, .. }
            | Self::Handled { span, .. } => *span,
            Self::Module(module) => module.span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    OrIf,
    Xor,
    And,
    AndIf,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Concat,
    Mul,
    Div,
    Mod,
    Rem,
    Pow,
}

impl BinaryOp {
    /// Binding power, low to high, following Z.200 §5.3.3–5.3.9.
    pub fn precedence(self) -> u8 {
        match self {
            Self::Or | Self::OrIf | Self::Xor => 1,
            Self::And | Self::AndIf => 2,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 3,
            Self::Add | Self::Sub | Self::Concat => 4,
            Self::Mul | Self::Div | Self::Mod | Self::Rem => 5,
            Self::Pow => 6,
        }
    }
}
