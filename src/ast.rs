use crate::lexer::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub modules: Vec<Module>,
    /// Remote program units parsed from `REMOTE`/`SPEC REMOTE`.
    pub remotes: Vec<RemoteUnit>,
    pub generic_modules: Vec<GenericModuleTemplate>,
    pub generic_module_instances: Vec<GenericModuleInstance>,
}

#[derive(Debug, Clone)]
pub struct GenericModuleTemplate {
    pub name: String,
    pub formal_params: Vec<GenericFormal>,
    pub module: Module,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct GenericModuleInstance {
    pub name: String,
    pub template: String,
    pub actuals: Vec<GenericActual>,
    pub span: Span,
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
    Process(ProcessDef),
    ProcSignature(ProcSignature),
    Visibility(Visibility),
    Mode(ModeDefinition),
    /// A nested `SPEC MODULE` inside a spec module or context.
    SpecModule(Box<Module>),
    /// A generic procedure template parameterized only by modes.
    GenericProc(GenericProcTemplate),
    /// `name: PROC = NEW template (actual modes) END;`
    GenericProcInstance(GenericProcInstance),
    /// `SIGNAL name [= (modes)];`
    Signal(Vec<SignalDef>),
}

#[derive(Debug, Clone)]
pub enum GenericFormal {
    Mode(String),
    Syn {
        name: String,
        default: Expr,
    },
    Proc {
        name: String,
        params: Vec<Param>,
        result: Option<ModeRef>,
        exceptions: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum GenericActual {
    Mode(ModeRef),
    Value(Expr),
    Proc { name: String, target: String },
}

#[derive(Debug, Clone)]
pub struct GenericProcTemplate {
    pub name: String,
    pub formal_params: Vec<GenericFormal>,
    pub proc_def: ProcDef,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct GenericProcInstance {
    pub name: String,
    pub template: String,
    pub actuals: Vec<GenericActual>,
    pub span: Span,
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
    Set(Vec<SetElement>),
    Moreta {
        spec: bool,
        bases: Vec<String>,
        declarations: Vec<Declaration>,
        invariant: Option<Expr>,
        span: Span,
    },
    Alias(ModeRef),
}

#[derive(Debug, Clone)]
pub struct SetElement {
    pub name: String,
    pub value: Option<i128>,
    pub span: Span,
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
    pub constructor: bool,
    pub destructor: bool,
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
    pub constructor_args: Option<Vec<Expr>>,
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
    /// Inline `ARRAY (low:high, ...) mode` bounds.
    pub arrays: Vec<ArrayBounds>,
    /// `CHARS(n)` / `WCHARS(n)` length, if specified.
    pub string_length: Option<usize>,
    /// `VARYING` after a string length.
    pub varying: bool,
    /// True for an `ACCESS` mode.
    pub access: bool,
    /// True for a `POWERSET member-mode` mode.
    pub powerset: bool,
    /// True for a `REF mode` reference mode.
    pub reference: bool,
    /// True for a `ROW mode` row mode.
    pub row: bool,
    /// True when ACCESS declares an index mode.
    pub access_indexed: bool,
    /// Literal index range for an indexed ACCESS mode, if specified.
    pub access_index: Option<ArrayBounds>,
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
pub struct SignalDef {
    pub name: String,
    pub modes: Vec<ModeRef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProcessDef {
    pub name: String,
    pub params: Vec<Param>,
    pub declarations: Vec<Declaration>,
    pub actions: Vec<Action>,
    pub handler: Option<Handler>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProcDef {
    pub name: String,
    pub params: Vec<Param>,
    pub result: Option<ModeRef>,
    pub exceptions: Vec<String>,
    pub constructor: bool,
    pub destructor: bool,
    pub declarations: Vec<Declaration>,
    pub actions: Vec<Action>,
    pub handler: Option<Handler>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ReceiveAlternative {
    pub signal: String,
    pub bindings: Vec<String>,
    pub actions: Vec<Action>,
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
    /// `START process(args);`
    Start {
        process: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// `STOP;`
    Stop {
        span: Span,
    },
    Delay {
        event: String,
        priority: Option<Expr>,
        span: Span,
    },
    Continue {
        event: String,
        span: Span,
    },
    Send {
        signal: String,
        values: Vec<Expr>,
        span: Span,
    },
    ReceiveCase {
        alternatives: Vec<ReceiveAlternative>,
        else_actions: Option<Vec<Action>>,
        span: Span,
    },
    Timing {
        duration: Expr,
        delay: bool,
        body: Vec<Action>,
        timeout: Vec<Action>,
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

#[derive(Debug, Clone)]
pub struct ArrayTupleElement {
    /// `None` for an unlabelled tuple entry.
    pub labels: Option<Vec<CaseLabel>>,
    pub value: Expr,
    /// Powerset range upper bound, as in `[low:high]`.
    pub range_end: Option<Box<Expr>>,
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
    Float {
        value: f64,
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
    WideChar {
        value: char,
        span: Span,
    },
    WideString {
        value: String,
        span: Span,
    },
    Bits {
        value: String,
        span: Span,
    },
    Null {
        span: Span,
    },
    SelfRef {
        span: Span,
    },
    /// `-> location`, creating a reference to a location.
    AddressOf {
        target: Box<Expr>,
        span: Span,
    },
    /// `reference -> [mode]`, dereferencing a reference or row.
    Deref {
        base: Box<Expr>,
        mode: Option<Box<ModeRef>>,
        span: Span,
    },
    /// `ALLOCATE(mode [, value])` or `GETSTACK(mode [, value])`.
    Allocate {
        mode: Box<ModeRef>,
        value: Option<Box<Expr>>,
        constructor_args: Option<Vec<Expr>>,
        stack: bool,
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
    /// An array tuple: `(: v, ... :)`, `[v, ...]`, or labelled entries.
    ArrayTuple {
        elements: Vec<ArrayTupleElement>,
        span: Span,
    },
    /// A component procedure call `object.method(...)`.
    MethodCall {
        base: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// An array slice: `a(lower : upper)` or `a(first UP size)`.
    ArraySlice {
        name: String,
        first: Box<Expr>,
        last: Box<Expr>,
        /// True for the `UP` form, where `last` is the slice size.
        up: bool,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Integer { span, .. }
            | Self::Float { span, .. }
            | Self::Bool { span, .. }
            | Self::Char { span, .. }
            | Self::String { span, .. }
            | Self::WideChar { span, .. }
            | Self::WideString { span, .. }
            | Self::Bits { span, .. }
            | Self::Null { span }
            | Self::SelfRef { span }
            | Self::AddressOf { span, .. }
            | Self::Deref { span, .. }
            | Self::Allocate { span, .. }
            | Self::Name { span, .. }
            | Self::Apply { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::Field { span, .. }
            | Self::ArrayTuple { span, .. }
            | Self::ArraySlice { span, .. }
            | Self::MethodCall { span, .. } => *span,
        }
    }
}

impl Action {
    pub fn span(&self) -> Span {
        match self {
            Self::Assign { span, .. }
            | Self::Assert { span, .. }
            | Self::Call { span, .. }
            | Self::Start { span, .. }
            | Self::Stop { span }
            | Self::Delay { span, .. }
            | Self::Continue { span, .. }
            | Self::Send { span, .. }
            | Self::ReceiveCase { span, .. }
            | Self::Timing { span, .. }
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
    In,
}

impl BinaryOp {
    /// Binding power, low to high, following Z.200 §5.3.3–5.3.9.
    pub fn precedence(self) -> u8 {
        match self {
            Self::Or | Self::OrIf | Self::Xor => 1,
            Self::And | Self::AndIf => 2,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 3,
            Self::In => 3,
            Self::Add | Self::Sub | Self::Concat => 4,
            Self::Mul | Self::Div | Self::Mod | Self::Rem => 5,
            Self::Pow => 6,
        }
    }
}
