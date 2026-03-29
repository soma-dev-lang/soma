use std::fmt;

/// Source location for error reporting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// A node with source location
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

// ── Program ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Program {
    pub imports: Vec<String>,
    pub cells: Vec<Spanned<CellDef>>,
}

// ── Cell ─────────────────────────────────────────────────────────────

/// What kind of cell is this?
/// The language grows by defining new kinds of cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellKind {
    /// Regular cell: `cell Foo { ... }`
    Cell,
    /// Property definition: `cell property persistent { ... }`
    Property,
    /// Type definition: `cell type Log<T> { ... }`
    Type,
    /// Checker rule: `cell checker auth_required { ... }`
    Checker,
    /// Storage backend: `cell backend sqlite { ... }`
    Backend,
    /// Built-in function: `cell builtin print { ... }`
    Builtin,
    /// Test cell: `cell test MyTests { assert expr == expected }`
    Test,
}

#[derive(Debug, Clone)]
pub struct CellDef {
    pub kind: CellKind,
    pub name: String,
    /// Generic type parameters for `cell type` definitions
    pub type_params: Vec<String>,
    pub sections: Vec<Spanned<Section>>,
}

#[derive(Debug, Clone)]
pub enum Section {
    Face(FaceSection),
    Memory(MemorySection),
    Interior(InteriorSection),
    OnSignal(OnSection),
    /// Rules section for meta-cells (properties, checkers)
    Rules(RulesSection),
    /// Runtime section: how this cell executes
    Runtime(RuntimeSection),
    /// State machine section
    State(StateMachineSection),
    /// Scheduled execution: every 30s { ... }
    Every(EverySection),
    /// Orchestration: scale { replicas: 100, shard: data, ... }
    Scale(ScaleSection),
}

#[derive(Debug, Clone)]
pub struct EverySection {
    pub interval_ms: u64,
    pub body: Vec<Spanned<Statement>>,
}

// ── Scale (Orchestration) ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScaleSection {
    pub replicas: u64,
    pub shard: Option<String>,
    pub consistency: ScaleConsistency,
    pub tolerance: u64,
    // Resources per instance
    pub cpu: Option<u64>,           // cores
    pub memory: Option<String>,     // "8Gi", "512Mi"
    pub disk: Option<String>,       // "100Gi", "1Ti"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScaleConsistency {
    Strong,
    Causal,
    Eventual,
}

impl std::fmt::Display for ScaleConsistency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScaleConsistency::Strong => write!(f, "strong"),
            ScaleConsistency::Causal => write!(f, "causal"),
            ScaleConsistency::Eventual => write!(f, "eventual"),
        }
    }
}

// ── Face (Contract) ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FaceSection {
    pub declarations: Vec<Spanned<FaceDecl>>,
}

#[derive(Debug, Clone)]
pub enum FaceDecl {
    Given(GivenDecl),
    Promise(PromiseDecl),
    Signal(SignalDecl),
    Await(AwaitDecl),
}

#[derive(Debug, Clone)]
pub struct GivenDecl {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub where_clause: Option<Vec<Spanned<Constraint>>>,
}

#[derive(Debug, Clone)]
pub struct PromiseDecl {
    pub constraint: Spanned<Constraint>,
}

#[derive(Debug, Clone)]
pub struct SignalDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
}

#[derive(Debug, Clone)]
pub struct AwaitDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Spanned<TypeExpr>>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
}

// ── Memory (State) ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MemorySection {
    pub slots: Vec<Spanned<MemorySlot>>,
}

#[derive(Debug, Clone)]
pub struct MemorySlot {
    pub name: String,
    pub ty: Spanned<TypeExpr>,
    pub properties: Vec<Spanned<MemoryProperty>>,
}

/// Memory properties are now string-based and resolved against the registry.
/// This allows user-defined properties without changing the compiler.
#[derive(Debug, Clone)]
pub enum MemoryProperty {
    /// Flag property: `persistent`, `encrypted`, `my_custom_prop`
    Flag(String),
    /// Parameterized property: `capacity(1000)`, `ttl(30min)`, `partitioned(user_id, 16)`
    Param(PropertyParam),
}

impl MemoryProperty {
    pub fn name(&self) -> &str {
        match self {
            MemoryProperty::Flag(name) => name,
            MemoryProperty::Param(p) => &p.name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PropertyParam {
    pub name: String,
    pub values: Vec<Spanned<Literal>>,
}

// ── Rules Section (for meta-cells) ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct RulesSection {
    pub rules: Vec<Spanned<Rule>>,
}

#[derive(Debug, Clone)]
pub enum Rule {
    /// `contradicts [ephemeral, retain, replicated]`
    Contradicts(Vec<String>),
    /// `implies [persistent]`
    Implies(Vec<String>),
    /// `requires [persistent]` — must coexist
    Requires(Vec<String>),
    /// `mutex_group durability` — at most one from this group
    MutexGroup(String),
    /// `check { ... }` — custom validation logic (for checker cells)
    Check(Vec<Spanned<Statement>>),
    /// `matches [persistent, consistent]` — backend selection rule
    Matches(Vec<String>),
    /// `native "rust_fn_name"` — bridge to native implementation
    Native(String),
    /// `assert expr == expected` — test assertion
    Assert(Spanned<Expr>),
}

// ── Runtime Section ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RuntimeSection {
    pub entries: Vec<Spanned<RuntimeEntry>>,
}

#[derive(Debug, Clone)]
pub enum RuntimeEntry {
    /// `emit signal_name(args)` — fire a signal on startup or in response
    Emit { signal_name: String, args: Vec<Spanned<Expr>> },
    /// `connect cell_a.signal -> cell_b` — explicit signal wiring
    Connect { from_cell: String, signal: String, to_cell: String },
    /// `start cell_name` — launch a child cell
    Start { cell_name: String },
    /// Arbitrary statement for scripting
    Stmt(Statement),
}

// ── State Machine ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StateMachineSection {
    pub name: String,
    pub initial: String,
    pub transitions: Vec<Spanned<Transition>>,
}

#[derive(Debug, Clone)]
pub struct Transition {
    /// Source state ("*" = any)
    pub from: String,
    /// Target state
    pub to: String,
    /// Guard condition (must be true for transition to proceed)
    pub guard: Option<Spanned<Expr>>,
    /// Effect statements (run after transition)
    pub effect: Vec<Spanned<Statement>>,
}

// ── Interior (Children) ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InteriorSection {
    pub cells: Vec<Spanned<CellDef>>,
}

// ── Signal Handlers ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OnSection {
    pub signal_name: String,
    pub params: Vec<Param>,
    pub body: Vec<Spanned<Statement>>,
    /// Handler properties: e.g. [native]
    pub properties: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Statement {
    Let {
        name: String,
        value: Spanned<Expr>,
    },
    Assign {
        name: String,
        value: Spanned<Expr>,
    },
    Return {
        value: Spanned<Expr>,
    },
    If {
        condition: Spanned<Expr>,
        then_body: Vec<Spanned<Statement>>,
        else_body: Vec<Spanned<Statement>>,
    },
    For {
        var: String,
        iter: Spanned<Expr>,
        body: Vec<Spanned<Statement>>,
    },
    While {
        condition: Spanned<Expr>,
        body: Vec<Spanned<Statement>>,
    },
    Emit {
        signal_name: String,
        args: Vec<Spanned<Expr>>,
    },
    Require {
        constraint: Spanned<Constraint>,
        else_signal: String,
    },
    MethodCall {
        target: String,
        method: String,
        args: Vec<Spanned<Expr>>,
    },
    Break,
    Continue,
    /// Bare expression statement (for function calls as statements)
    ExprStmt {
        expr: Spanned<Expr>,
    },
}

// ── Types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum TypeExpr {
    /// Simple type: Int, String, Bool, Float
    Simple(String),
    /// Generic type: Map<K, V>, List<T>, Result<T, E>
    Generic {
        name: String,
        args: Vec<Spanned<TypeExpr>>,
    },
    /// Reference to another cell's type: Orders.OrderId
    CellRef {
        cell: String,
        member: String,
    },
}

// ── Constraints ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Constraint {
    /// a < b, a == b, etc.
    Comparison {
        left: Spanned<Expr>,
        op: CmpOp,
        right: Spanned<Expr>,
    },
    /// valid(user), sorted(list)
    Predicate {
        name: String,
        args: Vec<Spanned<Expr>>,
    },
    /// constraint && constraint
    And(Box<Spanned<Constraint>>, Box<Spanned<Constraint>>),
    /// constraint || constraint
    Or(Box<Spanned<Constraint>>, Box<Spanned<Constraint>>),
    /// !constraint
    Not(Box<Spanned<Constraint>>),
    /// A textual/descriptive promise: promise "all payments settle within 24h"
    Descriptive(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

impl fmt::Display for CmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lt => write!(f, "<"),
            Self::Gt => write!(f, ">"),
            Self::Le => write!(f, "<="),
            Self::Ge => write!(f, ">="),
            Self::Eq => write!(f, "=="),
            Self::Ne => write!(f, "!="),
        }
    }
}

// ── Expressions ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    FieldAccess {
        target: Box<Spanned<Expr>>,
        field: String,
    },
    MethodCall {
        target: Box<Spanned<Expr>>,
        method: String,
        args: Vec<Spanned<Expr>>,
    },
    /// Free function / signal call: compute(n - 1)
    FnCall {
        name: String,
        args: Vec<Spanned<Expr>>,
    },
    BinaryOp {
        left: Box<Spanned<Expr>>,
        op: BinOp,
        right: Box<Spanned<Expr>>,
    },
    /// Comparison operators as expressions (for if conditions)
    CmpOp {
        left: Box<Spanned<Expr>>,
        op: CmpOp,
        right: Box<Spanned<Expr>>,
    },
    Not(Box<Spanned<Expr>>),
    /// Record literal: User { name: "Alice", age: 30 }
    Record {
        type_name: String,
        fields: Vec<(String, Spanned<Expr>)>,
    },
    /// Try expression: try { expr } returns map with value or error
    Try(Box<Spanned<Expr>>),
    /// Pipe: expr |> fn(args) → fn(expr, args)
    Pipe {
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },
    /// Match expression: match expr { pattern -> result, ... }
    Match {
        subject: Box<Spanned<Expr>>,
        arms: Vec<MatchArm>,
    },
    /// Lambda: s => expr
    Lambda {
        param: String,
        body: Box<Spanned<Expr>>,
    },
    /// Block lambda: s => { stmts; result_expr }
    LambdaBlock {
        param: String,
        stmts: Vec<Spanned<Statement>>,
        result: Box<Spanned<Expr>>,
    },
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Vec<Spanned<Statement>>,
    /// The result expression (last expression in body, or the single expression after ->)
    pub result: Spanned<Expr>,
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Literal(Literal),
    Wildcard, // _
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Duration(Duration),
    Percentage(f64),
    Unit,
}

#[derive(Debug, Clone)]
pub struct Duration {
    pub value: f64,
    pub unit: DurationUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
    Years,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => write!(f, "+"),
            Self::Sub => write!(f, "-"),
            Self::Mul => write!(f, "*"),
            Self::Div => write!(f, "/"),
            Self::Mod => write!(f, "%"),
            Self::And => write!(f, "&&"),
            Self::Or => write!(f, "||"),
        }
    }
}
