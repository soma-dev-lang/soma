pub mod builtins;
pub mod native_ffi;
pub mod soma_int;
pub mod record_log;
pub mod horde;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use rustc_hash::FxHashMap;
use indexmap::IndexMap;

/// Fast environment map — uses FxHash (no crypto overhead) for variable lookups
pub(crate) type Env = FxHashMap<String, Value>;
use crate::ast::*;
use crate::runtime::storage::{StorageBackend, StoredValue};
pub use crate::interpreter::soma_int::SomaInt;
use num_bigint::BigInt;
use thiserror::Error;

/// How to undo one committed effect (see `Interpreter::journal`).
pub(crate) enum UndoOp {
    /// `set` / `delete`: restore the previous value, or remove the key
    Restore { backend: Arc<dyn StorageBackend>, key: String, prev: Option<crate::runtime::storage::StoredValue> },
    /// `next_id()`: undone with the whole handler, NOT by a failing `try`
    /// (the id had already escaped into a local: the next call handed out
    /// the same id and one record overwrote another)
    Counter { backend: Arc<dyn StorageBackend>, key: String, prev: Option<crate::runtime::storage::StoredValue> },
    /// `append` / `push`
    Unappend { backend: Arc<dyn StorageBackend> },
    /// `rows[i] = v` / `rows.delete(i)` on a List slot: put the old log back
    RestoreList { backend: Arc<dyn StorageBackend>, prev: Vec<crate::runtime::storage::StoredValue> },
    /// `rows[i] = v` on a List slot: put the one old element back
    ListSet { backend: Arc<dyn StorageBackend>, index: usize, prev: crate::runtime::storage::StoredValue },
    /// `rows.delete(i)` on a List slot: put the one removed element back
    ListRestore { backend: Arc<dyn StorageBackend>, token: i64, prev: crate::runtime::storage::StoredValue },
    /// a `publish` / `emit` push to SSE and WebSocket clients: held until the
    /// handler commits (a rolled-back handler told clients about a move
    /// that never happened); undoing it is dropping it
    Push(BusEvent),
    /// a cross-process `emit` line for the [peers] bus, sent at commit
    PeerSend(String),
    Cluster(crate::runtime::cluster::Update),
    /// a horde started / cancelled: applied to the live registry (workers
    /// start) when the unit commits; undoing it is dropping it
    Horde(horde::Commit),
}

/// One handler at a time. `soma serve` runs each request on its own thread
/// over shared storage; without this, two concurrent read-modify-write
/// handlers both read the old balance (measured: 300 payments of 10 against
/// 1000 with 50 parallel calls paid out 122–153 times). Handlers are
/// serialized: correctness first, the language's claim is that limits hold.
static HANDLER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn with_committed_storage<T>(f: impl FnOnce() -> T) -> T {
    let _guard = HANDLER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    f()
}
/// Set by `soma serve`: no terminal is attached to a request, so `approve()`
/// can never prompt — it fails closed instead of auto-approving.
pub static IN_SERVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// `soma test`: storage is in memory — no cross-process lock on disk (a
/// `.soma_data/` beside the tests made every call open lock.db: 15-60x slower)
pub static IN_TEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("undefined variable: {0}")]
    UndefinedVar(String),
    #[error("undefined function: {0}")]
    UndefinedFn(String),
    #[error("{0}")]
    TypeError(String),
    #[error("no handler found for signal '{0}' in cell '{1}'")]
    NoHandler(String, String),
    #[error("{}", display_require(.0))]
    RequireFailed(String),
    /// `fail("not_found", "reservation {id}")` — a domain error with a kind
    /// a caller can branch on (`r.kind == "not_found"`).
    #[error("{message}")]
    Domain { kind: String, message: String },
    /// A transaction boundary failed: execution cannot safely continue in `try`.
    #[error("storage: {0}")]
    StorageTransaction(String),
    #[error("stack overflow (recursion depth exceeded)")]
    StackOverflow,
}

/// RequireFailed carries three different things: a `require … else Tag`
/// that failed, and the runtime's own refusals (illegal transition, failed
/// guard, violated invariant, failed ensure). Only the first is a
/// "require"; the others are reported as what they are.
fn display_require(msg: &str) -> String {
    const OWN: &[&str] = &["invalid transition", "guard failed", "memory invariant", "ensure postcondition"];
    if OWN.iter().any(|p| msg.starts_with(p)) {
        msg.to_string()
    } else {
        format!("require failed: {}", msg)
    }
}

impl RuntimeError {
    /// A stable machine-readable kind, exposed as `r.kind` on a try-result.
    pub fn kind(&self) -> String {
        match self {
            RuntimeError::UndefinedVar(_) => "undefined_variable".to_string(),
            RuntimeError::UndefinedFn(_) => "undefined_function".to_string(),
            RuntimeError::NoHandler(..) => "no_handler".to_string(),
            RuntimeError::StackOverflow => "stack_overflow".to_string(),
            RuntimeError::StorageTransaction(_) => "storage".to_string(),
            RuntimeError::Domain { kind, .. } => kind.clone(),
            RuntimeError::RequireFailed(msg) => {
                if msg.starts_with("invalid transition") {
                    "invalid_transition".to_string()
                } else if msg.starts_with("guard failed") {
                    "guard_failed".to_string()
                } else if msg.starts_with("memory invariant") {
                    "invariant".to_string()
                } else if msg.starts_with("ensure postcondition") {
                    "ensure".to_string()
                } else {
                    // `require cond else Tag` → "Tag: constraint violated"
                    msg.split(':').next().unwrap_or("require").trim().to_string()
                }
            }
            RuntimeError::TypeError(msg) => {
                if msg.contains("division by zero") || msg.contains("modulo by zero") {
                    "division_by_zero".to_string()
                } else if msg.starts_with("think()") {
                    "llm".to_string()
                } else if msg.contains("token budget") {
                    "budget".to_string()
                } else if msg.contains("out of bounds") {
                    "index".to_string()
                } else if msg.starts_with("range: ") {
                    // a [native] to_int() past the Int range: same kind as interpreted
                    "range".to_string()
                } else {
                    "type".to_string()
                }
            }
        }
    }

    /// The part after the kind, when the message has the `kind: detail` shape.
    pub fn detail(&self) -> String {
        match self {
            RuntimeError::Domain { kind, message } => {
                message.strip_prefix(&format!("{}: ", kind)).unwrap_or(message).to_string()
            }
            other => other.to_string(),
        }
    }
}

/// Return a human-readable type name for a Value (e.g. "String", "Int").
pub fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::String(_) => "String",
        Value::Bool(_) => "Bool",
        Value::List(_) => "List",
        Value::Map(_) => "Map",
        Value::Lambda { .. } | Value::LambdaBlock { .. } => "Function",
        Value::Variant { .. } => "Variant",
        Value::Unit => "Unit",
    }
}

/// Human-readable name for a BinOp verb (e.g. "add", "subtract").
fn binop_verb(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "subtract",
        BinOp::Mul => "multiply",
        BinOp::Div => "divide",
        BinOp::Mod => "modulo",
        BinOp::And => "logical-and",
        BinOp::Or => "logical-or",
    }
}

/// Convert a byte offset to line:col using source text
/// Spans of an imported file start at `(k + 1) * IMPORT_SPAN_BASE` (k its
/// registration index), so an error in it is reported in THAT file — it
/// used to be reported at the importer's path, on a line past its end.
pub const IMPORT_SPAN_BASE: usize = 1 << 40;

static IMPORT_SOURCES: std::sync::Mutex<Vec<(String, std::sync::Arc<str>)>> = std::sync::Mutex::new(Vec::new());

/// Register an imported file's text; the base to add to its spans.
pub fn register_import_source(file: &str, text: &str) -> usize {
    let mut v = IMPORT_SOURCES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = v.iter().position(|(f, t)| f == file && &**t == text) {
        return (i + 1) * IMPORT_SPAN_BASE;
    }
    v.push((file.to_string(), std::sync::Arc::from(text)));
    v.len() * IMPORT_SPAN_BASE
}

/// The imported file, its text and the local offset of a shifted position.
pub fn resolve_import_pos(pos: usize) -> Option<(String, std::sync::Arc<str>, usize)> {
    if pos < IMPORT_SPAN_BASE { return None; }
    let v = IMPORT_SOURCES.lock().unwrap_or_else(|e| e.into_inner());
    let k = pos / IMPORT_SPAN_BASE;
    v.get(k - 1).map(|(f, t)| (f.clone(), t.clone(), pos % IMPORT_SPAN_BASE))
}

/// `file`, `text`, `pos` of a diagnostic, redirected to the imported file
/// the position belongs to.
pub fn locate_pos<'a>(file: &'a str, text: &'a str, pos: usize) -> (std::borrow::Cow<'a, str>, std::borrow::Cow<'a, str>, usize) {
    match resolve_import_pos(pos) {
        Some((f, t, p)) => (std::borrow::Cow::Owned(f), std::borrow::Cow::Owned(t.to_string()), p),
        None => (std::borrow::Cow::Borrowed(file), std::borrow::Cow::Borrowed(text), pos),
    }
}

pub fn span_to_location(source: &str, offset: usize) -> (usize, usize) {
    if let Some((_, t, p)) = resolve_import_pos(offset) {
        return span_to_location(&t, p);
    }
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.chars().enumerate() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Build a source-context snippet with a caret line pointing at the error position.
/// Returns an empty string if no source is available.
pub fn format_error_context(source: &str, span_start: usize) -> String {
    if let Some((_, t, p)) = resolve_import_pos(span_start) {
        return format_error_context(&t, p);
    }
    let (line_num, col) = span_to_location(source, span_start);
    // Extract the source line
    let full_line = source.split('\n').nth(line_num - 1).unwrap_or("");
    // a 40 KB generated line used to be echoed whole: show a window of it
    // around the caret; control characters (a NUL byte) are shown escaped
    let chars: Vec<char> = full_line.chars().map(|c| if c.is_control() && c != '\t' { '·' } else { c }).collect();
    let caret = col.saturating_sub(1).min(chars.len());
    let (from, to) = (caret.saturating_sub(60), (caret + 60).min(chars.len()));
    let mut line_text: String = chars[from..to].iter().collect();
    if from > 0 { line_text = format!("…{}", line_text); }
    if to < chars.len() { line_text.push('…'); }
    let line_num_str = format!("{}", line_num);
    let gutter_width = line_num_str.len();
    let padding = " ".repeat(gutter_width);
    let caret_offset = " ".repeat(caret - from + if from > 0 { 1 } else { 0 });
    format!(
        "{} |\n{} | {}\n{} | {}^",
        padding, line_num_str, line_text, padding, caret_offset
    )
}

/// Format an error with file location and source context if available.
pub fn format_runtime_error(
    err: &RuntimeError,
    source_file: Option<&str>,
    source_text: Option<&str>,
    span: Option<crate::ast::Span>,
) -> String {
    let (location, context) = match (source_file, source_text, span) {
        (Some(file), Some(text), Some(sp)) => {
            let (file, _, _) = locate_pos(file, text, sp.start);
            let (line, col) = span_to_location(text, sp.start);
            let loc = format!("  --> {}:{}:{}\n", file, line, col);
            let ctx = format_error_context(text, sp.start);
            (loc, ctx)
        }
        (Some(file), _, _) => (format!("  --> {}\n", file), String::new()),
        _ => (String::new(), String::new()),
    };
    if context.is_empty() {
        format!("error: {}\n{}", err, location)
    } else {
        format!("error: {}\n{}{}", err, location, context)
    }
}

/// Compute Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (m, n) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Check if a Value is truthy (false for Bool(false), Unit, Int(0); true otherwise)
/// Collect identifier leaves of an expression. Used to scope a memory
/// invariant to the slots it actually names.
fn collect_expr_idents(expr: &Expr, out: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::Ident(n) => { out.insert(n.clone()); }
        Expr::FieldAccess { target, .. } => collect_expr_idents(&target.node, out),
        Expr::MethodCall { target, args, .. } => {
            collect_expr_idents(&target.node, out);
            for a in args { collect_expr_idents(&a.node, out); }
        }
        Expr::FnCall { args, .. } => {
            for a in args { collect_expr_idents(&a.node, out); }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_expr_idents(&left.node, out);
            collect_expr_idents(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => collect_expr_idents(&i.node, out),
        Expr::ListLiteral(items) => {
            for i in items { collect_expr_idents(&i.node, out); }
        }
        _ => {}
    }
}

/// Result of evaluating one `{...}` interpolation segment.
enum InterpResult {
    Value(Value),
    /// The segment does not parse as an expression — render it as literal text.
    NotAnExpr,
    Err(ExecError),
}

/// A condition's truth (`if`, `while`, if-expressions): a List is refused —
/// `[-5] >= 0` gives the 0/1 mask `[0]`, which read as true.
fn cond_truth(val: &Value) -> Result<bool, ExecError> {
    if let Value::List(_) = val {
        return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
            "a condition got a List ({}) — a comparison on a list gives a 0/1 list per element: use all(...) / any(...)",
            { let t: String = format!("{}", val).chars().take(30).collect(); t }))));
    }
    // a condition is a Bool (an absent value, `()`, is false): the String
    // "false" from a client body read as TRUE in `if body.admin { … }`
    match val {
        Value::Bool(b) => Ok(*b),
        Value::Unit => Ok(false),
        other => Err(ExecError::Runtime(RuntimeError::TypeError(format!(
            "a condition got {} {} — a condition is a Bool: compare it (`x == \"yes\"`, `n > 0`, `xs != []`)",
            value_type_name(other), { let t: String = format!("{}", other).chars().take(30).collect(); t })))),
    }
}

/// An invariant over a List value holds for EVERY element (the 0/1 mask of
/// `[-5, -7] >= 0` is [0, 0], which read as true because it is non-empty).
fn invariant_holds(val: &Value) -> bool {
    match val {
        Value::List(mask) => mask.iter().all(is_truthy),
        // a condition is a Bool: `invariant vals` accepted 5 and refused 0
        Value::Bool(b) => *b,
        _ => false,
    }
}

pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Unit => false,
        Value::Int(si) => si.to_i64() != Some(0),
        Value::Float(f) => *f != 0.0 && !f.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::List(l) => !l.is_empty(),
        Value::Map(m) => !m.is_empty(),
        _ => true,
    }
}

/// Runtime values
#[derive(Debug, Clone)]
pub enum Value {
    Int(SomaInt),
    Float(f64),
    String(String),
    Bool(bool),
    List(Vec<Value>),
    Map(IndexMap<String, Value>),
    /// Lambda: captured param name + body expression + closure environment
    Lambda {
        param: std::string::String,
        body: Box<Spanned<Expr>>,
        env: HashMap<std::string::String, Value>,
    },
    /// Block lambda: with statements before result
    LambdaBlock {
        param: std::string::String,
        stmts: Vec<Spanned<Statement>>,
        result: Box<Spanned<Expr>>,
        env: HashMap<std::string::String, Value>,
    },
    /// Sum-type variant.  `type_name` records which `cell type` it
    /// belongs to (needed for equality, exhaustiveness, and serialization).
    Variant {
        type_name: String,
        variant: String,
        fields: VariantValue,
    },
    Unit,
}

/// Payload of a sum-type variant value.
#[derive(Debug, Clone)]
pub enum VariantValue {
    Unit,
    Tuple(Vec<Value>),
    Struct(IndexMap<String, Value>),
}

/// Build a Value::Map from a Vec of (String, Value) pairs.
/// Preserves insertion order; duplicate keys overwrite.
pub fn map_from_pairs(pairs: Vec<(String, Value)>) -> Value {
    Value::Map(pairs.into_iter().collect())
}

/// Proper JSON string escaping per RFC 8259 §7. Escapes the structural
/// characters (`"`, `\`) plus all control characters U+0000–U+001F so
/// downstream JSON parsers don't choke on multi-line strings emitted
/// by `to_json` (e.g. LLM output that contains newlines).
fn json_escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(si) => write!(f, "{}", si),
            Value::Float(n) => {
                // shortest round-trip digits (`{:.1}` wrote the exact binary
                // expansion: 6.02214076e23 → 602214075999999987023872.0)
                let t = format!("{}", n);
                if n.is_finite() && !t.contains('.') { write!(f, "{}.0", t) } else { write!(f, "{}", t) }
            }
            Value::String(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    match item {
                        Value::String(s) => {
                            write!(f, "\"{}\"", json_escape_str(s))?
                        }
                        other => write!(f, "{}", other)?,
                    }
                }
                write!(f, "]")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().filter(|(k, v)| !(k.as_str() == "_response" && matches!(v, Value::Lambda { param, .. } if param == HTTP_MARK))).enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": ", json_escape_str(k))?;
                    match v {
                        Value::String(s) => {
                            write!(f, "\"{}\"", json_escape_str(s))?
                        }
                        other => write!(f, "{}", other)?,
                    }
                }
                write!(f, "}}")
            }
            Value::Lambda { param, .. } => write!(f, "<lambda({})>", param),
            Value::LambdaBlock { param, .. } => write!(f, "<lambda({})>", param),
            Value::Variant { variant, fields, .. } => match fields {
                VariantValue::Unit => write!(f, "{}", variant),
                VariantValue::Tuple(vs) => {
                    write!(f, "{}(", variant)?;
                    for (i, v) in vs.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", v)?;
                    }
                    write!(f, ")")
                }
                VariantValue::Struct(entries) => {
                    write!(f, "{} {{", variant)?;
                    for (i, (k, v)) in entries.iter().enumerate() {
                        if i > 0 { write!(f, ",")?; }
                        write!(f, " {}: {}", k, v)?;
                    }
                    write!(f, " }}")
                }
            },
            Value::Unit => write!(f, "null"),
        }
    }
}

impl Value {
    pub fn as_int(&self) -> Result<i64, RuntimeError> {
        match self {
            Value::Int(si) => si.to_i64().ok_or_else(|| RuntimeError::TypeError("BigInt too large for i64".to_string())),
            other => Err(RuntimeError::TypeError(format!("expected Int, got {} {}", value_type_name(other), short_value(other)))),
        }
    }

    pub fn as_soma_int(&self) -> Option<SomaInt> {
        if let Value::Int(si) = self { Some(si.clone()) } else { None }
    }

    fn as_float(&self) -> Result<f64, RuntimeError> {
        match self {
            Value::Float(n) => Ok(*n),
            Value::Int(si) => Ok(si.to_f64()),
            other => Err(RuntimeError::TypeError(format!("expected Float, got {} {}", value_type_name(other), short_value(other)))),
        }
    }

    fn as_bool(&self) -> Result<bool, RuntimeError> {
        match self {
            Value::Bool(b) => Ok(*b),
            // an absent value is false wherever a condition is read (`if`
            // took it, `!()`, `() && x`, `require ()` and guards raised)
            Value::Unit => Ok(false),
            // `!0` used to be `true`: a condition is a Bool, as in `if`
            other => Err(RuntimeError::TypeError(format!("expected Bool, got {} {} — compare it: `x == 0`, `s == \"\"`, `xs == []`", value_type_name(other), short_value(other)))),
        }
    }

    /// Check if this is a big (non-inline) integer
    pub fn is_big(&self) -> bool {
        if let Value::Int(si) = self { !si.is_small() } else { false }
    }
}

/// Signal that a return statement was hit
#[derive(Debug)]
struct ReturnSignal(Value);

/// Tree-walking interpreter for Soma programs
/// Pre-computed handler lookup: (cell_name, signal_name) → (params, body)
type HandlerKey = (String, String);
type HandlerValue = (Arc<Vec<Param>>, Arc<Vec<Spanned<Statement>>>);

/// Check whether a slice of statements contains any `let` bindings (used to
/// decide whether we need full scoping overhead in exec_body_scoped).
fn body_has_let(body: &[Spanned<Statement>]) -> bool {
    body.iter().any(|s| matches!(s.node, Statement::Let { .. }))
}

/// A broadcast event emitted by `emit` — sent to all SSE clients and connected cells
#[derive(Debug, Clone)]
pub struct BusEvent {
    pub stream: String,
    pub data: Value,
    /// an `emit` (cell-to-cell): delivered to SSE clients that NAME it, never
    /// to every WebSocket client (an internal `emit secret_hand(…)` reached
    /// an unauthenticated WS client); `publish` goes to both
    pub internal: bool,
}

/// Shared broadcast bus for real-time event distribution
/// Bounded per-client queues: a client that stops reading is dropped once
/// its queue is full (a non-reading SSE client grew the server to 394 MB).
pub type EventBus = Arc<std::sync::Mutex<Vec<std::sync::mpsc::SyncSender<BusEvent>>>>;
pub const BUS_QUEUE: usize = 1024;

/// This process's bus port and a random nonce: an outbound [peers] link
/// opens with `HELLO <bus_port> <nonce>` so the receiving side knows who
/// it is (a two-way [peers] pair delivered every event twice; a peer
/// address that was this very process looped).
pub static OWN_BUS_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
pub fn process_nonce() -> u64 {
    static N: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_u32(std::process::id());
        h.finish()
    })
}
/// The resolved addresses of this process's own [peers] (set by serve)
pub static PEER_ADDRS: std::sync::OnceLock<Vec<std::net::SocketAddr>> = std::sync::OnceLock::new();

/// Writer side of a bus link: sends queued lines until the link is over —
/// the peer closed (`alive` cleared by the reader), a write failed, or the
/// queue was dropped — then closes the socket. It polls `alive`: a writer
/// blocked on an empty queue kept its thread and socket forever after the
/// peer left (8 000 connect/close cycles = 8 000 threads, fds in CLOSE_WAIT).
pub fn bus_writer_loop(rx: std::sync::mpsc::Receiver<String>, mut stream: std::net::TcpStream, alive: Arc<std::sync::atomic::AtomicBool>) {
    use std::io::Write;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(line) => {
                if !alive.load(std::sync::atomic::Ordering::SeqCst) || stream.write_all(line.as_bytes()).is_err() || stream.flush().is_err() {
                    eprintln!("bus: event '{}' NOT delivered to a peer that disconnected", line.split_whitespace().nth(1).unwrap_or("?"));
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if !alive.load(std::sync::atomic::Ordering::SeqCst) { break; }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    alive.store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// TCP peer connections for inter-process signal bus
pub type PeerBus = Arc<std::sync::Mutex<Vec<std::sync::mpsc::SyncSender<String>>>>;

pub fn new_event_bus() -> EventBus {
    Arc::new(std::sync::Mutex::new(Vec::new()))
}

/// Send a bus line to every peer. A peer whose queue is full (it stopped
/// reading, or hung without closing) is DROPPED, as a WebSocket client is:
/// an unbounded queue grew the emitter to gigabytes.
/// Env marker naming the slot whose invariant is being evaluated (not a
/// valid identifier, so no program can bind or read it)
const INV_SLOT: &str = "#invariant_slot";

/// Each configured `[peers]` peer and whether its link is up (the
/// supervisor in serve sets it): an event emitted while one is down is
/// reported NOT delivered to it, even when other links carry it (the report
/// used to depend on ANY link being connected — 50 alerts lost, 3 logged).
pub static PEER_UP: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<String, bool>>> = std::sync::OnceLock::new();

pub fn set_peer_up(name: &str, up: bool) {
    PEER_UP.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), up);
}

pub fn send_to_peers(senders: &mut Vec<std::sync::mpsc::SyncSender<String>>, line: &str) {
    if !senders.is_empty() {
        if let Some(m) = PEER_UP.get() {
            let down: Vec<String> = m.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(_, up)| !**up).map(|(n, _)| n.clone()).collect();
            if !down.is_empty() {
                let name = line.split_whitespace().nth(1).unwrap_or("?");
                eprintln!("bus: event '{}' NOT delivered to peer {} (link down) — fire-and-forget: reconcile, or retry from an outbox", name, down.join(", "));
            }
        }
    }
    // [peers] are configured but none is connected (a peer down): the event
    // is not delivered — say so (points were debited here, never credited
    // there, and the log showed only `POST /transfer → 200`)
    if senders.is_empty() && crate::commands::HAS_PEERS.load(std::sync::atomic::Ordering::Relaxed) {
        let name = line.split_whitespace().nth(1).unwrap_or("?");
        eprintln!("bus: event '{}' NOT delivered — no peer is connected (fire-and-forget: reconcile, or retry from an outbox)", name);
        return;
    }
    senders.retain(|s| match s.try_send(line.to_string()) {
        Ok(()) => true,
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            eprintln!("bus: a peer stopped reading ({} events queued) — disconnected", BUS_QUEUE);
            false
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            eprintln!("bus: event '{}' NOT delivered to a peer that disconnected", line.split_whitespace().nth(1).unwrap_or("?"));
            false
        }
    });
}

pub fn new_peer_bus() -> PeerBus {
    Arc::new(std::sync::Mutex::new(Vec::new()))
}

pub struct Interpreter {
    /// All cells in the program, by name
    pub(crate) cells: HashMap<String, CellDef>,
    /// cell names in declaration order (dispatch of a bare call is deterministic)
    pub(crate) cell_order: Vec<String>,
    /// Pre-computed handler lookup — avoids scanning sections on every call
    handler_cache: HashMap<HandlerKey, HandlerValue>,
    /// handler name → the parameter counts it is defined with (any cell).
    /// Drives call resolution: `f(a, b)` is the user's handler when one
    /// takes 2 arguments, the builtin `f` otherwise. User definitions
    /// shadow the library — and adding a builtin can never hijack a program.
    handler_arities: HashMap<String, Vec<usize>>,
    /// Maximum recursion depth
    max_depth: usize,
    pub(crate) current_depth: usize,
    /// Signals emitted during execution (collected for runtime dispatch)
    emitted_signals: Vec<(String, Vec<Value>)>,
    /// Storage backends for memory slots, keyed by "cell_name.slot_name"
    pub(crate) storage: HashMap<String, Arc<dyn StorageBackend>>,
    /// State machines: (cell_name, machine_name) → definition
    pub(crate) state_machines: HashMap<(String, String), StateMachineSection>,
    /// Broadcast bus for SSE/real-time events
    pub event_bus: Option<EventBus>,
    /// Peer bus for inter-process signal delivery
    pub peer_bus: Option<PeerBus>,
    /// WebSocket client outgoing channel (for ws_send)
    pub ws_out: Option<Arc<std::sync::Mutex<std::sync::mpsc::Sender<String>>>>,
    /// Source file path for error reporting
    pub source_file: Option<String>,
    /// Source text for line:col conversion
    pub source_text: Option<String>,
    /// Last known span (set before eval, used for error reporting)
    pub last_span: Option<crate::ast::Span>,
    /// Cached handler for the current recursive call (avoids repeated HashMap lookups)
    current_handler: Option<(String, String, Arc<Vec<Param>>, Arc<Vec<Spanned<Statement>>>)>,
    /// Locals of the handler at its `transition()` call, handed to the
    /// transition's guard: `a -> b { guard { amount < 10000 } }` reads the
    /// caller's `amount`.
    pub(crate) transition_env: Option<Env>,
    /// Undo journal of the running top-level handler. Every committed
    /// write / delete / append / state transition records how to undo
    /// itself; a handler that fails is rolled back entirely, and a
    /// `try { }` that fails is rolled back to where it started (savepoint).
    /// None outside a top-level invocation.
    pub(crate) journal: Option<Vec<UndoOp>>,
    /// Scripted LLM replies (`mock think …` in a test cell): Ok(text) or
    /// Err(message). think() consumes this queue before any mock mode.
    pub mock_queue: std::collections::VecDeque<Result<String, String>>,
    /// Scripted answers for approve() (`mock approve false`).
    pub approve_queue: std::collections::VecDeque<bool>,
    /// Under `soma test` with no key and no mock configured, think() is
    /// mocked (echo) instead of failing on the network.
    pub test_auto_mock: bool,
    /// `mock <handler> …` in a test cell: handler name → queued answers
    /// (Ok = value returned, Err = message raised) consumed one per call.
    pub handler_stubs: HashMap<String, std::collections::VecDeque<Result<Value, String>>>,
    /// a real network call under `soma test` was already reported
    pub(crate) net_noted: bool,
    /// writes committed by the last top-level invocation
    pub last_commit_writes: usize,
    /// `mock now <unix seconds>`: the clock the builtins answer with.
    /// `mock now` in a test: the frozen clock, in MILLISECONDS
    pub frozen_now: Option<i64>,
    pub(crate) auto_mock_noted: bool,
    /// V1.6: tool-capability scope. Set when the LLM dispatches into a tool
    /// with declared capabilities; the http/* builtins consult it.
    pub(crate) current_tool_caps: Option<Vec<String>>,
    /// the scopes of the tools this one runs inside: a nested agent's tool
    /// answers to every one of them (its own unscoped tool reset the scope)
    pub(crate) outer_tool_caps: Vec<Vec<String>>,
    /// `map("tools_allowed", [...])` of the running think(): the only tools
    /// offered to (and dispatched for) the model
    pub(crate) think_tools_allowed: Option<Vec<String>>,
    /// > 0 while a handler runs as a model's tool call
    pub(crate) tool_depth: u32,
    /// the open step of a top-level `[task]` handler (think() ends it)
    pub(crate) task_unit: Option<Unit>,
    transaction_failure: Option<String>,
    /// incremented at each unit: a `try` savepoint from an earlier step
    pub(crate) journal_gen: u64,
    /// set by do_connect: false once that peer link is gone
    pub last_link_alive: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// the `cell test` whose rules are running (its helpers win bare calls)
    pub current_test_cell: Option<String>,
    /// Loaded [native] handler FFI function pointers, keyed by (cell_name, signal_name)
    pub native_handlers: HashMap<(String, String), native_ffi::LoadedNative>,
    /// Cluster node for distributed storage (None = standalone mode)
    pub cluster: Option<Arc<crate::runtime::cluster::ClusterNode>>,
    /// Which memory slots are sharded (slot_name → true)
    pub sharded_slots: HashMap<String, bool>,
    /// Memory invariants: slot_name → list of expressions to check on .set()
    pub(crate) invariants: HashMap<String, Vec<Expr>>,
    // ── Agent-specific state ────────────────────────────────────────
    /// Token usage tracking: total tokens consumed by think() calls
    pub(crate) agent_tokens_used: i64,
    /// Token budget: max tokens allowed (0 = unlimited)
    pub(crate) agent_token_budget: i64,
    /// set_budget(0): no more model calls (0 in agent_token_budget means none set)
    pub(crate) agent_budget_zero: bool,
    /// `transition()` in progress: (instance id, target state) — the `status`
    /// binding of an invariant reads the state the machine is MOVING to
    pub(crate) pending_status: Option<(String, String)>,
    /// one multi-turn LLM context per agent cell
    pub(crate) agent_conversations: std::collections::HashMap<String, Vec<serde_json::Value>>,
    /// provider rounds allowed for the current think() (max_rounds, ≤ 10)
    pub(crate) think_rounds: usize,
    /// Conversation history for multi-turn think() within a handler
    pub(crate) agent_conversation: Vec<serde_json::Value>,
    /// Structured trace log: every think, tool call, transition, delegate
    pub(crate) agent_trace: Vec<Value>,
    /// Pending approval gates (for human-in-the-loop)
    pub(crate) agent_pending_approval: Option<String>,
    /// Agent LLM config (from soma.toml [agent] section)
    pub agent_config: Option<crate::pkg::manifest::AgentConfig>,
    /// The budget of the horde this interpreter runs a task of: every
    /// think() reserves against it before calling the model.
    pub(crate) horde_budget: Option<Arc<horde::Budget>>,
    /// the agent instance a horde task runs as (`instance` option): its
    /// remember() / recall() keys are its own
    pub(crate) horde_instance: Option<String>,
    /// the horde this interpreter runs a task / callback of
    pub(crate) horde_current: Option<String>,
    pub(crate) horde_depth: usize,
    /// running a horde's apply / on_done (a horde started there is the next
    /// round, not a nested one)
    pub(crate) horde_closing: bool,
    /// hordes to run synchronously once the current unit has committed
    /// (no workers: `soma test`), and the guard against running them nested
    pub(crate) deferred_hordes: Vec<(horde::Spec, Vec<Value>)>,
    pub(crate) running_deferred: bool,
    /// Named model configs (from soma.toml [models.*] sections)
    pub agent_models: std::collections::HashMap<String, crate::pkg::manifest::AgentConfig>,
    // ── V1: record/replay ───────────────────────────────────────────
    /// Set of (cell, handler) pairs that should be recorded
    pub(crate) record_handlers: std::collections::HashSet<(String, String)>,
    /// Path to the .somalog file for the current run (None = no recording)
    pub record_log_path: Option<std::path::PathBuf>,
    /// Names of nondeterministic builtins called during the current handler.
    /// Reset at the start of each [record] handler call.
    pub(crate) record_nondet_called: Vec<String>,
    /// Replay mode: when true, [record] handlers do NOT append to the log;
    /// instead the runner compares results against pre-loaded entries.
    pub replay_mode: bool,
    /// Sum-type variant registry: variant name → (type name, fields shape).
    /// Built once at construction from `cell type Foo { variants { … } }` definitions.
    pub(crate) variant_registry: HashMap<String, (String, VariantShape)>,
    /// Per-type variant list: type name → ordered Vec of variant names.
    /// Used by the exhaustiveness checker.
    pub(crate) type_variants: HashMap<String, Vec<String>>,
    /// (type, variant) → declared payload: shape and field types
    pub(crate) variant_fields: HashMap<(String, String), VariantFields>,
}

/// What kind of payload a registered variant takes.
#[derive(Debug, Clone)]
pub enum VariantShape {
    Unit,
    Tuple(usize),         // arity
    Struct(Vec<String>),  // field names
}

/// The provenance mark of an HTTP response map (`response()`, `html()`,
/// `redirect()`, `sse()`): a value no JSON body, query string, header or
/// stored slot can produce — a map a CLIENT sent with `_status` / `_body`
/// / header keys used to become a raw HTTP response (stored XSS, forged
/// cookies, open redirects).
pub const HTTP_MARK: &str = "__soma_http_response__";

pub fn http_marker() -> Value {
    Value::Lambda {
        param: HTTP_MARK.to_string(),
        body: Box::new(Spanned::new(Expr::Literal(Literal::Unit), crate::ast::Span { start: 0, end: 0 })),
        env: HashMap::new(),
    }
}

pub fn is_http_response(v: &Value) -> bool {
    matches!(v, Value::Map(m) if matches!(m.get("_response"), Some(Value::Lambda { param, .. }) if param == HTTP_MARK))
}

/// Handlers some `emit` of this program targets (set by `soma serve`).
/// The most elements (characters, list items, matrix cells) one builtin call
/// builds: a single absurd size in a request aborted the whole process
/// ("memory allocation of 4611686018427387903 bytes failed").
pub const MAX_BUILT_LEN: usize = 100_000_000;
/// A materialized List (range(0, n)): ~115 bytes per element, so 10M is ~1 GB
/// — one `GET /biglist/20000000` took the server from 126 MB to 2.3 GB.
/// `for i in range(a, b)` is lazy and not limited.
pub const MAX_LIST_LEN: usize = 10_000_000;

pub static EVENT_LISTENERS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
/// soma.toml `[bus] accept`: events other processes may send.
pub static BUS_ACCEPT: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// May an event arriving from ANOTHER process run here? Only an event this
/// program emits or `[bus] accept` lists; never a private `_` handler, the
/// router, a start-up hook or `ws`; never data forging a record / variant.
pub fn bus_event_allowed(name: &str, data: Option<&serde_json::Value>) -> Result<(), String> {
    if name.starts_with('_') || matches!(name, "request" | "ws" | "start" | "init") {
        return Err(format!("refused event '{}' (a private or reserved handler)", name));
    }
    let accepted = EVENT_LISTENERS.get().map_or(false, |e| e.contains(name))
        || BUS_ACCEPT.get().map_or(false, |a| a.iter().any(|x| x == name));
    if !accepted {
        return Err(format!("refused event '{}' — not emitted by this program nor listed in soma.toml [bus] accept", name));
    }
    fn forged(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(m) => m.contains_key("_type") || m.contains_key("_variant") || m.contains_key("_values") || m.values().any(forged),
            serde_json::Value::Array(xs) => xs.iter().any(forged),
            _ => false,
        }
    }
    if data.map_or(false, forged) {
        return Err(format!("refused event '{}' (its data carries _type / _variant)", name));
    }
    if data.map_or(false, builtins::string::json_has_inf) {
        return Err(format!("refused event '{}' (a number beyond the Float range)", name));
    }
    Ok(())
}

/// The process's `[agent]` / `[models]` config (set by `soma serve`): the
/// default of every interpreter it creates.
pub static DEFAULT_AGENT: std::sync::OnceLock<(Option<crate::pkg::manifest::AgentConfig>, std::collections::HashMap<String, crate::pkg::manifest::AgentConfig>)> = std::sync::OnceLock::new();

/// Set by `soma run` and `soma serve`: state-machine instances live in
/// .soma_data/soma.db whatever the slots of the program.
pub static PERSIST_MACHINES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Interpreter {
    pub fn new(program: &Program) -> Self {
        let mut cells = HashMap::new();
        let cell_order: Vec<String> = program.cells.iter().map(|c| c.node.name.clone()).collect();
        let mut handler_cache = HashMap::new();
        let mut handler_arities: HashMap<String, Vec<usize>> = HashMap::new();
        let mut state_machines = HashMap::new();
        let mut record_handlers: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        for cell in &program.cells {
            cells.insert(cell.node.name.clone(), cell.node.clone());
            for section in &cell.node.sections {
                if let Section::OnSignal(ref on) = section.node {
                    let key = (cell.node.name.clone(), on.signal_name.clone());
                    if handler_cache.contains_key(&key) {
                        eprintln!("warning: duplicate handler '{}' in cell '{}' (last definition wins)", on.signal_name, cell.node.name);
                    }
                    let value = (Arc::new(on.params.clone()), Arc::new(on.body.clone()));
                    handler_cache.insert(key, value);
                    handler_arities.entry(on.signal_name.clone()).or_default().extend(crate::ast::accepted_arities(&on.params));
                    if on.properties.iter().any(|p| p == "record") {
                        record_handlers.insert((cell.node.name.clone(), on.signal_name.clone()));
                    }
                }
                if let Section::State(ref sm) = section.node {
                    state_machines.insert(
                        (cell.node.name.clone(), sm.name.clone()),
                        sm.clone(),
                    );
                }
            }
        }
        // Build the variant registry from any `cell type Foo` with a
        // `variants { … }` section.  Variant names must be unique within
        // a single program; collisions later in this loop overwrite
        // silently (the checker pass reports a friendlier error).
        let mut variant_registry: HashMap<String, (String, VariantShape)> = HashMap::new();
        let mut type_variants: HashMap<String, Vec<String>> = HashMap::new();
        let mut variant_fields: HashMap<(String, String), VariantFields> = HashMap::new();
        for cell in &program.cells {
            if !matches!(cell.node.kind, CellKind::Type) {
                continue;
            }
            for section in &cell.node.sections {
                if let Section::Variants(ref vs) = section.node {
                    let mut names = Vec::with_capacity(vs.variants.len());
                    for vd in &vs.variants {
                        let shape = match &vd.node.fields {
                            VariantFields::Unit => VariantShape::Unit,
                            VariantFields::Tuple(ts) => VariantShape::Tuple(ts.len()),
                            VariantFields::Struct(fs) => {
                                VariantShape::Struct(fs.iter().map(|(n, _)| n.clone()).collect())
                            }
                        };
                        variant_registry.insert(
                            vd.node.name.clone(),
                            (cell.node.name.clone(), shape),
                        );
                        names.push(vd.node.name.clone());
                        variant_fields.insert((cell.node.name.clone(), vd.node.name.clone()), vd.node.fields.clone());
                    }
                    type_variants.insert(cell.node.name.clone(), names);
                }
            }
        }
        // Collect memory invariants from all cells. An invariant that
        // names specific slots guards only those slots; one that uses
        // only the generic bindings (value/key/size) guards every slot
        // in its section.
        let mut invariants: HashMap<String, Vec<Expr>> = HashMap::new();
        for cell in &program.cells {
            for section in &cell.node.sections {
                if let Section::Memory(ref mem) = section.node {
                    let slot_names: Vec<&str> =
                        mem.slots.iter().map(|s| s.node.name.as_str()).collect();
                    for inv in &mem.invariants {
                        // deep, as the checker counts them: a slot named in a
                        // lambda (`all([0], y => a >= 0)`) guards `a` only
                        let refs = crate::checker::invariants::deep_idents(&inv.node);
                        let named: Vec<&str> = slot_names.iter()
                            .filter(|n| refs.contains(**n))
                            .copied()
                            .collect();
                        let targets: &[&str] = if named.is_empty() { &slot_names } else { &named };
                        // `links.size <= N` → the generic `size`, scoped above to `links`
                        for t in targets {
                            // normalized for THIS target: `t.size` is the
                            // written slot's size, another slot's `.size`
                            // keeps its name (it is that slot's entry count,
                            // bound at check time — it used to be stripped to
                            // the same `size` and the rule was vacuous)
                            let inv_expr = crate::checker::invariants::normalize_invariant(&inv.node, &[t.to_string()]);
                            let key = format!("{}.{}", cell.node.name, t);
                            invariants.entry(key).or_default().push(inv_expr.clone());
                            invariants.entry((*t).to_string()).or_default().push(inv_expr);
                        }
                    }
                }
            }
        }
        Self {
            cells,
            handler_cache,
            cell_order,
            handler_arities,
            max_depth: 512,
            current_depth: 0,
            emitted_signals: Vec::new(),
            storage: HashMap::new(),
            state_machines,
            event_bus: None,
            peer_bus: None,
            ws_out: None,
            source_file: None,
            source_text: None,
            last_span: None,
            current_handler: None,
            transition_env: None,
            journal: None,
            mock_queue: std::collections::VecDeque::new(),
            approve_queue: std::collections::VecDeque::new(),
            test_auto_mock: false,
            handler_stubs: HashMap::new(),
            net_noted: false,
            last_commit_writes: 0,
            frozen_now: None,
            auto_mock_noted: false,
            current_tool_caps: None,
            outer_tool_caps: Vec::new(),
            think_tools_allowed: None,
            tool_depth: 0,
            task_unit: None,
            transaction_failure: None,
            journal_gen: 0,
            last_link_alive: None,
            current_test_cell: None,
            native_handlers: HashMap::new(),
            cluster: None,
            sharded_slots: HashMap::new(),
            invariants,
            agent_tokens_used: 0,
            agent_token_budget: 0,
            agent_budget_zero: false,
            pending_status: None,
            think_rounds: 10,
            agent_conversation: Vec::new(),
            agent_conversations: std::collections::HashMap::new(),
            agent_trace: Vec::new(),
            agent_pending_approval: None,
            agent_config: DEFAULT_AGENT.get().and_then(|d| d.0.clone()),
            horde_budget: None,
            horde_instance: None,
            horde_current: None,
            horde_depth: 0,
            horde_closing: false,
            deferred_hordes: Vec::new(),
            running_deferred: false,
            agent_models: DEFAULT_AGENT.get().map(|d| d.1.clone()).unwrap_or_default(),
            record_handlers,
            record_log_path: None,
            record_nondet_called: Vec::new(),
            replay_mode: false,
            variant_registry,
            type_variants,
            variant_fields,
        }
    }

    /// Register an additional cell definition (used by runtime to inject interior cells)
    pub fn register_cell(&mut self, cell: CellDef) {
        // Update handler cache
        for section in &cell.sections {
            if let Section::OnSignal(ref on) = section.node {
                let key = (cell.name.clone(), on.signal_name.clone());
                let value = (Arc::new(on.params.clone()), Arc::new(on.body.clone()));
                self.handler_cache.insert(key, value);
                self.handler_arities.entry(on.signal_name.clone()).or_default().extend(crate::ast::accepted_arities(&on.params));
            }
        }
        self.cells.insert(cell.name.clone(), cell);
    }

    /// Inject storage backends for memory slots
    pub fn set_storage(&mut self, cell_name: &str, slots: &HashMap<String, Arc<dyn StorageBackend>>) {
        for (slot_name, backend) in slots {
            let key = format!("{}.{}", cell_name, slot_name);
            self.storage.insert(key, backend.clone());
        }
        // Also register without cell prefix for direct access in handlers
        for (slot_name, backend) in slots {
            self.storage.insert(slot_name.clone(), backend.clone());
        }
    }

    /// Inject pre-keyed storage backends (keys already include cell prefix)
    pub fn set_storage_raw(&mut self, slots: &HashMap<String, Arc<dyn StorageBackend>>) {
        for (key, backend) in slots {
            self.storage.insert(key.clone(), backend.clone());
        }
    }

    /// Configure cluster mode — enables distributed storage operations
    pub fn set_cluster(&mut self, cluster: Arc<crate::runtime::cluster::ClusterNode>, sharded: &HashMap<String, bool>) {
        self.cluster = Some(cluster);
        self.sharded_slots = sharded.clone();
    }

    /// Execute an `every` block's body
    pub fn exec_every(&mut self, body: &[Spanned<Statement>], env: &mut Env, cell_name: &str) -> Result<Value, RuntimeError> {
        self.exec_tick(body, env, cell_name, false)
    }

    /// Run a tick's body: one atomic unit, or `[task]` steps
    /// (`every 5s [task] { … }`) whose think()s wait outside the lock.
    pub fn exec_tick(&mut self, body: &[Spanned<Statement>], env: &mut Env, cell_name: &str, task: bool) -> Result<Value, RuntimeError> {
        if task && self.journal.is_none() && self.task_unit.is_none() {
            return self.run_task(|s| match s.exec_body(body, env, cell_name, "_every") {
                Ok(v) | Err(ExecError::Return(v)) => Ok(v),
                Err(ExecError::Break) => Err(RuntimeError::TypeError("break outside of loop".to_string())),
                Err(ExecError::Continue) => Err(RuntimeError::TypeError("continue outside of loop".to_string())),
                Err(ExecError::Runtime(e)) => Err(e),
            });
        }
        // a tick is a handler invocation: serialized with requests AND
        // rolled back when it raises (its writes used to stay committed)
        let outcome = self.atomically(|s| match s.exec_body(body, env, cell_name, "_every") {
            Ok(v) | Err(ExecError::Return(v)) => Ok(v),
            Err(e) => Err(e),
        });
        match outcome {
            Ok(val) => Ok(val),
            Err(ExecError::Return(val)) => Ok(val),
            Err(ExecError::Break) => {
                Err(RuntimeError::TypeError("break outside of loop".to_string()))
            }
            Err(ExecError::Continue) => {
                Err(RuntimeError::TypeError("continue outside of loop".to_string()))
            }
            // the caller logs it once (`[scheduler:Cell] tick error: …`)
            Err(ExecError::Runtime(e)) => Err(e),
        }
    }

    /// Ensure state machine storage slots exist
    /// Uses persistent backend (SQLite) if any existing slot is persistent, otherwise memory
    pub fn ensure_state_machine_storage(&mut self) {
        // `soma run` / `soma serve` persist machine instances even when the
        // program has no [persistent] slot (they were kept in memory then:
        // every transition forgotten when the handler returned)
        let has_persistent = PERSIST_MACHINES.load(std::sync::atomic::Ordering::Relaxed)
            || self.storage.values().any(|b| b.backend_name() == "sqlite" || b.backend_name() == "file");
        for ((cell_name, sm_name), _) in self.state_machines.clone() {
            // Use cell-scoped key to prevent collisions between agents
            let key = format!("__sm_{}_{}", cell_name, sm_name);
            let legacy_key = format!("__sm_{}", sm_name);
            if !self.storage.contains_key(&key) && !self.storage.contains_key(&legacy_key) {
                let backend: Arc<dyn crate::runtime::storage::StorageBackend> = if has_persistent {
                    Arc::new(crate::runtime::storage::SqliteBackend::new(&cell_name, &format!("_sm_{}", sm_name)))
                } else {
                    Arc::new(crate::runtime::storage::MemoryBackend::new())
                };
                self.storage.insert(key, backend);
            }
        }
    }

    /// Start-up audit of persistent data against the CURRENT program:
    /// instances stored in a state the machine no longer declares, and
    /// slot values that violate an invariant added since. Nothing is
    /// changed; each problem is one line for the operator.
    pub fn audit_stored_data(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        // state-machine instances
        for ((cell_name, sm_name), sm) in self.state_machines.clone() {
            let key = format!("__sm_{}_{}", cell_name, sm_name);
            let Some(backend) = self.storage.get(&key).cloned() else { continue };
            let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();
            declared.insert(sm.initial.clone());
            for t in &sm.transitions {
                if t.node.from != "*" { declared.insert(t.node.from.clone()); }
                declared.insert(t.node.to.clone());
            }
            let mut stuck = 0usize;
            let mut sample = String::new();
            for id in backend.keys() {
                if let Some(StoredValue::String(st)) = backend.get(&id) {
                    if !declared.contains(&st) {
                        stuck += 1;
                        if sample.is_empty() { sample = format!("'{}' in '{}'", id, st); }
                    }
                }
            }
            if stuck > 0 {
                out.push(format!("{} instance(s) of state machine '{}' are stored in a state the program no longer declares (e.g. {}): they can take no transition, not even a `*` edge — move them with a one-shot program that still declares the old state and an edge out of it (`soma run migrate.cell _migrate`), or delete .soma_data/", stuck, sm_name, sample));
            }
        }
        // slot invariants over stored values
        let mut invs: Vec<(String, Vec<Expr>)> = self.invariants.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        invs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut seen_slots: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (key, exprs) in invs {
            let slot_only = key.rsplit('.').next().unwrap_or(&key).to_string();
            if !seen_slots.insert(slot_only) { continue; }
            let (cell_name, slot_name) = match key.split_once('.') { Some((c, s)) => (c.to_string(), s.to_string()), None => (String::new(), key.clone()) };
            let Some(backend) = self.storage.get(&key).or_else(|| self.storage.get(&slot_name)).cloned() else { continue };
            let mut bad = 0usize;
            let mut sample = String::new();
            // counted ONCE (per key it made the start-up audit quadratic:
            // 21 s to start a 50k-policy book)
            let size = backend.len() as i64;
            for k in backend.keys().into_iter().take(10_000) {
                let Some(stored) = backend.get(&k) else { continue };
                let val = self.from_slot(&cell_name, &slot_name, stored_to_value(stored));
                if self.check_invariants(&cell_name, &slot_name, &k, &val, size, "read").is_err() {
                    bad += 1;
                    if sample.is_empty() { sample = format!("key \"{}\" = {}", k, val); }
                }
            }
            if bad > 0 {
                let _ = exprs;
                out.push(format!("{} stored value(s) of slot '{}' violate its invariant today (e.g. {}): verify proves the invariant for future writes only — fix the data or the invariant", bad, slot_name, sample));
            }
        }
        // stored values of another TYPE than the slot declares (a slot
        // re-typed `Map<String, Int>` → `Map<String, Map>`: the old Ints are
        // read back as Ints and crash the handlers that expect records)
        let mut keys: Vec<String> = self.storage.keys().filter(|k| k.contains('.') && !k.starts_with("__")).cloned().collect();
        keys.sort();
        for key in keys {
            let (cell_name, slot_name) = key.split_once('.').map(|(c, s)| (c.to_string(), s.to_string())).unwrap();
            if self.slot_value_type(&cell_name, &slot_name).is_none() { continue; }
            let Some(backend) = self.storage.get(&key).cloned() else { continue };
            let values: Vec<(String, Value)> = if self.slot_kind(&cell_name, &slot_name) == Some("List") {
                backend.list().into_iter().take(10_000).enumerate().map(|(i, v)| (format!("#{}", i), self.from_slot(&cell_name, &slot_name, stored_to_value(v)))).collect()
            } else {
                backend.keys().into_iter().take(10_000).filter_map(|k| backend.get(&k).map(|v| (k, self.from_slot(&cell_name, &slot_name, stored_to_value(v))))).collect()
            };
            let mut bad = 0usize;
            let mut sample = String::new();
            for (k, v) in &values {
                if self.check_slot_value_type(&cell_name, &slot_name, v).is_err() {
                    bad += 1;
                    if sample.is_empty() { sample = format!("{} = {} {}", k, value_type_name(v), short_value(v)); }
                }
            }
            if bad > 0 {
                out.push(format!("{} stored value(s) of slot '{}' are not of its declared type {} (e.g. {}): the slot's type changed since they were written — migrate them (read, convert, set) or rename the slot", bad, slot_name, self.slot_value_type(&cell_name, &slot_name).unwrap_or_default(), sample));
            }
        }
        // tables no slot reads any more (a renamed slot is a NEW empty slot;
        // the old rows stay in the file, invisible), tables of cells the
        // program no longer declares, and slots whose KIND changed
        // (List ↔ Map: the rows sit in the other table and read as empty)
        if let Some(conn) = crate::runtime::storage::shared_connection() {
            let mut expected: std::collections::HashSet<String> = self.storage.keys()
                .filter_map(|k| k.split_once('.').map(|(c, s)| format!("{}_{}", c, s)))
                .collect();
            for (cell, sm) in self.state_machines.keys() { expected.insert(format!("{}__sm_{}", cell, sm)); }
            // the runtime's own tables: next_id() counters, remember() memory
            for cell in self.cells.keys() {
                expected.insert(format!("{}__counters", cell));
                expected.insert(format!("{}__agent_memory", cell));
            }
            let mut cells: Vec<String> = self.cells.keys().cloned().collect();
            cells.sort_by_key(|c| std::cmp::Reverse(c.len()));   // OrderLine before Order
            let slot_keys: Vec<String> = { let mut v: Vec<String> = self.storage.keys().filter(|k| k.contains('.') && !k.starts_with("__")).cloned().collect(); v.sort(); v };
            let kinds: Vec<(String, String, Option<&'static str>)> = slot_keys.iter()
                .map(|k| { let (c, sl) = k.split_once('.').unwrap(); (c.to_string(), sl.to_string(), self.slot_kind(c, sl)) }).collect();
            let c = conn.lock().unwrap_or_else(|e| e.into_inner());
            let count = |t: &str| -> i64 { c.query_row(&format!("SELECT COUNT(*) FROM \"{}\"", t), [], |r| r.get(0)).unwrap_or(0) };
            let tables: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
                .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0)).map(|rows| rows.filter_map(|r| r.ok()).collect()))
                .unwrap_or_default();
            let table_set: std::collections::HashSet<&String> = tables.iter().collect();
            let mut orphans: Vec<String> = Vec::new();
            let mut orphan_machines: Vec<String> = Vec::new();
            let mut gone_cells: Vec<String> = Vec::new();
            for t in &tables {
                if t.ends_with("_log") || t.starts_with("sqlite_") || expected.contains(t) { continue; }
                // a horde's queue (`Audit__horde-meta`, `Audit__horde-tasks`)
                if t.ends_with("__horde-meta") || t.ends_with("__horde-tasks") { continue; }
                // the cluster's per-key versions and tombstones
                // (`_soma_cluster_v2_Replicated.records`): runtime metadata
                // of a declared cell, reported at every restart as the data
                // of a removed cell
                if t.starts_with("_soma_cluster_") { continue; }
                let log = format!("{}_log", t);
                let rows = count(t) + if table_set.contains(&log) { count(&log) } else { 0 };
                if rows == 0 { continue; }
                match cells.iter().find(|cn| t.starts_with(&format!("{}_", cn))) {
                    Some(cell) if !t.contains("__sm_") => orphans.push(format!("{}.{} ({} row(s))", cell, &t[cell.len() + 1..], rows)),
                    // a renamed state machine: its instances would silently
                    // restart from the initial state (terminal ones included)
                    Some(cell) => orphan_machines.push(format!("{}.{} ({} instance(s))", cell, t.split("__sm_").nth(1).unwrap_or(""), rows)),
                    None => gone_cells.push(format!("'{}' ({} row(s))", t, rows)),
                }
            }
            // (under soma run too: the cell is declared by THIS program, so a
            // shared directory is no excuse — a terminal order became fresh)
            if !orphan_machines.is_empty() {
                out.push(format!("state machine instances no machine declares any more: {} — a renamed or removed machine; under a new name every instance starts over from the initial state (terminal ones included): rename it back", orphan_machines.join(", ")));
            }
            if !orphans.is_empty() && IN_SERVE.load(std::sync::atomic::Ordering::Relaxed) {
                out.push(format!("slot data no slot declares any more: {} — a renamed or removed slot; its rows are still in .soma_data/soma.db (rename the slot back to read them, or copy them over in a one-shot handler)", orphans.join(", ")));
            }
            // only a SERVICE owns its data directory: under `soma run`
            // several programs often share one (examples/), and their
            // tables are not "gone"
            if !gone_cells.is_empty() && IN_SERVE.load(std::sync::atomic::Ordering::Relaxed) {
                out.push(format!("data of cells this program no longer declares: tables {} — a renamed or removed cell (its slots and state-machine instances are still in .soma_data/soma.db)", gone_cells.join(", ")));
            }
            for (cell, slot, kind) in kinds {
                let t = format!("{}_{}", cell, slot);
                let log = format!("{}_log", t);
                let (kv, lg) = (if table_set.contains(&t) { count(&t) } else { 0 }, if table_set.contains(&log) { count(&log) } else { 0 });
                match kind {
                    Some("List") if kv > 0 && lg == 0 => out.push(format!("slot '{}' is declared a List but holds {} keyed entries written when it was a Map — they read back as a list of values; migrate them", slot, kv)),
                    Some("Map") if lg > 0 && kv == 0 => out.push(format!("slot '{}' is declared a Map but holds {} list entries written when it was a List — it reads as empty; migrate them", slot, lg)),
                    _ => {}
                }
            }
        }
        // a `size` invariant over a List slot (the keyed pass above sees no keys)
        let list_invs: Vec<String> = self.invariants.keys().filter(|k| k.contains('.')).cloned().collect();
        for key in list_invs {
            let (cell_name, slot_name) = key.split_once('.').map(|(c, s)| (c.to_string(), s.to_string())).unwrap();
            if self.slot_kind(&cell_name, &slot_name) != Some("List") { continue; }
            let Some(backend) = self.storage.get(&key).cloned() else { continue };
            let items: Vec<StoredValue> = backend.list().into_iter().take(10_000).collect();
            let size = items.len() as i64;
            let mut bad = 0usize;
            let mut sample = String::new();
            for (i, v) in items.into_iter().enumerate() {
                let val = self.from_slot(&cell_name, &slot_name, stored_to_value(v));
                // the element's index is its key (it was checked with ""
                // and valid data at #0 was reported)
                if self.check_invariants(&cell_name, &slot_name, &i.to_string(), &val, size, "read").is_err() {
                    bad += 1;
                    if sample.is_empty() { sample = format!("#{} = {}", i, short_value(&val)); }
                }
            }
            if bad > 0 {
                out.push(format!("{} stored entr(ies) of List slot '{}' violate its invariant today (e.g. {}, size {}): fix the data or the invariant", bad, slot_name, sample, size));
            }
        }
        out
    }

    /// Fresh in-memory state-machine storage (test isolation between cells).
    pub fn reset_state_machine_storage(&mut self) {
        for ((cell_name, sm_name), _) in self.state_machines.clone() {
            let key = format!("__sm_{}_{}", cell_name, sm_name);
            self.storage.remove(&format!("__sm_{}", sm_name));
            self.storage.insert(key, Arc::new(crate::runtime::storage::MemoryBackend::new()) as Arc<dyn crate::runtime::storage::StorageBackend>);
        }
    }

    /// Take all emitted signals (drains the buffer)
    /// Find which cell has a handler for the given signal and call it
    /// Same as find_and_call but with a different name for pipe operator
    pub fn find_and_call_with_args(&mut self, signal_name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.find_and_call(signal_name, args)
    }

    pub fn find_and_call(&mut self, signal_name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        // Search handler cache for matching signal — the FIRST cell in
        // declaration order (a HashMap walk picked one at random when two
        // cells defined the name; check now refuses that program)
        let mut candidates: Vec<&String> = self.handler_cache.keys()
            .filter(|(_, sig)| sig == signal_name)
            .map(|(cell, _)| cell)
            .collect();
        candidates.sort_by_key(|c| self.cell_order.iter().position(|o| o == *c).unwrap_or(usize::MAX));
        let cell_name = candidates.first().map(|c| (*c).clone());

        if let Some(cell) = cell_name {
            self.call_signal(&cell, signal_name, args)
        } else {
            // Try as a builtin
            if let Some(result) = self.call_builtin(signal_name, &args, "") {
                result
            } else {
                Err(RuntimeError::UndefinedFn(signal_name.to_string()))
            }
        }
    }

    pub fn take_emitted_signals(&mut self) -> Vec<(String, Vec<Value>)> {
        std::mem::take(&mut self.emitted_signals)
    }

    /// Run a signal handler on a cell with the given arguments.
    /// Only clones the handler's params and body — not the whole CellDef.
    /// Invoke a handler. A top-level invocation is ATOMIC: serialized
    /// against every other top-level invocation of the process and rolled
    /// back entirely if it fails (see `atomically`). A nested call — a
    /// handler calling a handler — joins the unit already running.
    pub fn call_signal(
        &mut self,
        cell_name: &str,
        signal_name: &str,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        // a call that RETURNED leaves the position at the caller: an error
        // later in the caller's expression was reported inside the callee
        // (`assert request(…).x` "raised at line 3", the handler's line)
        let caller_span = self.last_span;
        // Record the outcome after the transaction, including native errors,
        // parameter failures and failed commits. Nested calls replay as part
        // of their caller and must not create separate entries.
        let is_recorded = self.current_depth == 0 && self.record_handlers.contains(&(cell_name.to_string(), signal_name.to_string()));
        let recorded_args = if is_recorded { Some(args.clone()) } else { None };
        if is_recorded { self.record_nondet_called.clear(); }
        let r = if self.journal.is_none() && self.task_unit.is_none() && self.handler_is_task(cell_name, signal_name) {
            self.run_task(|me| me.call_signal_inner(cell_name, signal_name, args))
        } else {
            self.atomically(|me| me.call_signal_inner(cell_name, signal_name, args))
        };
        match &r {
            Ok(val) => self.maybe_record(is_recorded, cell_name, signal_name, recorded_args.as_ref(), val, None),
            Err(e) => {
                let marker = map_from_pairs(vec![("__error__".to_string(), Value::String(e.kind()))]);
                self.maybe_record(is_recorded, cell_name, signal_name, recorded_args.as_ref(), &marker, Some(e.kind()));
            }
        }
        if r.is_ok() { self.last_span = caller_span; }
        r
    }

    fn call_signal_inner(
        &mut self,
        cell_name: &str,
        signal_name: &str,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        // a scripted answer from a test cell replaces the body (one per call):
        // `mock Cell.h` for this cell's h first, then a bare `mock h`
        let qualified = format!("{}.{}", cell_name, signal_name);
        let stub_key = if self.handler_stubs.get(&qualified).map_or(false, |q| !q.is_empty()) { qualified } else { signal_name.to_string() };
        if let Some(q) = self.handler_stubs.get_mut(&stub_key) {
            if let Some(answer) = q.pop_front() {
                return match answer {
                    // a scripted answer is held to the face's return type as
                    // the real handler is (`mock price "abc"` for `-> Int`)
                    Ok(v) => self.check_face_return(cell_name, signal_name, v),
                    Err(msg) => {
                        // `mock h error "not_found: no such item"` raises kind
                        // not_found; a bare "down" is both kind and detail
                        let (kind, detail) = match msg.split_once(": ") {
                            Some((k, d)) if !k.is_empty() && !k.contains(' ') => (k.to_string(), d.to_string()),
                            _ => (msg.clone(), msg.clone()),
                        };
                        Err(RuntimeError::Domain { kind: kind.clone(), message: format!("{}: {}", kind, detail) })
                    }
                };
            }
        }
        // Check for [native] FFI handler first — fast path
        let native_key = (cell_name.to_string(), signal_name.to_string());
        // the declared parameter types, as for an interpreted handler: the
        // FFI reinterpreted the bits (sq(2.5) squared 2.5's bit pattern)
        let args = if self.native_handlers.contains_key(&native_key) {
            let params: Option<Vec<Param>> = self.cells.get(cell_name).and_then(|c| c.sections.iter().find_map(|s| match &s.node {
                Section::OnSignal(on) if on.signal_name == signal_name => Some(on.params.clone()),
                _ => None,
            }));
            match params {
                Some(params) => {
                    if args.len() != params.len() {
                        return Err(RuntimeError::TypeError(format!("{}() expected {} arguments, got {}", signal_name, params.len(), args.len())));
                    }
                    let mut checked = Vec::with_capacity(args.len());
                    for (p, a) in params.iter().zip(args.into_iter()) {
                        match check_param_type(p, a).and_then(|v| self.check_sum_param(p, v)) {
                            Ok(v) => checked.push(v),
                            Err(m) => return Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("{}(): {}", signal_name, m) }),
                        }
                    }
                    checked
                }
                None => args,
            }
        } else { args };
        // a test that mocks a handler: the native code would call its
        // sibling directly, past the mock (`assert_fails outer(1)` passed
        // interpreted and failed with [native]) — interpret it, the
        // backends agree by construction
        if self.native_handlers.contains_key(&native_key) && self.handler_stubs.values().all(|q| q.is_empty()) {
            let native = self.native_handlers.get(&native_key).unwrap();
            match native_ffi::call_native(native, &args) {
                Ok(val) => {
                    let val = self.check_face_return(cell_name, signal_name, val)?;
                    return Ok(val);
                }
                Err(e) if e.contains("overflow_rerun") => {
                    // i128 overflow — fall through to interpreted path for BigInt
                    eprintln!("[native] i128 overflow, falling back to interpreted BigInt");
                }
                // a native `soma:<kind>: …` refusal keeps its kind (a
                // loop_bound overrun was kind "type" natively, "loop_bound"
                // interpreted)
                // …and the interpreter's wording: its messages carry no
                // `kind: ` prefix (str_at raised "index: str_at: …" natively)
                Err(e) => return Err(match e.split_once(": ") {
                    Some((k, rest)) if matches!(k, "loop_bound" | "range" | "index") => RuntimeError::Domain { kind: k.to_string(), message: rest.to_string() },
                    Some(("type", rest)) => RuntimeError::TypeError(rest.to_string()),
                    _ => RuntimeError::TypeError(e),
                }),
            }
        }

        // Lookup from pre-computed cache — O(1) instead of scanning sections
        let key = (cell_name.to_string(), signal_name.to_string());
        let (params, body) = {
            let entry = self.handler_cache.get(&key)
                .ok_or_else(|| RuntimeError::NoHandler(signal_name.to_string(), cell_name.to_string()))?;
            // Arc::clone is cheap — just increments a refcount (no deep copy)
            (Arc::clone(&entry.0), Arc::clone(&entry.1))
        };

        // Cache this handler for fast recursive lookups
        let prev_handler = self.current_handler.take();
        self.current_handler = Some((
            cell_name.to_string(),
            signal_name.to_string(),
            Arc::clone(&params),
            Arc::clone(&body),
        ));

        let result = self.call_signal_resolved(cell_name, signal_name, args, &params, &body);

        self.current_handler = prev_handler;
        // The face is a contract: `signal f() -> Int` returning a String
        // used to pass check AND run. Checked at the boundary, with the
        // same leniency as parameters (Map takes a record/variant/`()`).
        // (`request` is the router: a route answers with whatever the
        // route returns — list, string, response map — so it is exempt)
        let result = match result {
            Ok(val) => self.check_face_return(cell_name, signal_name, val),
            err => err,
        };
        result
    }

    fn face_return_type(&self, cell_name: &str, signal_name: &str) -> Option<Spanned<TypeExpr>> {
        let cell = self.cells.get(cell_name)?;
        cell.sections.iter().find_map(|s| match &s.node {
            Section::Face(face) => face.declarations.iter().find_map(|d| match &d.node {
                FaceDecl::Signal(sig) if sig.name == signal_name => sig.return_type.clone(),
                _ => None,
            }),
            _ => None,
        })
    }

    /// Append a record entry to the .somalog file if recording is active.
    fn maybe_record(
        &mut self,
        is_recorded: bool,
        cell_name: &str,
        signal_name: &str,
        args: Option<&Vec<Value>>,
        result: &Value,
        error_kind: Option<String>,
    ) {
        if !is_recorded || self.replay_mode { return; }
        let Some(path) = self.record_log_path.clone() else { return; };
        let Some(args) = args else { return; };
        let entry = record_log::RecordEntry {
            ts_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            cell: cell_name.to_string(),
            handler: signal_name.to_string(),
            args: args.clone(),
            result: result.clone(),
            error_kind,
            nondet: std::mem::take(&mut self.record_nondet_called),
            src: self.source_text.as_deref().map(record_log::source_fingerprint),
        };
        if let Err(e) = record_log::append(&path, &entry) {
            eprintln!("warning: failed to append to {}: {}", path.display(), e);
        }
    }

    /// Fast path: execute a signal handler with pre-resolved params/body.
    /// Avoids the HashMap lookup when we already know which handler to call.
    fn call_signal_resolved(
        &mut self,
        cell_name: &str,
        signal_name: &str,
        args: Vec<Value>,
        params: &[Param],
        body: &[Spanned<Statement>],
    ) -> Result<Value, RuntimeError> {
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.current_depth -= 1;
            return Err(RuntimeError::StackOverflow);
        }

        // a left-out trailing `Map` parameter (an options map) is map()
        let mut args = args;
        if args.len() < params.len() && crate::ast::accepted_arities(params).contains(&args.len()) {
            while args.len() < params.len() { args.push(Value::Map(Default::default())); }
        }
        // Check arity
        if args.len() != params.len() {
            self.current_depth -= 1;
            let ok = crate::ast::accepted_arities(params);
            let expected = if ok.len() > 1 {
                let mut v = ok.clone(); v.sort();
                format!("{} to {} arguments (trailing Map parameters are optional)", v[0], v[v.len() - 1])
            } else {
                format!("{} argument{}", params.len(), if params.len() == 1 { "" } else { "s" })
            };
            return Err(RuntimeError::TypeError(format!("{}() expected {}, got {}", signal_name, expected, args.len())));
        }

        // Bind parameters, checking the declared type at the boundary: a
        // String reaching an `Int` parameter used to surface deep inside the
        // body ("cannot compare String and Int"), or not at all.
        let mut env = FxHashMap::with_capacity_and_hasher(params.len() + 4, Default::default());
        for (param, val) in params.iter().zip(args) {
            let val = match check_param_type(param, val).and_then(|v| self.check_sum_param(param, v)) {
                Ok(v) => v,
                Err(msg) => {
                    self.current_depth -= 1;
                    return Err(RuntimeError::TypeError(format!("{}(): {}", signal_name, msg)));
                }
            };
            env.insert(param.name.clone(), val);
        }

        let result = self.exec_body(body, &mut env, cell_name, signal_name);

        self.current_depth -= 1;

        match result {
            Ok(val) => Ok(val),
            Err(ExecError::Return(val)) => Ok(val),
            Err(ExecError::Break) => Err(RuntimeError::TypeError("break outside of loop".to_string())),
            Err(ExecError::Continue) => Err(RuntimeError::TypeError("continue outside of loop".to_string())),
            Err(ExecError::Runtime(e)) => Err(e),
        }
    }

    // We use a separate error type internally to handle early returns
    fn exec_body(
        &mut self,
        body: &[Spanned<Statement>],
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        let mut last_value = Value::Unit;

        for stmt in body {
            self.last_span = Some(stmt.span);
            last_value = self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
        }

        Ok(last_value)
    }

    /// Execute a body with proper block scoping:
    /// - `let` declarations are local to the block
    /// - assignments to pre-existing variables propagate to outer scope
    fn exec_body_scoped(
        &mut self,
        body: &[Spanned<Statement>],
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        with_lexical_scope(body, env, |env| self.exec_body(body, env, cell_name, signal_name))
    }

    fn exec_stmt(
        &mut self,
        stmt: &Statement,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        if crate::runtime::storage::has_storage_error() { self.check_storage_write()?; }
        self.check_transaction()?;
        match stmt {
            Statement::Let { name, value } => {
                self.last_span = Some(value.span);
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::IndexSet { name, index, value } => {
                self.last_span = Some(value.span);
                let idx_val = self.eval_expr(&index.node, env, cell_name, signal_name)?;
                let new_val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                // A storage slot: xs[k] = v  ⇒  xs.set(k, v) — unless a LOCAL
                // of that name is in scope (`let rows = [1, 2]  rows[0] = 99`
                // used to overwrite the persistent slot `rows`)
                if !env.contains_key(name) && (self.storage.contains_key(name)
                    || self.storage.contains_key(&format!("{}.{}", cell_name, name)))
                {
                    self.call_storage_method(cell_name, name, "set", &[idx_val, new_val])?;
                    return Ok(Value::Unit);
                }
                // A local list or map, updated in place.
                match env.get_mut(name) {
                    Some(Value::List(items)) => {
                        let raw = idx_val.as_int().map_err(ExecError::Runtime)?;
                        let i = if raw < 0 { raw + items.len() as i64 } else { raw };
                        if i < 0 || i as usize >= items.len() {
                            return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                                "list index {} out of bounds (length {})", raw, items.len()
                            ))));
                        }
                        items[i as usize] = new_val;
                        Ok(Value::Unit)
                    }
                    Some(Value::Map(entries)) => {
                        entries.insert(format!("{}", idx_val), new_val);
                        Ok(Value::Unit)
                    }
                    // a record: `l.qty = v` on a struct variant sets a declared field
                    Some(Value::Variant { type_name, variant, fields: VariantValue::Struct(fs) }) => {
                        let key = match &idx_val { Value::String(k) => k.clone(), other => format!("{}", other) };
                        if !fs.contains_key(&key) {
                            let known: Vec<&str> = fs.keys().map(|k| k.as_str()).collect();
                            return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                                "{} has no field '{}' (fields: {})", variant, key, known.join(", ")
                            ))));
                        }
                        // the declared field type holds (`l.qty = "x"` on qty: Int was kept)
                        let fty = match self.variant_fields.get(&(type_name.clone(), variant.clone())) {
                            Some(crate::ast::VariantFields::Struct(fields)) => fields.iter().find(|(f, _)| *f == key).map(|(_, t)| t.node.clone()),
                            _ => None,
                        };
                        let new_val = match fty {
                            Some(t) => {
                                let v = self.coerce_records(&t, new_val).map_err(|e| ExecError::Runtime(RuntimeError::TypeError(format!("{}.{}: {}", variant, key, e))))?;
                                self.value_fits(&t, &v).map_err(|e| ExecError::Runtime(RuntimeError::TypeError(format!("{}.{}: {}", variant, key, e))))?;
                                v
                            }
                            None => new_val,
                        };
                        fs.insert(key, new_val);
                        Ok(Value::Unit)
                    }
                    Some(other) => Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "cannot index-assign into {} '{}'", value_type_name(other), name
                    )))),
                    None => Err(ExecError::Runtime(RuntimeError::UndefinedVar(name.clone()))),
                }
            }

            Statement::Assign { name, value } => {
                self.last_span = Some(value.span);
                if !env.contains_key(name) && self.slot_kind(cell_name, name).is_some() {
                    return Err(ExecError::Runtime(RuntimeError::TypeError(slot_assign_message(name, self.slot_kind(cell_name, name)))));
                }
                // Preserve the documented self-append idiom without removing
                // the list while its arguments can still read it.
                if let Expr::FnCall { name: fn_name, args } = &value.node {
                    if matches!(fn_name.as_str(), "list" | "push" | "append") && args.len() >= 2
                        && self.builtin_fast_path(fn_name, args.len(), env, cell_name)
                        && matches!(&args[0].node, Expr::Ident(n) if n == name)
                        && matches!(env.get(name), Some(Value::List(_)))
                    {
                        let snapshot = args[1..].iter().any(|a| expr_may_assign(&a.node)).then(|| env[name].clone());
                        let tail = args[1..].iter().map(|a| self.eval_expr(&a.node, env, cell_name, signal_name)).collect::<Result<Vec<_>, _>>()?;
                        if !self.builtin_fast_path(fn_name, args.len(), env, cell_name) {
                            let mut values = vec![snapshot.unwrap_or_else(|| env[name].clone())];
                            values.extend(tail);
                            let val = self.eval_named_call(fn_name, values, env, cell_name, signal_name)?;
                            env.insert(name.clone(), val);
                        } else {
                            let mut base = snapshot.unwrap_or_else(|| env.remove(name).unwrap());
                            if let Value::List(xs) = &mut base { xs.extend(tail); }
                            env.insert(name.clone(), base);
                        }
                        return Ok(Value::Unit);
                    }
                }
                // Reuse already evaluated arithmetic operands. Logical operators
                // retain eval_expr's left-type check and short-circuit semantics.
                if let Expr::BinaryOp { left, op, right } = &value.node {
                    if !matches!(op, BinOp::And | BinOp::Or)
                        && matches!(&left.node, Expr::Ident(n) if n == name)
                    {
                        if let Some(Value::Int(current)) = env.get(name) {
                            let lhs = Value::Int(current.clone());
                            let rhs = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                            let val = self.eval_binop(&lhs, *op, &rhs).map_err(ExecError::Runtime)?;
                            env.insert(name.clone(), val);
                            return Ok(Value::Unit);
                        }
                    }
                }
                // All pairs are evaluated before an in-place map update commits.
                if let Expr::Pipe { left, right } = &value.node {
                    if matches!(&left.node, Expr::Ident(n) if n == name) {
                        if let Expr::FnCall { name: fn_name, args } = &right.node {
                            if fn_name == "with" && args.len() % 2 == 0
                                && self.builtin_fast_path(fn_name, args.len() + 1, env, cell_name)
                                && matches!(env.get(name), Some(Value::Map(_)))
                            {
                                let snapshot = args.iter().any(|a| expr_may_assign(&a.node)).then(|| env[name].clone());
                                let values = args.iter().map(|a| self.eval_expr(&a.node, env, cell_name, signal_name)).collect::<Result<Vec<_>, _>>()?;
                                if !self.builtin_fast_path(fn_name, args.len() + 1, env, cell_name) {
                                    let mut all = vec![snapshot.unwrap_or_else(|| env[name].clone())];
                                    all.extend(values);
                                    let val = self.eval_named_call(fn_name, all, env, cell_name, signal_name)?;
                                    env.insert(name.clone(), val);
                                } else {
                                    let mut base = snapshot.unwrap_or_else(|| env.remove(name).unwrap());
                                    if let Value::Map(m) = &mut base {
                                        for pair in values.chunks_exact(2) { m.insert(format!("{}", pair[0]), pair[1].clone()); }
                                    }
                                    env.insert(name.clone(), base);
                                }
                                return Ok(Value::Unit);
                            }
                        }
                    }
                }
                // `m = with(m, k, v)` / `m = without(m, k)` on a local map:
                // update in place like the pipe form above (each call copied
                // the whole map: 20 000 removals took 26 s)
                if let Expr::FnCall { name: fn_name, args } = &value.node {
                    if matches!(fn_name.as_str(), "with" | "without") && args.len() >= 2
                        && (fn_name == "without" || (args.len() - 1) % 2 == 0)
                        && self.builtin_fast_path(fn_name, args.len(), env, cell_name)
                        && matches!(&args[0].node, Expr::Ident(n) if n == name)
                        && matches!(env.get(name), Some(Value::Map(_)))
                    {
                        let snapshot = args[1..].iter().any(|a| expr_may_assign(&a.node)).then(|| env[name].clone());
                        let values = args[1..].iter().map(|a| self.eval_expr(&a.node, env, cell_name, signal_name)).collect::<Result<Vec<_>, _>>()?;
                        if !self.builtin_fast_path(fn_name, args.len(), env, cell_name) {
                            let mut all = vec![snapshot.unwrap_or_else(|| env[name].clone())];
                            all.extend(values);
                            let val = self.eval_named_call(fn_name, all, env, cell_name, signal_name)?;
                            env.insert(name.clone(), val);
                        } else {
                            let mut base = snapshot.unwrap_or_else(|| env.remove(name).unwrap());
                            if let Value::Map(m) = &mut base {
                                if fn_name == "with" {
                                    for pair in values.chunks_exact(2) { m.insert(format!("{}", pair[0]), pair[1].clone()); }
                                } else {
                                    for k in &values { m.shift_remove(&format!("{}", k)); }
                                }
                            }
                            env.insert(name.clone(), base);
                        }
                        return Ok(Value::Unit);
                    }
                }
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::Return { value } => {
                self.last_span = Some(value.span);
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                Err(ExecError::Return(val))
            }

            Statement::Break => {
                Err(ExecError::Break)
            }

            Statement::Continue => {
                Err(ExecError::Continue)
            }

            Statement::Ensure { condition } => {
                // Postcondition: store condition for checking after handler returns.
                // For now, evaluate eagerly — if false at this point, fail.
                // This is useful for pre+post style: ensure at end of handler body.
                let val = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                // a Bool, like `if` (a mask or "false" passed)
                if !val.as_bool().map_err(ExecError::Runtime)? {
                    Err(ExecError::Runtime(RuntimeError::RequireFailed(
                        format!("ensure postcondition failed")
                    )))
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                self.last_span = Some(condition.span);
                let cond = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                if cond_truth(&cond)? {
                    self.exec_body_scoped(then_body, env, cell_name, signal_name)
                } else if !else_body.is_empty() {
                    self.exec_body_scoped(else_body, env, cell_name, signal_name)
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::For { var, iter, body, bound } => {
                let bound = *bound;
                // `[loop_bound(N)]` is part of a cost / termination proof:
                // more iterations than declared is an error, not a silent
                // overrun (a "proven" 100-token bound spent 300)
                let over = |n: u128| -> Result<(), ExecError> {
                    match bound {
                        Some(b) if n > b as u128 => Err(ExecError::Runtime(RuntimeError::Domain {
                            kind: "loop_bound".to_string(),
                            message: format!("for [loop_bound({})] would run {} times — the declared bound is part of the cost/termination proof: raise it or bound the data", b, n),
                        })),
                        _ => Ok(()),
                    }
                };
                // The loop variable shadows any outer binding of the same name;
                // restore it (rather than just removing) when the loop ends.
                let shadowed = env.get(var).cloned();
                let restore = |env: &mut Env| {
                    match &shadowed {
                        Some(old) => { env.insert(var.clone(), old.clone()); }
                        None => { env.remove(var); }
                    }
                };
                // Builtin ranges do not allocate a list. Respect user handlers
                // and closures, and evaluate each argument exactly once.
                if let Expr::FnCall { name: fn_name, args: fn_args } = &iter.node {
                    if fn_name == "range" && (2..=3).contains(&fn_args.len())
                        && self.builtin_fast_path(fn_name, fn_args.len(), env, cell_name)
                        && !fn_args.iter().any(|a| expr_may_assign(&a.node))
                    {
                        let values = fn_args.iter().map(|a| self.eval_expr(&a.node, env, cell_name, signal_name)).collect::<Result<Vec<_>, _>>()?;
                        let (start, end, step, count) = builtins::collection::range_spec(&values).map_err(ExecError::Runtime)?;
                        over(count)?;
                        let mut last = Value::Unit;
                        let mut i = start;
                        let needs_scope = body_has_let(body);
                        while if step > 0 { i < end } else { i > end } {
                            env.insert(var.clone(), Value::Int(SomaInt::from_i64(i)));
                            let result = if needs_scope {
                                self.exec_body_scoped(body, env, cell_name, signal_name)
                            } else {
                                self.exec_body(body, env, cell_name, signal_name)
                            };
                            match result {
                                Ok(val) => last = val,
                                Err(ExecError::Break) => break,
                                Err(ExecError::Continue) => {},
                                Err(e) => { restore(env); return Err(e); }
                            }
                            match i.checked_add(step) { Some(next) => i = next, None => break }
                        }
                        restore(env);
                        return Ok(last);
                    }
                }

                // General path: evaluate iterator
                let iter_val = self.eval_expr(&iter.node, env, cell_name, signal_name)?;

                let items = match iter_val {
                    Value::List(items) => items,
                    Value::Map(entries) => {
                        entries.into_iter().map(|(k, v)| {
                            map_from_pairs(vec![
                                ("key".to_string(), Value::String(k)),
                                ("value".to_string(), v),
                            ])
                        }).collect()
                    }
                    Value::String(s) => {
                        if s.contains('\n') {
                            s.split('\n')
                                .filter(|l| !l.is_empty())
                                .map(|l| Value::String(l.to_string()))
                                .collect()
                        } else {
                            vec![Value::String(s)]
                        }
                    }
                    // an absent value (`slot.get(missing)`) has no elements
                    // (the body ran once with `()`); a number is not a
                    // sequence (`for x in 5` ran once with 5)
                    Value::Unit => Vec::new(),
                    other => return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "for {} in …: {} {} is not a List, Map or String — for a count write `for i in range(0, n)`", var, value_type_name(&other), { let t: String = format!("{}", other).chars().take(30).collect(); t }) })),
                };
                over(items.len() as u128)?;

                let mut last = Value::Unit;
                for item in items {
                    env.insert(var.clone(), item);
                    match self.exec_body_scoped(body, env, cell_name, signal_name) {
                        Ok(val) => last = val,
                        Err(ExecError::Break) => break,
                        Err(ExecError::Continue) => continue,
                        Err(e) => { restore(env); return Err(e); }
                    }
                }
                restore(env);
                Ok(last)
            }

            Statement::While { condition, body, bound: Some(b) } => {
                // a declared bound is checked: iteration N+1 raises
                let b = *b;
                let mut n: u64 = 0;
                let mut last = Value::Unit;
                loop {
                    let c = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                    if !c.as_bool().map_err(ExecError::Runtime)? { break; }
                    if n == b {
                        return Err(ExecError::Runtime(RuntimeError::Domain {
                            kind: "loop_bound".to_string(),
                            message: format!("while [loop_bound({})] is still true after {} iterations — the declared bound is part of the cost/termination proof", b, b),
                        }));
                    }
                    n += 1;
                    match self.exec_body_scoped(body, env, cell_name, signal_name) {
                        Ok(v) => last = v,
                        Err(ExecError::Break) => break,
                        Err(ExecError::Continue) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(last)
            }
            Statement::While { condition, body, .. } => {
                // Prepare the entire integer-register loop before executing it.
                // Falling back halfway through an iteration replays assignments;
                // a missing RHS register must never silently become zero.
                if let Expr::CmpOp { left, op: CmpOp::Lt, right } = &condition.node {
                    if let (Expr::Ident(counter), Expr::Literal(Literal::Int(limit))) = (&left.node, &right.node) {
                        enum Operand { Literal(i64), Local(usize) }
                        let mut names = vec![counter.clone()];
                        let mut slot = |name: &String| {
                            if let Some(i) = names.iter().position(|n| n == name) { i }
                            else { names.push(name.clone()); names.len() - 1 }
                        };
                        let prepared: Option<Vec<_>> = body.iter().map(|stmt| {
                            let Statement::Assign { name, value } = &stmt.node else { return None };
                            let Expr::BinaryOp { left, op, right } = &value.node else { return None };
                            if !matches!(&left.node, Expr::Ident(n) if n == name)
                                || !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) { return None; }
                            let target = slot(name);
                            let rhs = match &right.node {
                                Expr::Literal(Literal::Int(n)) => Operand::Literal(*n),
                                Expr::Ident(n) => Operand::Local(slot(n)),
                                _ => return None,
                            };
                            Some((target, *op, rhs))
                        }).collect();
                        if let Some(ops) = prepared {
                            let registers: Option<Vec<_>> = names.iter().map(|name| match env.get(name) {
                                Some(Value::Int(n)) => Some(n.clone()),
                                _ => None,
                            }).collect();
                            if let Some(mut values) = registers {
                                let limit = SomaInt::from_i64(*limit);
                                let result: Result<(), RuntimeError> = (|| {
                                    while values[0].cmp(&limit) < 0 {
                                        for (target, op, rhs) in &ops {
                                            let rhs = match rhs {
                                                Operand::Literal(n) => SomaInt::from_i64(*n),
                                                Operand::Local(i) => values[*i].clone(),
                                            };
                                            let lhs = values[*target].clone();
                                            values[*target] = match op {
                                                BinOp::Add => lhs.add(rhs),
                                                BinOp::Sub => lhs.sub(rhs),
                                                BinOp::Mul => lhs.checked_big_mul(rhs).map_err(|message| RuntimeError::Domain { kind: "range".into(), message })?,
                                                _ => unreachable!(),
                                            };
                                        }
                                    }
                                    Ok(())
                                })();
                                for (name, value) in names.into_iter().zip(values) {
                                    env.insert(name, Value::Int(value));
                                }
                                result.map_err(ExecError::Runtime)?;
                                return Ok(Value::Unit);
                            }
                        }
                    }
                }
                // General path
                loop {
                    let cond = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                    if !cond_truth(&cond)? {
                        break;
                    }
                    match self.exec_body_scoped(body, env, cell_name, signal_name) {
                        Ok(_) => {}
                        Err(ExecError::Break) => break,
                        Err(ExecError::Continue) => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }

            Statement::ExprStmt { expr } => {
                self.last_span = Some(expr.span);
                self.eval_expr(&expr.node, env, cell_name, signal_name)
            }

            Statement::Require {
                constraint,
                else_signal,
            } => {
                let result = self.eval_constraint(&constraint.node, env, cell_name, signal_name)?;
                if !result {
                    // `else Tag` → kind Tag. `else "text {x}"` → a message,
                    // interpolated like any string (it used to be kept
                    // literally, braces and all), kind "require".
                    if let Some((tag, detail)) = else_signal.split_once('\u{1f}') {
                        // `else Tag "detail {x}"`: kind Tag, interpolated detail
                        let text = self.interpolate_string(detail, env, cell_name, signal_name)?;
                        return Err(ExecError::Runtime(RuntimeError::Domain {
                            kind: tag.to_string(),
                            message: format!("{}: {}", tag, text),
                        }));
                    }
                    let is_tag = !else_signal.is_empty()
                        && else_signal.chars().all(|c| c.is_alphanumeric() || c == '_');
                    if is_tag {
                        Err(ExecError::Runtime(RuntimeError::RequireFailed(
                            format!("{}: constraint violated", else_signal)
                        )))
                    } else {
                        let text = self.interpolate_string(else_signal, env, cell_name, signal_name)?;
                        Err(ExecError::Runtime(RuntimeError::Domain {
                            kind: "require".to_string(),
                            message: format!("require failed: {}", text),
                        }))
                    }
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::Emit { signal_name: sig, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }
                let dispatch_args = arg_vals.clone();
                self.emitted_signals.push((sig.clone(), arg_vals.clone()));
                // Prepare data for broadcast
                let broadcast_data = if arg_vals.len() == 1 { arg_vals[0].clone() } else { Value::List(arg_vals) };
                // Broadcast to event bus (SSE / WebSocket clients) — at commit
                if self.event_bus.is_some() {
                    self.send_bus(BusEvent { stream: sig.clone(), data: broadcast_data.clone(), internal: true });
                }
                // Send to peer bus (inter-process)
                if let Some(ref peers) = self.peer_bus {
                    // valid JSON on the wire (`{"inf": inf}` made the peer
                    // drop the whole event, silently): NaN / inf travel as null
                    let line = format!("EVENT {} {}\n", sig, builtins::string::to_json_string(&broadcast_data));
                    // at COMMIT, like SSE / WebSocket pushes: a handler that
                    // then raised had already told the other process
                    match self.journal.as_mut() {
                        Some(j) => j.push(UndoOp::PeerSend(line)),
                        None => if let Ok(mut senders) = peers.lock() {
                            send_to_peers(&mut senders, &line);
                        },
                    }
                }
                // Dispatch to sibling cells with matching handler (intra-process)
                // every cell with `on sig(…)`, the emitting cell included
                // (it used to be skipped) — except the handler that is
                // itself running `on sig` (an echo would recurse forever);
                // in declaration order, so the fan-out is deterministic
                let mut matching_cells: Vec<String> = self.handler_cache.keys()
                    .filter(|(c, s)| s == sig && !(c == cell_name && signal_name == sig))
                    .map(|(c, _)| c.clone())
                    .collect();
                matching_cells.sort_by_key(|c| self.cell_order.iter().position(|o| o == c).unwrap_or(usize::MAX));
                for target_cell in matching_cells {
                    self.call_signal(&target_cell, sig, dispatch_args.clone())
                        .map_err(ExecError::Runtime)?;
                }
                Ok(Value::Unit)
            }

            Statement::MethodCall { target, method, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }
                if !env.contains_key(target) && self.cells.contains_key(target)
                    && !self.storage.contains_key(target)
                    && !self.storage.contains_key(&format!("{}.{}", cell_name, target))
                {
                    return self.call_cell_handler(target, method, arg_vals);
                }
                // a local of the slot's name: `let rows = []  rows.push(x)`
                // would write the SLOT — refuse instead of guessing
                if env.contains_key(target) && (self.storage.contains_key(target)
                    || self.storage.contains_key(&format!("{}.{}", cell_name, target)))
                {
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "`{t}.{m}(…)`: `{t}` is a local variable here, and it has the name of the memory slot `{t}` — rename the local (a local list is rebuilt: `{t} = push({t}, x)`)",
                        t = target, m = method
                    ))));
                }
                // Check if target is a memory slot with a storage backend
                self.call_storage_method(cell_name, target, method, &arg_vals)
            }
        }
    }

    fn eval_expr(
        &mut self,
        expr: &Expr,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        if crate::runtime::storage::has_storage_error() { self.check_storage_write()?; }
        self.check_transaction()?;
        match expr {
            Expr::Literal(lit) => {
                let val = self.eval_literal(lit);
                // Auto-interpolate strings: "Hello {name}" → "Hello Alice"
                if let Value::String(ref s) = val {
                    if s.contains('{') {
                        return Ok(Value::String(self.interpolate_string(s, env, cell_name, signal_name)?));
                    }
                }
                Ok(val)
            }

            Expr::Ident(name) => {
                // Identifier in expression position can be:
                //   - a local/scope binding (most common path)
                //   - a unit sum-type variant (e.g., `Pending`).
                // Locals win — they shadow.
                if let Some(v) = env.get(name) {
                    Ok(v.clone())
                } else if let Some(v) = self.materialize_slot(cell_name, name) {
                    self.check_storage_write()?;
                    // a memory slot read by its bare name: its whole content
                    // (List → items, Map → entries). Writes go through
                    // slot.push / slot.set — never through assignment.
                    Ok(v)
                } else if let Some((type_name, shape)) = self.variant_registry.get(name) {
                    match shape {
                        VariantShape::Unit => Ok(Value::Variant {
                            type_name: type_name.clone(),
                            variant: name.clone(),
                            fields: VariantValue::Unit,
                        }),
                        VariantShape::Tuple(_) | VariantShape::Struct(_) => Err(ExecError::Runtime(
                            RuntimeError::TypeError(format!(
                                "variant '{}' takes payload — write a constructor expression",
                                name
                            )),
                        )),
                    }
                } else if let Some(state) = self.find_state_machine_for(cell_name)
                    .filter(|(sm, _)| sm.initial == *name || sm.transitions.iter().any(|t| t.node.from == *name || t.node.to == *name))
                    .map(|_| name.clone())
                {
                    // `transition(id, CLOSED)`: a bare state name of this cell's
                    // machine is that state (check and verify already read it so)
                    Ok(Value::String(state))
                } else {
                    Err(ExecError::Runtime(RuntimeError::UndefinedVar(name.clone())))
                }
            }

            Expr::BinaryOp { left, op, right } => {
                // Short-circuit for logical And/Or
                if *op == BinOp::And {
                    let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                    // `&&` / `||` take Bools: a comparison MASK (`xs >= 0` on a
                    // list) or a String counted as true, so a compound
                    // invariant, guard or assert let `[5000000]` through
                    if !l.as_bool().map_err(|e| ExecError::Runtime(and_or_err(e, "&&")))? {
                        return Ok(Value::Bool(false));
                    }
                    let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                    return Ok(Value::Bool(r.as_bool().map_err(|e| ExecError::Runtime(and_or_err(e, "&&")))?));
                }
                if *op == BinOp::Or {
                    let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                    if l.as_bool().map_err(|e| ExecError::Runtime(and_or_err(e, "||")))? {
                        return Ok(Value::Bool(true));
                    }
                    let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                    return Ok(Value::Bool(r.as_bool().map_err(|e| ExecError::Runtime(and_or_err(e, "||")))?));
                }
                let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                self.eval_binop(&l, *op, &r).map_err(ExecError::Runtime)
            }

            Expr::CmpOp { left, op, right } => {
                let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                self.eval_cmpop(&l, *op, &r).map_err(ExecError::Runtime)
            }

            Expr::FnCall { name, args } => {
                // `a ?? b` short-circuits: `m.get(k) ?? fail(…)` failed on a
                // present key, `id ?? next_id()` burned an id per call
                if name == "_coalesce" && args.len() == 2 {
                    let a = self.eval_expr(&args[0].node, env, cell_name, signal_name)?;
                    if !matches!(a, Value::Unit) { return Ok(a); }
                    return self.eval_expr(&args[1].node, env, cell_name, signal_name);
                }
                // `len(rows)` / `nth(rows, i)` on a local: read in place —
                // evaluating `rows` copied the whole list per call (a
                // `while i < len(rows)` loop over 20k rows took 39 s)
                if matches!(name.as_str(), "len" | "nth") && self.builtin_fast_path(name, args.len(), env, cell_name) {
                    if let Some(Expr::Ident(local)) = args.first().map(|a| &a.node) {
                        if name == "len" && args.len() == 1 {
                            match env.get(local) {
                                Some(Value::List(xs)) => return Ok(Value::Int(SomaInt::from_i64(xs.len() as i64))),
                                Some(Value::Map(m)) => return Ok(Value::Int(SomaInt::from_i64(m.len() as i64))),
                                Some(Value::String(t)) => return Ok(Value::Int(SomaInt::from_i64(t.chars().count() as i64))),
                                Some(_) => {}
                                // `len(slot)`: counted by the backend — reading
                                // the bare name materialized every row
                                None => if let Some(kind) = self.slot_kind(cell_name, local) {
                                    if let Some(b) = self.storage.get(&format!("{}.{}", cell_name, local)).or_else(|| if cell_name.is_empty() || self.is_test_cell(cell_name) { self.storage.get(local.as_str()) } else { None }) {
                                        let n = if kind == "List" { b.list_len() } else { b.len() };
                                        return Ok(Value::Int(SomaInt::from_i64(n as i64)));
                                    }
                                },
                            }
                        }
                        if name == "nth" && args.len() == 2 && !expr_may_assign(&args[1].node) && matches!(env.get(local), Some(Value::List(_))) {
                            let idx = self.eval_expr(&args[1].node, env, cell_name, signal_name)?;
                            if let (Some(Value::List(xs)), Value::Int(i)) = (env.get(local), &idx) {
                                // like the builtin: negative from the end, () out of range
                                let raw = i.to_i64().ok_or_else(|| ExecError::Runtime(RuntimeError::Domain {
                                    kind: "range".to_string(),
                                    message: "nth(): an Int argument past 64 bits (a count, index or width is at most 2^63 - 1)".to_string(),
                                }))?;
                                let k = if raw < 0 { raw + xs.len() as i64 } else { raw };
                                return Ok(if k >= 0 && (k as usize) < xs.len() { xs[k as usize].clone() } else { Value::Unit });
                            }
                            return Err(ExecError::Runtime(RuntimeError::TypeError("nth(list, i): index must be an Int".to_string())));
                        }
                    }
                }
                let mut arg_vals = Vec::new();
                // `horde(Reviewer.review, …)` / `horde(review, …)`: the first
                // argument names a handler, it is not evaluated
                let handler_ref = if (name == "horde" || (name == "vote" && args.len() == 3)) && !self.user_handler_takes(name, args.len()) {
                    match args.first().map(|a| &a.node) {
                        Some(Expr::FieldAccess { target, field }) => match &target.node {
                            Expr::Ident(c) if !env.contains_key(c) && self.cells.contains_key(c) => Some(format!("{}.{}", c, field)),
                            _ => None,
                        },
                        Some(Expr::Ident(h)) if !env.contains_key(h) && self.handler_arities.contains_key(h) => Some(h.clone()),
                        _ => None,
                    }
                } else { None };
                for (i, arg) in args.iter().enumerate() {
                    if i == 0 { if let Some(r) = &handler_ref { arg_vals.push(Value::String(r.clone())); continue; } }
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }

                self.eval_named_call(name, arg_vals, env, cell_name, signal_name)
            }

            Expr::Not(inner) => {
                let val = self.eval_expr(&inner.node, env, cell_name, signal_name)?;
                let b = val.as_bool().map_err(ExecError::Runtime)?;
                Ok(Value::Bool(!b))
            }

            Expr::ListLiteral(elements) => {
                let mut items = Vec::with_capacity(elements.len());
                for elem in elements {
                    items.push(self.eval_expr(&elem.node, env, cell_name, signal_name)?);
                }
                Ok(Value::List(items))
            }

            Expr::Record { type_name, fields } => {
                // Two cases:
                //   1. Struct-variant constructor: `Accepted { id, price }` where
                //      Accepted is a registered variant.
                //   2. Plain record literal: `User { name, age }`.
                if let Some((vtype, shape)) = self.variant_registry.get(type_name).cloned() {
                    if let VariantShape::Struct(expected) = shape {
                        let mut entries = IndexMap::new();
                        for (field_name, field_expr) in fields {
                            let val = self.eval_expr(&field_expr.node, env, cell_name, signal_name)?;
                            entries.insert(field_name.clone(), val);
                        }
                        // Soft check: warn if fields don't match the variant declaration.
                        for f in &expected {
                            if !entries.contains_key(f) {
                                return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                                    "variant '{}' missing field '{}'",
                                    type_name, f
                                ))));
                            }
                        }
                        // the declared field types: `Charged { tx: 1 }` for
                        // `tx: String` was built and handed around
                        let fields = self.coerce_variant_fields(&vtype, type_name, VariantValue::Struct(entries))
                            .map_err(|m| ExecError::Runtime(RuntimeError::TypeError(m)))?;
                        if let Err(m) = self.variant_ok(&vtype, type_name, &fields) {
                            return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: m }));
                        }
                        return Ok(Value::Variant {
                            type_name: vtype,
                            variant: type_name.clone(),
                            fields,
                        });
                    }
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!("variant '{}' does not have named fields", type_name))));
                }
                // Fall through to record literal.
                let mut entries = IndexMap::new();
                entries.insert("_type".to_string(), Value::String(type_name.clone()));
                for (field_name, field_expr) in fields {
                    let val = self.eval_expr(&field_expr.node, env, cell_name, signal_name)?;
                    entries.insert(field_name.clone(), val);
                }
                Ok(Value::Map(entries))
            }

            Expr::Try(inner) => {
                // try { expr } → returns map("value", result) or map("error", message)
                // savepoint: what a failing `try` block wrote is undone
                let savepoint = self.journal.as_ref().map(|j| (self.journal_gen, j.len()));
                let outcome = self.eval_expr(&inner.node, env, cell_name, signal_name);
                self.check_transaction()?;
                // a `[task]` step committed inside the try (a think()): what it
                // committed stays; everything of the current step is undone
                if let (Err(ExecError::Runtime(_)), Some((gen, mark))) = (&outcome, savepoint) {
                    self.rollback_savepoint(if gen == self.journal_gen { mark } else { 0 });
                }
                self.check_storage_write().map_err(|e| ExecError::Runtime(self.fail_transaction(e.to_string())))?;
                // runaway recursion is not caught: a `try` around a
                // self-applying lambda re-ran it at every level (100% CPU,
                // 940 MB, the server's lock held) — it fails the handler
                if matches!(outcome, Err(ExecError::Runtime(RuntimeError::StackOverflow | RuntimeError::StorageTransaction(_)))) {
                    return outcome;
                }
                match outcome {
                    Ok(val) => Ok(map_from_pairs(vec![
                        ("value".to_string(), val),
                        ("error".to_string(), Value::Unit),
                    ])),
                    // error: the message; kind: what to branch on
                    // ("not_found", "invalid_transition", "invariant", …);
                    // detail: the message without its kind. `fail(r)` re-raises.
                    Err(ExecError::Runtime(e)) => Ok(map_from_pairs(vec![
                        ("value".to_string(), Value::Unit),
                        ("error".to_string(), Value::String(format!("{}", e))),
                        ("kind".to_string(), Value::String(e.kind())),
                        ("detail".to_string(), Value::String(e.detail())),
                    ])),
                    Err(ExecError::Return(val)) => Err(ExecError::Return(val)),
                    Err(ExecError::Break) => Err(ExecError::Break),
                    Err(ExecError::Continue) => Err(ExecError::Continue),
                }
            }

            Expr::TryPropagate(inner) => {
                // expr? — evaluate inner, if result is a map with a non-unit error field, propagate it
                let val = self.eval_expr(&inner.node, env, cell_name, signal_name)?;
                match &val {
                    Value::Map(entries) => {
                        if let Some(err) = entries.get("error") {
                            if !matches!(err, Value::Unit) {
                                // Has an error — RE-RAISE it (kind and detail kept):
                                // returning the error map answered 200 and
                                // committed the handler's earlier writes
                                let kind = match entries.get("kind") { Some(Value::String(k)) => k.clone(), _ => "error".to_string() };
                                let detail = match entries.get("detail") { Some(Value::String(d)) => d.clone(), _ => format!("{}", err) };
                                return Err(ExecError::Runtime(RuntimeError::Domain { kind, message: detail }));
                            }
                        }
                        // No error — unwrap value
                        Ok(entries.get("value").cloned().unwrap_or(val))
                    }
                    _ => Ok(val) // Not a result map, return as-is
                }
            }

            Expr::Lambda { param, body } => {
                // Capture only the names the body mentions: capturing the
                // whole environment made `rows |> map(r => …)` copy `rows`
                // (20 000 maps) into the closure AND once per element (the
                // closure env is cloned for each call) — 104 s for 20k rows.
                let mut names: HashSet<String> = HashSet::new();
                free_names_expr(&body.node, &mut names);
                self.add_guard_names(&mut names);
                let captured: HashMap<String, Value> = env.iter()
                    .filter(|(k, _)| names.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone())).collect();
                Ok(Value::Lambda {
                    param: param.clone(),
                    body: body.clone(),
                    env: captured,
                })
            }

            Expr::LambdaBlock { param, stmts, result } => {
                // Capture current environment + statements (only the names used)
                let mut names: HashSet<String> = HashSet::new();
                free_names_stmts(stmts, &mut names);
                free_names_expr(&result.node, &mut names);
                self.add_guard_names(&mut names);
                let captured: HashMap<String, Value> = env.iter()
                    .filter(|(k, _)| names.contains(k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone())).collect();
                Ok(Value::LambdaBlock {
                    param: param.clone(),
                    stmts: stmts.clone(),
                    result: result.clone(),
                    env: captured,
                })
            }

            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                let cond_val = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                let (body, result) = if cond_truth(&cond_val)? {
                    (then_body, then_result)
                } else {
                    (else_body, else_result)
                };
                // Evaluate the result while branch locals still exist, then
                // restore shadowed names on success, failure or control flow.
                with_lexical_scope(body, env, |env| {
                    self.exec_body(body, env, cell_name, signal_name)?;
                    self.eval_expr(&result.node, env, cell_name, signal_name)
                })
            }

            Expr::Match { subject, arms } => {
                let val = self.eval_expr(&subject.node, env, cell_name, signal_name)?;
                for arm in arms {
                    let (matches, bindings) = self.match_pattern(&arm.pattern, &val);
                    if matches {
                        // the arm is a scope: its pattern bindings and `let`s
                        // hide outer names only inside it (they overwrote the
                        // outer variable — and a failed guard DELETED it)
                        let mut saved: Vec<(String, Option<Value>)> = Vec::new();
                        let remember = |name: &str, env: &Env, saved: &mut Vec<(String, Option<Value>)>| {
                            if !saved.iter().any(|(n, _)| n == name) { saved.push((name.to_string(), env.get(name).cloned())); }
                        };
                        for (name, _) in &bindings { remember(name, env, &mut saved); }
                        for stmt in &arm.body {
                            if let Statement::Let { name, .. } = &stmt.node { remember(name, env, &mut saved); }
                        }
                        let restore = |env: &mut Env, saved: Vec<(String, Option<Value>)>| {
                            for (name, prev) in saved {
                                match prev { Some(v) => { env.insert(name, v); } None => { env.remove(&name); } }
                            }
                        };
                        for (name, bound_val) in &bindings {
                            env.insert(name.clone(), bound_val.clone());
                        }
                        // Evaluate guard clause if present
                        if let Some(ref guard) = arm.guard {
                            let guard_val = match self.eval_expr(&guard.node, env, cell_name, signal_name) {
                                Ok(v) => v,
                                Err(e) => { restore(env, saved); return Err(e); }
                            };
                            let ok = match guard_val.as_bool() {
                                Ok(b) => b,
                                Err(e) => { restore(env, saved); return Err(ExecError::Runtime(e)); }
                            };
                            if !ok {
                                restore(env, saved);
                                continue;
                            }
                        }
                        let outcome = (|| -> Result<Value, ExecError> {
                            // Execute body statements, capturing last value
                            let mut last_val = Value::Unit;
                            for stmt in &arm.body {
                                self.last_span = Some(stmt.span);
                                last_val = self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
                            }
                            // If the result expression is Unit (parser couldn't extract it),
                            // use the last body statement's value instead
                            if matches!(arm.result.node, Expr::Literal(Literal::Unit)) && !arm.body.is_empty() {
                                return Ok(last_val);
                            }
                            self.eval_expr(&arm.result.node, env, cell_name, signal_name)
                        })();
                        restore(env, saved);
                        return outcome;
                    }
                }
                // No match found. For sum-type variants this is a hole the
                // checker also rejects — enforce it at runtime instead of
                // silently producing Unit.
                if let Value::Variant { type_name, variant, .. } = &val {
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "non-exhaustive match on '{}': variant '{}' not handled", type_name, variant
                    ))));
                }
                // every arm is a variant pattern and the subject is not a
                // variant at all: `()` would hide a wrong value (a plain map
                // where a Pay was expected)
                if !arms.is_empty() && arms.iter().all(|a| matches!(a.pattern, MatchPattern::Variant { .. })) {
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "match expects a variant ({}), got {} {} — no arm can match it",
                        arms.iter().map(|a| match &a.pattern { MatchPattern::Variant { name, .. } => name.as_str(), _ => "?" }).collect::<Vec<_>>().join(" / "),
                        value_type_name(&val), { let t: String = format!("{}", val).chars().take(40).collect(); t }
                    ))));
                }
                Ok(Value::Unit)
            }

            Expr::Pipe { left, right } => {
                // Evaluate left side
                let left_val = self.eval_expr(&left.node, env, cell_name, signal_name)?;

                // Right side must be a FnCall — prepend left_val as first arg
                self.last_span = Some(right.span);
                match &right.node {
                    Expr::FnCall { name, args } => {
                        let mut all_args = vec![left_val];
                        for arg in args {
                            all_args.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                        }
                        if !self.handler_shadows_builtin(name, all_args.len(), cell_name)
                            && !env.contains_key(name) && name == "map" && all_args.len() == 2
                            && matches!(all_args[0], Value::String(_))
                            && matches!(all_args[1], Value::Lambda { .. } | Value::LambdaBlock { .. }) {
                            return Err(ExecError::Runtime(RuntimeError::TypeError("map(list, f) needs a List, got String".to_string())));
                        }
                        self.eval_named_call(name, all_args, env, cell_name, signal_name)
                    }
                    Expr::Ident(name) => {
                        self.eval_named_call(name, vec![left_val], env, cell_name, signal_name)
                    }
                    Expr::FieldAccess { target, field } => {
                        // expr |> obj.method → method call with pipe value
                        let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                        self.call_storage_method(cell_name, &format!("{}", target_val), field, &[left_val])
                    }
                    _ => Err(ExecError::Runtime(RuntimeError::TypeError(
                        "pipe (|>) right side must be a function call".to_string()
                    )))
                }
            }

            Expr::FieldAccess { target, field } => {
                // Check if target is an ident referring to a storage slot
                if let Expr::Ident(ref slot_name) = target.node {
                    // A local binding shadows a slot of the same name — in an
                    // invariant the slot name IS bound, to the value being
                    // written, so `invariant accts.balance >= 0` reads that
                    // record's field (it used to call a slot method named
                    // `balance` and reject every write). Otherwise: storage.
                    if !env.contains_key(slot_name)
                        && (self.storage.contains_key(slot_name)
                            || self.storage.contains_key(&format!("{}.{}", cell_name, slot_name)))
                    {
                        return self.call_storage_method(cell_name, slot_name, field, &[]);
                    }
                    // a local: read the field in place (`rows.len` copied the
                    // whole list per read; `big_record.x` the whole record)
                    match env.get(slot_name) {
                        Some(Value::List(xs)) if matches!(field.as_str(), "len" | "length" | "size") =>
                            return Ok(Value::Int(SomaInt::from_i64(xs.len() as i64))),
                        Some(Value::Map(m)) if m.contains_key(field) => return Ok(m[field].clone()),
                        _ => {}
                    }
                }
                // Evaluate target and access field on the value
                let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                match target_val {
                    Value::Map(ref entries) => {
                        // a real key wins over the pseudo-fields: `r.len` on
                        // map("len", 351) answered 2 (the entry count)
                        if let Some(v) = entries.get(field) {
                            return Ok(v.clone());
                        }
                        // a record or variant has declared fields: a missing
                        // `size` is absent data, not the entry count
                        let is_record = entries.contains_key("_type") || entries.contains_key("_variant");
                        if is_record {
                            return Ok(Value::Unit);
                        }
                        match field.as_str() {
                            "keys" => return Ok(Value::List(entries.keys().map(|k| Value::String(k.clone())).collect())),
                            "values" => return Ok(Value::List(entries.values().cloned().collect())),
                            "length" | "len" | "size" => return Ok(Value::Int(SomaInt::from_i64(entries.len() as i64))),
                            _ => {}
                        }
                        Ok(Value::Unit)
                    }
                    Value::List(ref items) => {
                        // list.length, list.len, list.first, list.last
                        match field.as_str() {
                            "length" | "len" | "size" => Ok(Value::Int(SomaInt::from_i64(items.len() as i64))),
                            "first" => Ok(items.first().cloned().unwrap_or(Value::Unit)),
                            "last" => Ok(items.last().cloned().unwrap_or(Value::Unit)),
                            // matrix pseudo-fields (parens-free): m.T, m.shape
                            "T" | "transpose" if builtins::linalg::is_matrix(&target_val) => {
                                builtins::linalg::call_builtin("transpose", std::slice::from_ref(&target_val))
                                    .unwrap_or(Ok(Value::Unit)).map_err(ExecError::Runtime)
                            }
                            "shape" => {
                                builtins::linalg::call_builtin("shape", std::slice::from_ref(&target_val))
                                    .unwrap_or(Ok(Value::Unit)).map_err(ExecError::Runtime)
                            }
                            _ => {
                                // Try numeric index
                                if let Ok(idx) = field.parse::<usize>() {
                                    Ok(items.get(idx).cloned().unwrap_or(Value::Unit))
                                } else {
                                    // `body.worker` on a JSON body that was a list
                                    // read () and every `??` default applied
                                    Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                                        "cannot read field '{}' of List {} — it is not a map or a record (a List has .len, .first, .last)", field, short_value(&target_val)))))
                                }
                            }
                        }
                    }
                    Value::String(ref s) => {
                        match field.as_str() {
                            "length" | "len" | "size" => Ok(Value::Int(SomaInt::from_i64(s.chars().count() as i64))),
                            // a JSON body `"str"` sent to a Map-shaped handler
                            // passed with every field defaulted by `??`
                            _ => Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                                "cannot read field '{}' of String {} — it is not a map or a record (from_json(s) parses JSON text)", field, short_value(&target_val))))),
                        }
                    }
                    // a variant with named fields reads them by name, like a
                    // record (`l.qty` on `Line { sku, qty }`)
                    Value::Variant { fields: VariantValue::Struct(ref fs), ref variant, .. } => match fs.get(field.as_str()) {
                        Some(v) => Ok(v.clone()),
                        None => Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                            "variant {} has no field '{}' (fields: {})", variant, field, fs.keys().cloned().collect::<Vec<_>>().join(", "))))),
                    },
                    _ => Err(ExecError::Runtime(RuntimeError::TypeError(
                        format!("cannot read field '{}' of {} {} — it is not a map or a record", field, value_type_name(&target_val), short_value(&target_val)),
                    )))
                }
            }

            Expr::Index { target, index } => {
                // `xs[i]` (list, by position), `m[k]` (map, by key),
                // `s[i]` (string, by char). A storage-slot index reads
                // through .get() so `slot[k]` works like slot.get(k).
                if let Expr::Ident(ref slot_name) = target.node {
                    // a local (or parameter, loop variable, lambda parameter)
                    // of the slot's name wins, as for every other read — but in
                    // an invariant `slot[k]` is the stored value, like slot.get(k)
                    let in_invariant = matches!(env.get(INV_SLOT), Some(Value::String(s)) if s == slot_name);
                    if (in_invariant || !env.contains_key(slot_name)) && (self.storage.contains_key(slot_name)
                        || self.storage.contains_key(&format!("{}.{}", cell_name, slot_name)))
                    {
                        let key = self.eval_expr(&index.node, env, cell_name, signal_name)?;
                        // `rows[i]` on a List slot raises like `xs[i]` on a list
                        // (it answered () out of range, and for `rows["1"]`)
                        if self.slot_kind(cell_name, slot_name) == Some("List") {
                            let backend = self.storage.get(&format!("{}.{}", cell_name, slot_name)).or_else(|| if cell_name.is_empty() || self.is_test_cell(cell_name) { self.storage.get(slot_name.as_str()) } else { None }).cloned();
                            if let Some(b) = backend {
                                let len = b.list_len();
                                self.check_storage_write()?;
                                let i = list_position(&key, len, "list").map_err(ExecError::Runtime)?;
                                let value = b.list_get(i).map(|v| self.from_slot(cell_name, slot_name, stored_to_value(v))).unwrap_or(Value::Unit);
                                self.check_storage_write()?;
                                return Ok(value);
                            }
                        }
                        return self.call_storage_method(cell_name, slot_name, "get", &[key]);
                    }
                }
                let idx_val = self.eval_expr(&index.node, env, cell_name, signal_name)?;
                // `xs[i]` on a local: index in place — evaluating `xs` cloned
                // the whole list per read (a `while i < n { xs[i] }` loop
                // over 20k rows took 112 s)
                if let Expr::Ident(ref name) = target.node {
                    match env.get(name) {
                        Some(Value::List(items)) => {
                            let i = list_position(&idx_val, items.len(), "list").map_err(ExecError::Runtime)?;
                            return Ok(items[i].clone());
                        }
                        Some(Value::Map(entries)) => {
                            let key = format!("{}", idx_val);
                            return Ok(entries.get(&key).cloned().unwrap_or(Value::Unit));
                        }
                        _ => {}
                    }
                }
                let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                // a record's field by name (`l["qty"]`, and `l.qty += 1`)
                if let (Value::Variant { variant, fields: VariantValue::Struct(fs), .. }, Value::String(k)) = (&target_val, &idx_val) {
                    return match fs.get(k) {
                        Some(v) => Ok(v.clone()),
                        None => Err(ExecError::Runtime(RuntimeError::TypeError(format!("{} has no field '{}' (fields: {})", variant, k, fs.keys().map(|x| x.as_str()).collect::<Vec<_>>().join(", "))))),
                    };
                }
                match target_val {
                    Value::List(ref items) => {
                        let i = list_position(&idx_val, items.len(), "list").map_err(ExecError::Runtime)?;
                        Ok(items[i].clone())
                    }
                    Value::Map(ref entries) => {
                        let key = format!("{}", idx_val);
                        Ok(entries.get(&key).cloned().unwrap_or(Value::Unit))
                    }
                    Value::String(ref s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let i = list_position(&idx_val, chars.len(), "string").map_err(ExecError::Runtime)?;
                        Ok(Value::String(chars[i].to_string()))
                    }
                    other => Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "cannot index {} with [{}]", value_type_name(&other), idx_val
                    )))),
                }
            }

            Expr::MethodCall { target, method, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }
                // Check if target is a storage slot
                if let Expr::Ident(ref slot_name) = target.node {
                    // inside an invariant the slot's name is bound to the value
                    // being written, but `slot.get(key)` is the STORED value
                    // (a Map-valued slot read the new value's field `key`,
                    // so a write-once audit log could be rewritten)
                    if matches!(env.get(INV_SLOT), Some(Value::String(s)) if s == slot_name)
                        && matches!(method.as_str(), "get" | "has" | "contains" | "contains_key")
                    {
                        return self.call_storage_method(cell_name, slot_name, method, &arg_vals);
                    }
                    if !env.contains_key(slot_name) && (self.storage.contains_key(slot_name)
                        || self.storage.contains_key(&format!("{}.{}", cell_name, slot_name)))
                    {
                        return self.call_storage_method(cell_name, slot_name, method, &arg_vals);
                    }
                    // `Ledger.deposit(a, n)`: a call into another cell's handler
                    if !env.contains_key(slot_name) && self.cells.contains_key(slot_name) {
                        return self.call_cell_handler(slot_name, method, arg_vals);
                    }
                }
                // Evaluate target and call method on the value
                let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                // `"hello".map(f)`: the higher-order map, not the constructor
                if method == "map" && arg_vals.len() == 1 && matches!(target_val, Value::String(_))
                    && matches!(arg_vals[0], Value::Lambda { .. } | Value::LambdaBlock { .. })
                    && !self.user_handler_takes("map", 2) {
                    let t: String = format!("{}", target_val).chars().take(30).collect();
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!("map(list, f) needs a List, got String {}", t))));
                }
                match (&target_val, method.as_str()) {
                    (Value::List(items), "get") => {
                        if let Some(Value::Int(si)) = arg_vals.first() {
                            // an Int past 64 bits is out of bounds, not element 0
                            Ok(si.to_i64().and_then(|i| items.get(i as usize)).cloned().unwrap_or(Value::Unit))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    (Value::List(items), "len" | "length") => {
                        Ok(Value::Int(SomaInt::from_i64(items.len() as i64)))
                    }
                    (Value::Map(entries), "get") => {
                        if let Some(key) = arg_vals.first() {
                            let key_str = format!("{}", key);
                            Ok(entries.get(&key_str).cloned().unwrap_or(Value::Unit))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    (Value::Map(entries), "keys") => {
                        Ok(Value::List(entries.keys().map(|k| Value::String(k.clone())).collect()))
                    }
                    (Value::Map(entries), "values") => {
                        Ok(Value::List(entries.values().cloned().collect()))
                    }
                    (Value::Map(entries), "has") => {
                        if let Some(key) = arg_vals.first() {
                            let key_str = format!("{}", key);
                            Ok(Value::Bool(entries.contains_key(&key_str)))
                        } else {
                            Ok(Value::Bool(false))
                        }
                    }
                    (Value::String(s), "len" | "length") => Ok(Value::Int(SomaInt::from_i64(s.chars().count() as i64))),
                    (Value::String(s), "split") => {
                        if let Some(Value::String(delim)) = arg_vals.first() {
                            Ok(Value::List(s.split(delim.as_str()).map(|p| Value::String(p.to_string())).collect()))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    _ => {
                        // UFCS: x.method(a, b) → method(x, a, b) for any
                        // builtin. Makes matrices first-class — m.reshape(2,2),
                        // m.transpose(), m.shape(), m.det() — and lists too
                        // (xs.reverse(), xs.sum(), xs.sort()).
                        let mut ufcs = Vec::with_capacity(arg_vals.len() + 1);
                        ufcs.push(target_val.clone());
                        ufcs.extend(arg_vals.iter().cloned());
                        if ufcs.iter().any(|v| matches!(v, Value::Lambda { .. } | Value::LambdaBlock { .. })) {
                            if let Some(res) = builtins::call_lambda_builtin(self, method, &ufcs, cell_name) {
                                return res.map_err(ExecError::Runtime);
                            }
                        }
                        if let Some(res) = self.call_builtin(method, &ufcs, cell_name) {
                            return res.map_err(ExecError::Runtime);
                        }
                        // UFCS on a user handler: `(n + 1).dbl()` → dbl(n + 1)
                        // (check accepted it; the runtime said "no method")
                        let is_slot = matches!(&target.node, Expr::Ident(n) if self.slot_kind(cell_name, n).is_some() && !env.contains_key(n));
                        if !is_slot && self.user_handler_takes(method, ufcs.len()) {
                            let defines = |cn: &str| self.cells.get(cn).map_or(false, |c| c.sections.iter().any(|s| {
                                matches!(&s.node, Section::OnSignal(on) if on.signal_name == *method)
                            }));
                            let target_cell = if defines(cell_name) { Some(cell_name.to_string()) } else {
                                let mut all: Vec<&String> = self.cells.keys().filter(|cn| defines(cn)).collect();
                                all.sort_by_key(|c| self.cell_order.iter().position(|o| o == *c).unwrap_or(usize::MAX));
                                all.first().map(|c| (*c).clone())
                            };
                            if let Some(tc) = target_cell {
                                return self.call_signal(&tc, method, ufcs).map_err(ExecError::Runtime);
                            }
                        }
                        // Try storage as fallback
                        if let Expr::Ident(ref name) = target.node {
                            return self.call_storage_method(cell_name, name, method, &arg_vals);
                        }
                        Err(ExecError::Runtime(RuntimeError::TypeError(
                            format!("no method '{}' on {} {}", method, value_type_name(&target_val), { let t: String = format!("{}", target_val).chars().take(40).collect(); t }),
                        )))
                    }
                }
            }

        }
    }

    /// Dispatch a method call to a storage backend.
    /// Handles: get(key), set(key, val), delete(key), append(val), len(), list()
    /// A slot method, and then: did the database refuse the write? A
    /// read-only `.soma_data` (or a full disk) used to make every `set` a
    /// silent no-op answered 200 — the handler raises and rolls back.
    fn call_storage_method(
        &mut self,
        cell_name: &str,
        slot_name: &str,
        method: &str,
        args: &[Value],
    ) -> Result<Value, ExecError> {
        let r = self.call_storage_method_inner(cell_name, slot_name, method, args);
        self.check_storage_write()?;
        r
    }

    fn call_storage_method_inner(
        &mut self,
        cell_name: &str,
        slot_name: &str,
        method: &str,
        args: &[Value],
    ) -> Result<Value, ExecError> {
        let prefixed = format!("{}.{}", cell_name, slot_name);
        let backend = self.storage.get(&prefixed)
            // the bare name only without a cell (a test rule): from a cell it
            // reached ANOTHER cell's slot (`"{bal.set(id, -3)}"` skipped the
            // owner's invariant and the privacy rule)
            .or_else(|| if cell_name.is_empty() || self.is_test_cell(cell_name) { self.storage.get(slot_name) } else { None });

        let backend = match backend {
            // Arc clone: ends the borrow of self.storage so arms can call
            // &mut self methods (invariant evaluation) before writing.
            Some(b) => b.clone(),
            None => {
                return Err(ExecError::Runtime(RuntimeError::TypeError(
                    format!("'{}' is not a memory slot — `.{}()` is a slot method; a local map is written with brackets (`{}[k] = v`), a local list is rebuilt (`{} = push({}, x)`)", slot_name, method, slot_name, slot_name, slot_name),
                )));
            }
        };

        // Is this slot sharded across the cluster?
        let is_sharded = self.cluster.is_some()
            && self.sharded_slots.contains_key(&prefixed);

        // A List slot is its append log: len / values / get(i) / first /
        // last read the log (the keyed map underneath is empty, so `.len`
        // answered 0 after two pushes and `.get(0)` was `()`).
        if self.slot_kind(cell_name, slot_name) == Some("List") && !is_sharded {
            let items = || -> Vec<Value> {
                backend.list().into_iter().map(|v| self.from_slot(cell_name, slot_name, stored_to_value(v))).collect()
            };
            match method {
                "len" | "size" | "count" | "length" if args.is_empty() => return Ok(Value::Int(SomaInt::from_i64(backend.list_len() as i64))),
                // only the no-argument aliases: `rows.all(r => …)` is the
                // builtin over the content, not "give me everything" (it
                // returned the list, so `if rows.all(p)` was always taken)
                "values" | "all" | "list" | "entries" | "items" if args.is_empty() => return Ok(Value::List(items())),
                "first" if args.is_empty() => return Ok(items().into_iter().next().unwrap_or(Value::Unit)),
                "last" if args.is_empty() => return Ok(items().into_iter().last().unwrap_or(Value::Unit)),
                "get" | "at" | "nth" => {
                    if let Some(Value::Int(i)) = args.first() {
                        // an Int past 64 bits is out of bounds, not element 0
                        let Some(raw) = i.to_i64() else { return Ok(Value::Unit) };
                        let idx = if raw < 0 { raw + backend.list_len() as i64 } else { raw };
                        return Ok(if idx >= 0 {
                            backend.list_get(idx as usize).map(|v| self.from_slot(cell_name, slot_name, stored_to_value(v))).unwrap_or(Value::Unit)
                        } else { Value::Unit });
                    }
                }
                "has" | "contains" => {
                    if let Some(x) = args.first() {
                        return Ok(Value::Bool(items().iter().any(|v| deep_equal(v, x))));
                    }
                }
                // a List slot is indexed by position: rows.set("0", 7) wrote
                // into an invisible keyed table and was lost
                "set" | "put" if !matches!(args.first(), Some(Value::Int(_))) => {
                    return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "{}.set(): '{}' is a List slot — index it by an Int position (rows.set(0, v)), got {}",
                        slot_name, slot_name, args.first().map(value_type_name).unwrap_or("nothing")) }));
                }
                // rows[i] = v / rows.set(i, v): replace one element (was a
                // silent no-op — the keyed map under the log took the write)
                "set" | "put" if matches!(args.first(), Some(Value::Int(_))) && args.len() == 2 => {
                    // length and the one old element, never the whole list
                    // (a 20 000-row list was read three times and rewritten
                    // for every `rows[i] = v`)
                    let len = backend.list_len();
                    self.check_storage_write()?;
                    // an Int past 64 bits is out of bounds, not element 0
                    let raw = match &args[0] {
                        Value::Int(i) => match i.to_i64() {
                            Some(v) => v,
                            None => return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                                "{}[{}]: index out of bounds (the List slot has {} items) — push() appends", slot_name, i, len)))),
                        },
                        _ => 0,
                    };
                    let idx = if raw < 0 { raw + len as i64 } else { raw };
                    if idx < 0 || idx as usize >= len {
                        return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                            "{}[{}]: index out of bounds (the List slot has {} items) — push() appends", slot_name, raw, len))));
                    }
                    if self.slot_immutable(cell_name, slot_name) {
                        return Err(Self::immutable_refusal(slot_name, &format!("overwriting element #{}", idx)));
                    }
                    let val = self.check_slot_value_type(cell_name, slot_name, &args[1])?;
                    // the REAL index: `slots[-2] = v` was checked with key -2
                    // and got past `invariant key != 0 || value == 0`
                    self.check_invariants(cell_name, slot_name, &idx.to_string(), &val, len as i64, "write")?;
                    let prev = backend.list_get(idx as usize);
                    self.check_storage_write()?;
                    let Some(prev) = prev else {
                        return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                            "{}[{}]: index out of bounds (the List slot has {} items) — push() appends", slot_name, raw, len))));
                    };
                    if let Some(j) = self.journal.as_mut() {
                        j.push(UndoOp::ListSet { backend: backend.clone(), index: idx as usize, prev });
                    }
                    backend.list_set(idx as usize, value_to_stored(&val));
                    self.check_storage_write()?;
                    return Ok(Value::Unit);
                }
                // rows.delete(i): remove one element by index
                "delete" | "remove" if matches!(args.first(), Some(Value::Int(_))) => {
                    // the whole list only when an invariant needs the
                    // shifted elements (each delete read and rewrote every row)
                    let needs_all = self.slot_invariants_use_key(cell_name, slot_name);
                    let xs = if needs_all { items() } else { Vec::new() };
                    let len = if needs_all { xs.len() } else { backend.list_len() };
                    self.check_storage_write()?;
                    // an Int past 64 bits is out of range (a miss, like any
                    // other index past the end), not element 0
                    let raw = match &args[0] { Value::Int(i) => i.to_i64().unwrap_or(i64::MAX), _ => 0 };
                    let idx = if raw < 0 { raw + len as i64 } else { raw };
                    // an [immutable] slot refuses a delete, in range or not
                    // (out of range answered `false`, so a test could not
                    // tell a refusal from a miss)
                    if self.slot_immutable(cell_name, slot_name) {
                        return Err(Self::immutable_refusal(slot_name, &format!("deleting element #{}", idx)));
                    }
                    if idx < 0 || idx as usize >= len {
                        return Ok(Value::Bool(false));
                    }
                    let old = if needs_all { xs[idx as usize].clone() } else {
                        let got = backend.list_get(idx as usize);
                        self.check_storage_write()?;
                        match got { Some(v) => self.from_slot(cell_name, slot_name, stored_to_value(v)), None => return Ok(Value::Bool(false)) }
                    };
                    // only a `size` clause can flip on a delete — as for a Map slot,
                    // and as verify says (every value invariant refused the
                    // delete here while verify proved the writer safe)
                    if self.slot_invariants_cross(cell_name, slot_name) {
                        self.check_invariants(cell_name, slot_name, &idx.to_string(), &Value::Unit, len as i64 - 1, "delete_cross")?;
                    }
                    if self.slot_invariants_use_size(cell_name, slot_name) {
                        self.check_invariants(cell_name, slot_name, &idx.to_string(), &old, len as i64 - 1, "delete")?;
                    }
                    // a List delete SHIFTS the later elements to new indexes: an
                    // invariant about `key` (or the value at a key) is checked
                    // for each of them at its new place (`drop0` left 5 at #0
                    // under `key != 0 || value == 0`)
                    if needs_all {
                        for i in (idx as usize + 1)..xs.len() {
                            self.check_invariants(cell_name, slot_name, &(i - 1).to_string(), &xs[i], len as i64 - 1, "shift")?;
                        }
                    }
                    let removed = backend.list_remove(idx as usize);
                    self.check_storage_write()?;
                    let Some(token) = removed else { return Ok(Value::Bool(false)) };
                    if let Some(j) = self.journal.as_mut() {
                        j.push(UndoOp::ListRestore { backend: backend.clone(), token, prev: value_to_stored(&old) });
                    }
                    return Ok(Value::Bool(true));
                }
                _ => {}
            }
        }

        match method {
            "get" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "get() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);

                match backend.get(&key_str) {
                    Some(stored) => Ok(self.from_slot(cell_name, slot_name, stored_to_value(stored))),
                    None => Ok(Value::Unit),
                }
            }
            "set" | "put" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "set() requires key and value arguments".to_string()
                    )))?;
                let val = args.get(1)
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "set() requires key and value arguments".to_string()
                    )))?;
                // `__x` keys are the storage's own (hidden from len / keys /
                // the size invariant: `reg("__b")` grew a proven-bounded slot)
                if format!("{}", key).starts_with("__") {
                    return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "{}.set(\"{}\", …): keys starting with `__` are reserved by the storage — prefix user keys differently", slot_name, key) }));
                }
                // `()` became the key "null" (the same entry as "null"): a
                // mistyped field used as a key wrote silently
                if matches!(key, Value::Unit) {
                    return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "{}.set((), …): the key is () — an absent value (a mistyped field?) cannot be a key", slot_name) }));
                }
                let key_str = format!("{}", key);

                if self.slot_immutable(cell_name, slot_name) && backend.get(&key_str).is_some() {
                    return Err(Self::immutable_refusal(slot_name, &format!("rewriting key \"{}\"", key_str)));
                }
                let coerced = self.check_slot_value_type(cell_name, slot_name, val)?;
                let val = &coerced;
                // V1.8: invariants are checked BEFORE the write commits —
                // a violated invariant must leave the slot untouched.
                // (the COUNT is paid only by a slot that has invariants)
                if self.slot_has_invariants(cell_name, slot_name) {
                    // the COUNT only when an invariant reads `size` (it made
                    // every write O(slot size): a 50k-entry load took minutes)
                    let size_after = if self.slot_invariants_use_size(cell_name, slot_name) {
                        let exists = backend.get(&key_str).is_some();
                        backend.len() as i64 + if exists { 0 } else { 1 }
                    } else { 0 };
                    self.check_invariants(cell_name, slot_name, &key_str, val, size_after, "write")?;
                }

                // Write locally (journaled: a failing handler is rolled back)
                let prev = backend.get(&key_str);
                self.check_storage_write()?;
                if let Some(j) = self.journal.as_mut() {
                    j.push(UndoOp::Restore { backend: backend.clone(), key: key_str.clone(), prev });
                }
                backend.set(&key_str, value_to_stored(val));
                self.check_storage_write()?;

                // In cluster mode: broadcast to peers via EVENT bus
                if is_sharded {
                    self.cluster_write(&prefixed, &key_str, Some(value_to_stored(val)))?;
                }

                Ok(Value::Unit)
            }
            "delete" | "remove" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "delete() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);
                if self.slot_immutable(cell_name, slot_name) && backend.get(&key_str).is_some() {
                    return Err(Self::immutable_refusal(slot_name, &format!("deleting key \"{}\"", key_str)));
                }

                // Invariants are checked BEFORE the delete commits, like
                // set/push: `size` is the entry count after removal, and the
                // slot value is bound to the entry being removed (it already
                // satisfied the invariant when written, so only size/key
                // clauses can flip). Deleting a missing key is a no-op.
                // a rule between slots: the deleted side reads as () after
                if self.slot_invariants_cross(cell_name, slot_name) && backend.get(&key_str).is_some() {
                    let size_after = backend.len() as i64 - 1;
                    self.check_invariants(cell_name, slot_name, &key_str, &Value::Unit, size_after, "delete_cross")?;
                }
                if !self.slot_has_invariants(cell_name, slot_name) || !self.slot_invariants_use_size(cell_name, slot_name) {
                    // (a delete can only flip a `size` clause)
                } else if let Some(stored) = backend.get(&key_str) {
                    let old = self.from_slot(cell_name, slot_name, stored_to_value(stored));
                    let size_after = backend.len() as i64 - 1;
                    self.check_invariants(cell_name, slot_name, &key_str, &old, size_after, "delete")?;
                }

                if let Some(j) = self.journal.as_mut() {
                    if let Some(prev) = backend.get(&key_str) {
                        j.push(UndoOp::Restore { backend: backend.clone(), key: key_str.clone(), prev: Some(prev) });
                    }
                }
                let removed = backend.delete(&key_str);
                self.check_storage_write()?;

                // Broadcast delete to cluster
                if is_sharded {
                    self.cluster_write(&prefixed, &key_str, None)?;
                }
                Ok(Value::Bool(removed))
            }
            "append" | "push" => {
                // a Map slot has keys, not positions: push() wrote rows it
                // never reads back (the value vanished)
                if self.slot_kind(cell_name, slot_name) == Some("Map") {
                    return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "{}.{}(): '{}' is a Map slot — write it by key ({}.set(key, value)), or declare it List<…>", slot_name, method, slot_name, slot_name) }));
                }
                let val = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "append() requires a value argument".to_string()
                    )))?;
                let coerced = self.check_slot_value_type(cell_name, slot_name, val)?;
                let val = &coerced;
                // a List lives in the log table: `len()` counts the MAP rows
                // (0 on SQLite), so `rows.size <= N` never refused a push
                if self.slot_has_invariants(cell_name, slot_name) {
                    let size_after = if self.slot_invariants_use_size(cell_name, slot_name) { backend.list_len() as i64 + 1 } else { 0 };
                    // the pushed element's index is the length before the push
                    let new_index = backend.list_len().to_string();
                    self.check_invariants(cell_name, slot_name, &new_index, val, size_after, "write")?;
                }
                backend.append(value_to_stored(val));
                self.check_storage_write()?;
                if let Some(j) = self.journal.as_mut() {
                    j.push(UndoOp::Unappend { backend: backend.clone() });
                }
                Ok(Value::Unit)
            }
            // `length` counts on a local Map / List too (it read as a missing field here)
            // with an argument it is the builtin: `m.count(e => …)` counted
            // the ENTRIES, whatever the predicate said
            "len" | "size" | "count" | "length" if args.is_empty() => {
                Ok(Value::Int(SomaInt::from_i64(backend.len() as i64)))
            }
            "list" | "all" if args.is_empty() => {
                let items = backend.list();
                Ok(Value::List(items.into_iter().map(stored_to_value).collect()))
            }
            // like entries(m): {key, value} records, sorted by key (it
            // returned the bare values)
            "entries" | "items" if args.is_empty() => {
                let mut out = Vec::new();
                for k in backend.keys() {
                    if let Some(v) = backend.get(&k) {
                        out.push(map_from_pairs(vec![
                            ("key".to_string(), Value::String(k)),
                            ("value".to_string(), self.from_slot(cell_name, slot_name, stored_to_value(v))),
                        ]));
                    }
                }
                Ok(Value::List(out))
            }
            "keys" => {
                let keys = backend.keys();
                Ok(Value::List(keys.into_iter().map(Value::String).collect()))
            }
            "values" => {
                // Full replicas: values/keys/len/get read the same local
                // snapshot. Fan-out duplicated keys and mixed handler states.
                let vals = backend.values();
                Ok(Value::List(vals.into_iter().map(stored_to_value).collect()))
            }
            "has" | "contains" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "has() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);
                Ok(Value::Bool(backend.has(&key_str)))
            }
            "backend" => {
                if is_sharded {
                    Ok(Value::String("cluster".to_string()))
                } else {
                    Ok(Value::String(backend.backend_name().to_string()))
                }
            }
            // A bare `slot.field` (field access, no args) reads the key —
            // symmetric with `slot.field = v` and with map field access.
            _ if args.is_empty() => {
                match backend.get(method) {
                    Some(stored) => Ok(self.from_slot(cell_name, slot_name, stored_to_value(stored))),
                    None => Ok(Value::Unit),
                }
            }
            _ => {
                // UFCS reaches slots too: `rows.any(r => …)` is
                // `any(rows, r => …)` over the slot's whole content
                if let Some(whole) = self.materialize_slot(cell_name, slot_name) {
                    let mut all = vec![whole];
                    all.extend(args.iter().cloned());
                    if let Some(r) = builtins::call_lambda_builtin(self, method, &all, cell_name) {
                        return r.map_err(ExecError::Runtime);
                    }
                    if let Some(r) = self.call_builtin(method, &all, cell_name) {
                        return r.map_err(ExecError::Runtime);
                    }
                }
                Err(ExecError::Runtime(RuntimeError::TypeError(
                    format!("unknown method '{}' on memory slot '{}' — slot methods are get/set/delete/push/keys/values/len, and any builtin taking the slot's content first (`{}.{}(…)` = `{}({}, …)`)", method, slot_name, slot_name, method, method, slot_name),
                )))
            }
        }
    }

    // ── Cluster storage helpers ──────────────────────────────────────

    fn cluster_write(&mut self, slot: &str, key: &str, value: Option<StoredValue>) -> Result<(), ExecError> {
        if key.starts_with("__") {
            return Err(ExecError::Runtime(RuntimeError::TypeError("cluster keys beginning with __ are reserved".into())));
        }
        let Some(cluster) = self.cluster.clone() else { return Ok(()); };
        let Some(meta) = cluster.metadata.get(slot).cloned() else { return Ok(()); };
        let version = cluster.next_version().map_err(|e| ExecError::Runtime(RuntimeError::TypeError(e)))?;
        let update = crate::runtime::cluster::Update {
            slot: slot.into(), key: key.into(), version, deleted: value.is_none(),
            value: value.as_ref().map(crate::runtime::storage::stored_to_json).unwrap_or(serde_json::Value::Null),
        };
        if crate::runtime::cluster::Frame::Update(update.clone()).encode().len() as u64 > BUS_MAX_LINE {
            return Err(ExecError::Runtime(RuntimeError::TypeError("cluster update exceeds the 16 MiB bus line limit".into())));
        }
        if let Some(j) = self.journal.as_mut() {
            j.push(UndoOp::Restore { backend: meta.clone(), key: key.into(), prev: meta.get(key) });
            j.push(UndoOp::Cluster(update.clone()));
        }
        meta.set(key, StoredValue::String(serde_json::to_string(&update).unwrap()));
        if self.journal.is_none() { cluster.broadcast(crate::runtime::cluster::Frame::Update(update)); }
        Ok(())
    }

    /// Apply a newer remote register under the same transaction and handler
    /// lock as local writes. Replication never bypasses slot/type boundaries.
    pub(crate) fn apply_cluster_update(&mut self, update: crate::runtime::cluster::Update) -> Result<(), ExecError> {
        self.atomically(|me| {
            let Some(cluster) = me.cluster.clone() else { return Ok(()); };
            let Some(meta) = cluster.metadata.get(&update.slot).cloned() else {
                return Err(ExecError::Runtime(RuntimeError::TypeError("cluster update targets a slot not selected by scale.shard".into())));
            };
            if update.key.starts_with("__") || update.version.0 == 0 || !crate::runtime::cluster::valid_node_id(&update.version.1) {
                return Err(ExecError::Runtime(RuntimeError::TypeError("invalid cluster update key/version".into())));
            }
            if crate::runtime::cluster::ClusterNode::read_update(&*meta, &update.key).map_or(false, |old| old.version >= update.version) {
                return Ok(());
            }
            let (cell, slot) = update.slot.split_once('.').ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError("cluster slot must be Cell.slot".into())))?;
            let backend = me.storage.get(&update.slot).cloned().ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError("unknown cluster slot".into())))?;
            let value = if update.deleted { None } else {
                let value = stored_to_value(crate::runtime::storage::json_to_stored(&update.value));
                Some(value_to_stored(&me.check_slot_value_type(cell, slot, &value)?))
            };
            if let Some(j) = me.journal.as_mut() {
                j.push(UndoOp::Restore { backend: backend.clone(), key: update.key.clone(), prev: backend.get(&update.key) });
                j.push(UndoOp::Restore { backend: meta.clone(), key: update.key.clone(), prev: meta.get(&update.key) });
            }
            match value { Some(v) => backend.set(&update.key, v), None => { backend.delete(&update.key); } }
            me.check_storage_write()?;
            meta.set(&update.key, StoredValue::String(serde_json::to_string(&update).unwrap()));
            cluster.observe(&update.version);
            Ok(())
        })
    }

    /// Evaluate a constraint expression, returning true/false
    fn eval_constraint(
        &mut self,
        constraint: &Constraint,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<bool, ExecError> {
        match constraint {
            Constraint::Comparison { left, op, right } => {
                let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                let result = self.eval_cmpop(&l, *op, &r).map_err(ExecError::Runtime)?;
                result.as_bool().map_err(ExecError::Runtime)
            }
            Constraint::Predicate { name, args: _ } => {
                // Evaluate as a boolean expression
                if let Some(val) = env.get(name) {
                    return val.as_bool().map_err(ExecError::Runtime);
                }
                // Unknown predicates pass
                Ok(true)
            }
            Constraint::And(a, b) => {
                let ra = self.eval_constraint(&a.node, env, cell_name, signal_name)?;
                if !ra { return Ok(false); }
                self.eval_constraint(&b.node, env, cell_name, signal_name)
            }
            Constraint::Or(a, b) => {
                let ra = self.eval_constraint(&a.node, env, cell_name, signal_name)?;
                if ra { return Ok(true); }
                self.eval_constraint(&b.node, env, cell_name, signal_name)
            }
            Constraint::Not(inner) => {
                let r = self.eval_constraint(&inner.node, env, cell_name, signal_name)?;
                Ok(!r)
            }
            Constraint::Descriptive(_) => Ok(true),
        }
    }

    /// Interpolate {var} in a string from the local scope.
    /// If var is not found in scope, leave {var} as-is (for render() compatibility).
    fn interpolate_string(&mut self, s: &str, env: &mut Env, cell_name: &str, signal_name: &str) -> Result<String, ExecError> {
        let mut result = String::with_capacity(s.len());
        let mut pos = 0;
        // `{` kept as literal text (JSON, CSS) still open: the `}` that
        // closes one is text too, never half of a `}}` escape
        // (`"{\"a\": {\"b\": 1}}"` printed `{"a": {"b": 1}`)
        let mut literal_open = 0usize;
        while pos < s.len() {
            let byte = s.as_bytes()[pos];
            if byte == b'}' && literal_open > 0 {
                literal_open -= 1;
                result.push('}');
                pos += 1;
                continue;
            }
            // {{ and }} escape literal braces
            if byte == b'{' && s.as_bytes().get(pos + 1) == Some(&b'{') {
                result.push('{');
                pos += 2;
                continue;
            }
            if byte == b'}' && s.as_bytes().get(pos + 1) == Some(&b'}') {
                result.push('}');
                pos += 2;
                continue;
            }
            if byte == b'{' {
                if let Some(end) = interp_segment_end(s, pos) {
                    let expr_str = &s[pos + 1..pos + 1 + end];

                    // The analyses and closure capture use this same rule.
                    if interp_segment_is_literal(expr_str) {
                        result.push('{');
                        literal_open += 1;
                        pos += 1;
                        continue;
                    }

                    match self.eval_interpolation_expr(expr_str, env, cell_name, signal_name) {
                        InterpResult::Value(val) => {
                            result.push_str(&format!("{}", val));
                            pos = pos + 1 + end + 1;
                            continue;
                        }
                        // Not parseable as an expression — treat the brace as literal text
                        InterpResult::NotAnExpr => {
                            result.push('{');
                            literal_open += 1;
                            pos += 1;
                            continue;
                        }
                        InterpResult::Err(e) => return Err(e),
                    }
                }
            }
            // Properly handle multi-byte UTF-8 characters
            let ch = &s[pos..];
            if let Some(c) = ch.chars().next() {
                result.push(c);
                pos += c.len_utf8();
            } else {
                pos += 1;
            }
        }
        Ok(result)
    }

    /// Parse and evaluate an expression string from interpolation.
    /// Evaluation failures propagate as real errors — they must not be
    /// silently embedded in the output string.
    fn eval_interpolation_expr(&mut self, expr_str: &str, env: &mut Env, cell_name: &str, signal_name: &str) -> InterpResult {
        // Fast path: simple variable name — an identifier, never a number
        // (`"{0x1F}"`, `"{1e3}"` and `"{42}"` were looked up as variables
        // and raised "undefined variable" after a clean check)
        if expr_str.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !expr_str.chars().next().map_or(true, |c| c.is_ascii_digit())
        {
            if let Some(val) = env.get(expr_str) { return InterpResult::Value(val.clone()); }
            // `"{true}"` passed check and raised "undefined variable: true"
            match expr_str { "true" => return InterpResult::Value(Value::Bool(true)), "false" => return InterpResult::Value(Value::Bool(false)), _ => {} }
            // a memory slot, a variant, a state name: as in code (`"{counts}"`
            // raised "undefined variable" after a clean check)
            return match self.eval_expr(&Expr::Ident(expr_str.to_string()), env, cell_name, signal_name) {
                Ok(v) => InterpResult::Value(v),
                Err(e) => InterpResult::Err(e),
            };
        }
        // Fast path: var.field on a map (anything else falls through to the full evaluator)
        if expr_str.contains('.') && !expr_str.contains('(') && !expr_str.contains(' ') {
            let parts: Vec<&str> = expr_str.splitn(2, '.').collect();
            if parts.len() == 2 {
                if let Some(Value::Map(ref entries)) = env.get(parts[0]) {
                    if let Some(field_val) = entries.get(parts[1]) {
                        return InterpResult::Value(field_val.clone());
                    }
                }
            }
        }
        // Full expression: parse and eval with the regular evaluator
        let wrapped = format!("cell _T {{ on _e() {{ return {} }} }}", expr_str);
        let mut lexer = crate::lexer::Lexer::new(&wrapped);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(_) => return InterpResult::NotAnExpr,
        };
        let mut parser = crate::parser::Parser::new(tokens);
        let program = match parser.parse_program() {
            Ok(p) => p,
            Err(_) => return InterpResult::NotAnExpr,
        };
        let cell = match program.cells.first() {
            Some(c) => c,
            None => return InterpResult::NotAnExpr,
        };
        let section = match cell.node.sections.first() {
            Some(s) => s,
            None => return InterpResult::NotAnExpr,
        };
        if let crate::ast::Section::OnSignal(ref on) = section.node {
            // `{total - fee junk}`: ONE expression — the trailing `junk` was
            // dropped silently (and never checked)
            if on.body.len() > 1 {
                return InterpResult::Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                    "string interpolation `{{{}}}` is not one expression — the part after it would be dropped; bind it with a let first", expr_str))));
            }
            if let Some(stmt) = on.body.first() {
                if let crate::ast::Statement::Return { ref value } = stmt.node {
                    return match self.eval_expr(&value.node, env, cell_name, signal_name) {
                        Ok(val) => InterpResult::Value(val),
                        Err(e) => InterpResult::Err(e),
                    };
                }
            }
        }
        InterpResult::NotAnExpr
    }

    fn eval_literal(&self, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(SomaInt::from_i64(*n)),
            Literal::BigInt(s) => {
                match s.parse::<rug::Integer>() {
                    Ok(r) => Value::Int(SomaInt::from_rug(r)),
                    Err(_) => Value::Int(SomaInt::from_i64(0)),
                }
            }
            Literal::Float(n) => Value::Float(*n),
            Literal::String(s) => Value::String(s.clone()),
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Duration(d) => {
                let ms = match d.unit {
                    DurationUnit::Milliseconds => d.value,
                    DurationUnit::Seconds => d.value * 1000.0,
                    DurationUnit::Minutes => d.value * 60_000.0,
                    DurationUnit::Hours => d.value * 3_600_000.0,
                    DurationUnit::Days => d.value * 86_400_000.0,
                    DurationUnit::Years => d.value * 365.25 * 86_400_000.0,
                };
                Value::Int(SomaInt::from_i64(ms as i64))
            }
            Literal::Percentage(p) => Value::Float(*p),
            Literal::Unit => Value::Unit,
        }
    }

    /// Match a pattern against a value, returning (matches, bindings)
    fn match_pattern(&self, pattern: &MatchPattern, val: &Value) -> (bool, Vec<(String, Value)>) {
        match pattern {
            MatchPattern::Wildcard => (true, vec![]),
            MatchPattern::Literal(lit) => {
                let lit_val = self.eval_literal(lit);
                (self.values_equal(val, &lit_val), vec![])
            }
            MatchPattern::Variable(name) => {
                (true, vec![(name.clone(), val.clone())])
            }
            MatchPattern::Or(alternatives) => {
                for alt in alternatives {
                    let (m, bindings) = self.match_pattern(alt, val);
                    if m { return (true, bindings); }
                }
                (false, vec![])
            }
            MatchPattern::MapDestructure(fields) => {
                if let Value::Map(entries) = val {
                    let mut bindings = Vec::new();
                    for (field_name, sub_pattern) in fields {
                        // a key the map lacks does not match: `{kind} -> …`
                        // used to bind kind = () for ANY map, so the arm
                        // after it was unreachable and the body failed later
                        // ("cannot add String and Unit")
                        let Some(field_val) = entries.get(field_name) else { return (false, vec![]); };
                        let (m, sub_bindings) = self.match_pattern(sub_pattern, field_val);
                        if !m { return (false, vec![]); }
                        bindings.extend(sub_bindings);
                    }
                    (true, bindings)
                } else {
                    (false, vec![])
                }
            }
            MatchPattern::StringPrefix { prefix, rest } => {
                if let Value::String(s) = val {
                    if s.starts_with(prefix.as_str()) {
                        let remainder = s[prefix.len()..].to_string();
                        (true, vec![(rest.clone(), Value::String(remainder))])
                    } else {
                        (false, vec![])
                    }
                } else {
                    (false, vec![])
                }
            }
            MatchPattern::Range { from, to } => {
                match val {
                    Value::Int(si) => {
                        if let Some(i) = si.to_i64() {
                            (*from <= i && i <= *to, vec![])
                        } else {
                            (false, vec![])
                        }
                    }
                    Value::Float(f) => ((*from as f64) <= *f && *f <= (*to as f64), vec![]),
                    _ => (false, vec![]),
                }
            }
            MatchPattern::Variant { type_name, name, fields } => {
                if let Value::Variant { type_name: vt, variant, fields: vfields } = val {
                    // Type-qualifier (if present) must match.
                    if let Some(t) = type_name {
                        if t != vt { return (false, vec![]); }
                    }
                    if variant != name { return (false, vec![]); }
                    match (fields, vfields) {
                        (VariantPatternFields::Unit, VariantValue::Unit) => (true, vec![]),
                        (VariantPatternFields::Tuple(subs), VariantValue::Tuple(vs))
                            if subs.len() == vs.len() =>
                        {
                            let mut bindings = Vec::new();
                            for (sub, v) in subs.iter().zip(vs.iter()) {
                                let (m, sub_b) = self.match_pattern(sub, v);
                                if !m { return (false, vec![]); }
                                bindings.extend(sub_b);
                            }
                            (true, bindings)
                        }
                        (VariantPatternFields::Struct { fields: pf, rest }, VariantValue::Struct(entries)) => {
                            // Each named field in the pattern must match the variant's field.
                            // `rest = true` allows unmatched fields in the value.
                            let mut bindings = Vec::new();
                            for (fname, sub) in pf {
                                let fv = match entries.get(fname) {
                                    Some(v) => v.clone(),
                                    None => return (false, vec![]),
                                };
                                let (m, sub_b) = self.match_pattern(sub, &fv);
                                if !m { return (false, vec![]); }
                                bindings.extend(sub_b);
                            }
                            // Without `..` the pattern fields must be exhaustive.
                            if !*rest && pf.len() != entries.len() {
                                return (false, vec![]);
                            }
                            (true, bindings)
                        }
                        _ => (false, vec![]),
                    }
                } else {
                    (false, vec![])
                }
            }
        }
    }

    /// Open a persistent WebSocket client connection
    fn do_ws_connect(&mut self, url: &str, cell_name: &str) -> Result<Value, RuntimeError> {
        use tungstenite::connect;

        // Connect raw TcpStream and do WS handshake
        let parsed = url::Url::parse(url).map_err(|e| {
            RuntimeError::TypeError(format!("ws_connect: bad URL: {}", e))
        })?;
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(80);

        let stream = std::net::TcpStream::connect(format!("{}:{}", host, port)).map_err(|e| {
            RuntimeError::TypeError(format!("ws_connect: {}", e))
        })?;
        // Set read timeout so read() doesn't block forever — allows checking outgoing channel
        stream.set_read_timeout(Some(std::time::Duration::from_millis(50))).ok();

        let (ws, _) = tungstenite::client::client(url, stream).map_err(|e| {
            RuntimeError::TypeError(format!("ws handshake: {}", e))
        })?;

        // Outgoing channel
        let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
        self.ws_out = Some(Arc::new(std::sync::Mutex::new(out_tx)));

        // Writer thread: owns the WS, sends outgoing messages
        // Incoming messages are handled via SSE (separate channel)
        spawn_handler_thread(move || {
            let mut ws = ws;
            for msg in out_rx {
                if ws.send(tungstenite::Message::Text(msg)).is_err() { break; }
                let _ = ws.flush();
            }
            eprintln!("ws: writer thread ended");
        });

        // ws_connect is send-only. Use subscribe() for receiving.
        // This separation avoids the read/write deadlock in tungstenite.

        eprintln!("ws: connected to {}", url);
        Ok(map_from_pairs(vec![
            ("status".to_string(), Value::String("connected".to_string())),
            ("url".to_string(), Value::String(url.to_string())),
        ]))
    }

    /// Apply a lambda to a value: bind param, eval body
    /// Connect to a remote cell via TCP signal bus
    pub fn do_connect(&mut self, addr: &str, cell_name: &str) -> Result<Value, RuntimeError> {
        let stream = std::net::TcpStream::connect(addr).map_err(|e| {
            RuntimeError::TypeError(format!("connect: {}", e))
        })?;
        let read_stream = stream.try_clone().map_err(|e| {
            RuntimeError::TypeError(format!("connect clone: {}", e))
        })?;

        // Writer: sends outgoing signals to this peer (bounded: a peer that
        // stops reading is dropped, see send_to_peers)
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(BUS_QUEUE);

        // Register this peer's sender in the peer bus
        if let Some(ref peers) = self.peer_bus {
            if let Ok(mut senders) = peers.lock() {
                senders.push(tx);
            }
        }

        // Writer thread. Once the reader saw the peer go away, the next
        // event is reported NOT delivered and the writer stops (its channel
        // closes, so later emits are reported by send_to_peers): a write to
        // a socket the peer closed still "succeeded" into the kernel
        // buffer, and every event after a peer restart vanished silently
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        self.last_link_alive = Some(alive.clone());
        let alive_w = alive.clone();
        {
            use std::io::Write;
            let mut s = &stream;
            let _ = s.write_all(format!("HELLO {} {}\n", OWN_BUS_PORT.load(std::sync::atomic::Ordering::Relaxed), process_nonce()).as_bytes());
        }
        let write_stream = stream;
        spawn_handler_thread(move || bus_writer_loop(rx, write_stream, alive_w));

        // Reader thread: reads EVENT lines, dispatches to handlers
        let cells = self.cells.clone();
        let storage: HashMap<String, Arc<dyn StorageBackend>> = self.storage.iter()
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        let state_machines = self.state_machines.clone();
        let event_bus = self.event_bus.clone();
        let peer_bus = self.peer_bus.clone();
        let ws_out = self.ws_out.clone();
        let cname = cell_name.to_string();

        spawn_handler_thread(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(read_stream);
            for line in bus_lines(reader) {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                // Parse: EVENT name json_data
                if line.starts_with("EVENT ") {
                    let rest = &line[6..];
                    if let Some(space) = rest.find(' ') {
                        let event_name = &rest[..space];
                        let json_data = &rest[space+1..];

                        // the same filter as the inbound bus port (this outbound link
                        // ran private handlers and anything not accepted)
                        let parsed_json = serde_json::from_str::<serde_json::Value>(json_data).ok();
                        if let Err(why) = bus_event_allowed(event_name, parsed_json.as_ref()) {
                            eprintln!("bus: {}", why);
                            continue;
                        }
                        let data = match &parsed_json {
                            Some(parsed) => builtins::serde_json_to_value(parsed),
                            None => Value::String(json_data.to_string()),
                        };

                        // Dispatch to on event_name(data) handler
                        let prog = Program { protocols: vec![], imports: vec![], cells: cells.values().map(|c| {
                            Spanned::new(c.clone(), Span::new(0, 0))
                        }).collect() };
                        let mut interp = Interpreter::new(&prog);
                        for (k, v) in &storage {
                            interp.storage.insert(k.clone(), v.clone());
                        }
                        interp.state_machines = state_machines.clone();
                        interp.event_bus = event_bus.clone();
                        interp.peer_bus = peer_bus.clone();
                        interp.ws_out = ws_out.clone();

                        // Try all cells to find the handler
                        let mut handled = false;
                        for cn in interp.cells.keys().cloned().collect::<Vec<_>>() {
                            if interp.handler_cache.contains_key(&(cn.clone(), event_name.to_string())) {
                                match interp.call_signal(&cn, event_name, vec![data.clone()]) {
                                    Ok(_) => { handled = true; break; }
                                    Err(_) => {}
                                }
                            }
                        }
                        if !handled {
                            eprintln!("[bus] no handler for '{}'", event_name);
                        }
                    }
                }
            }
            alive.store(false, std::sync::atomic::Ordering::SeqCst);
            eprintln!("connect: peer disconnected");
        });

        eprintln!("connect: linked to {}", addr);
        Ok(map_from_pairs(vec![
            ("status".to_string(), Value::String("connected".to_string())),
            ("peer".to_string(), Value::String(addr.to_string())),
        ]))
    }

    /// Subscribe to a remote WS stream — dedicated read-only connection
    /// Incoming messages are parsed as {"event":"name","data":{...}} and dispatched to on name() handlers
    /// One WebSocket connection for `subscribe()`: bounded connect and
    /// handshake (a server that accepts TCP and never answers held the
    /// handler — and, serialized, the whole server — forever).
    fn subscribe_connect(url: &str) -> Result<tungstenite::WebSocket<std::net::TcpStream>, RuntimeError> {
        let parsed = url::Url::parse(url).map_err(|e| {
            RuntimeError::TypeError(format!("subscribe: bad URL: {}", e))
        })?;
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(80);
        use std::net::ToSocketAddrs;
        let addr = format!("{}:{}", host, port).to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or_else(|| {
            RuntimeError::TypeError(format!("subscribe: cannot resolve {}:{}", host, port))
        })?;
        let stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).map_err(|e| {
            RuntimeError::TypeError(format!("subscribe: {}", e))
        })?;
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
        let timeout_handle = stream.try_clone().ok();
        let (ws, _) = tungstenite::client::client(url, stream).map_err(|e| {
            RuntimeError::TypeError(format!("subscribe handshake (10 s limit): {}", e))
        })?;
        // linked: reads wait for events again
        if let Some(h) = timeout_handle { let _ = h.set_read_timeout(None); }
        Ok(ws)
    }

    fn do_subscribe(&mut self, url: &str, cell_name: &str) -> Result<Value, RuntimeError> {
        // the first connection is the caller's: a server that is down is a
        // handler error; later drops are reconnected in the reader thread
        let ws = Self::subscribe_connect(url)?;

        // Reader thread: blocks on read, dispatches to handlers
        let cells = self.cells.clone();
        let storage: HashMap<String, Arc<dyn StorageBackend>> = self.storage.iter()
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        let state_machines = self.state_machines.clone();
        let event_bus = self.event_bus.clone();
        let ws_out = self.ws_out.clone();
        let cname = cell_name.to_string();

        let url_owned = url.to_string();
        spawn_handler_thread(move || {
            let mut ws = ws;
            eprintln!("subscribe: listening on {}", url_owned);
            // a lost stream is reconnected like a [peers] link (1 s, backing
            // off to 30 s): a publisher restart ended the subscription for
            // the rest of the subscriber's life
            let mut backoff = 1u64;
            loop {
            let reason: String = loop {
                match ws.read() {
                    Ok(tungstenite::Message::Text(text)) => {
                        // Parse {"event":"trade","data":{...}} format
                        let prog = Program { protocols: vec![], imports: vec![], cells: cells.values().map(|c| {
                            Spanned::new(c.clone(), Span::new(0, 0))
                        }).collect() };
                        let mut interp = Interpreter::new(&prog);
                        for (k, v) in &storage {
                            interp.storage.insert(k.clone(), v.clone());
                        }
                        interp.state_machines = state_machines.clone();
                        interp.event_bus = event_bus.clone();
                        interp.ws_out = ws_out.clone();

                        // Try to parse as bus event format
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(event_name) = parsed.get("event").and_then(|e| e.as_str()) {
                                // a remote server cannot run a private handler or
                                // forge a record / variant (it did both)
                                fn forged(v: &serde_json::Value) -> bool {
                                    match v {
                                        serde_json::Value::Object(m) => m.contains_key("_type") || m.contains_key("_variant") || m.contains_key("_values") || m.values().any(forged),
                                        serde_json::Value::Array(xs) => xs.iter().any(forged),
                                        _ => false,
                                    }
                                }
                                if event_name.starts_with('_') || parsed.get("data").map_or(false, forged) {
                                    eprintln!("subscribe: refused event '{}' (a private handler, or forged _type/_variant data)", event_name);
                                    continue;
                                }
                                // the bus's policy: only an EVENT (one this program
                                // emits, or soma.toml [bus] accept lists) — a remote
                                // stream ran `on ws` and a public `wipe` handler
                                let accepted = EVENT_LISTENERS.get().map_or(false, |e| e.contains(event_name))
                                    || BUS_ACCEPT.get().map_or(false, |a| a.iter().any(|x| x == event_name));
                                if !accepted || matches!(event_name, "request" | "start" | "init" | "ws") {
                                    eprintln!("subscribe: refused event '{}' — not emitted by this program nor listed in soma.toml [bus] accept", event_name);
                                    continue;
                                }
                                let data = parsed.get("data")
                                    .map(|d| builtins::serde_json_to_value(d))
                                    .unwrap_or(Value::String(text.clone()));
                                // Dispatch to on event_name(data)
                                if let Err(e) = interp.call_signal(&cname, event_name, vec![data]) {
                                    eprintln!("subscribe: event '{}' failed (rolled back): {}", event_name, e);
                                }
                                continue;
                            }
                        }
                        // Fallback: dispatch to on ws(message)
                        let _ = interp.call_signal(&cname, "ws", vec![Value::String(text)]);
                    }
                    Ok(tungstenite::Message::Close(_)) => break "connection closed".to_string(),
                    Err(e) => break format!("error: {}", e),
                    _ => {}
                }
            };
            eprintln!("subscribe: {} — reconnecting to {}", reason, url_owned);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(backoff));
                match Self::subscribe_connect(&url_owned) {
                    Ok(w) => {
                        ws = w;
                        backoff = 1;
                        eprintln!("subscribe: linked again to {}", url_owned);
                        break;
                    }
                    Err(e) => {
                        let why = e.to_string();
                        eprintln!("subscribe: {} — retry in {} s", why.trim_start_matches("subscribe: "), (backoff * 2).min(30));
                        backoff = (backoff * 2).min(30);
                    }
                }
            }
            }
        });

        Ok(map_from_pairs(vec![
            ("status".to_string(), Value::String("subscribed".to_string())),
            ("url".to_string(), Value::String(url.to_string())),
        ]))
    }

    /// Evaluate an expression with the given environment. Used by the VM for
    /// interpreter fallback (e.g. try expressions).
    pub fn eval_expr_with_env(
        &mut self,
        expr: &Expr,
        env: &HashMap<String, Value>,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        let mut fx_env: Env = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        self.eval_expr(expr, &mut fx_env, cell_name, signal_name)
    }

    /// Evaluate a slot's memory invariants against a candidate write.
    /// Bindings visible to the invariant expression:
    ///   <slot name> — the value being written (so `abs(position) <= 5` reads naturally)
    ///   value       — alias for the same
    ///   key         — the key being written ("" for push/append)
    ///   size        — the slot's entry count AFTER the write would commit
    /// Returns Err — and the caller must NOT commit — if any invariant is
    /// false or cannot be evaluated. Errors are `try`-catchable.
    /// `on f(p: Pay)`: p is a Pay variant (a String naming a unit variant of
    /// Pay — `soma run app.cell f Cash` — is that variant). Anything else ran
    /// the body and failed later at a `match`, or not at all.
    /// A plain Map (JSON from a client, a CSV row, a tool call) given where
    /// a RECORD type is declared — a `cell type` with exactly one variant
    /// that has named fields — becomes that record, field by field; the
    /// error names the missing, extra or mistyped field. (There was no way
    /// to turn input into `Line`; a line without `qty` was stored and every
    /// tick after failed.) Several variants still need the variant spelled
    /// out: a client cannot pick one.
    fn coerce_variant_fields(&self, type_name: &str, variant: &str, fields: VariantValue) -> Result<VariantValue, String> {
        match (self.variant_fields.get(&(type_name.to_string(), variant.to_string())), fields) {
            (Some(VariantFields::Struct(types)), VariantValue::Struct(mut fields)) => {
                for (name, ty) in types {
                    if let Some(value) = fields.get_mut(name) {
                        *value = self.coerce_records(&ty.node, value.clone())
                            .map_err(|e| format!("{}.{}: {}", variant, name, e))?;
                    }
                }
                Ok(VariantValue::Struct(fields))
            }
            (Some(VariantFields::Tuple(types)), VariantValue::Tuple(fields)) => {
                let mut out = Vec::with_capacity(fields.len());
                for (i, value) in fields.into_iter().enumerate() {
                    out.push(match types.get(i) {
                        Some(ty) => self.coerce_records(&ty.node, value).map_err(|e| format!("{} field {}: {}", variant, i, e))?,
                        None => value,
                    });
                }
                Ok(VariantValue::Tuple(out))
            }
            (_, fields) => Ok(fields),
        }
    }

    fn coerce_records(&self, ty: &TypeExpr, v: Value) -> Result<Value, String> {
        match (ty, v) {
            // an Int where a Float is declared is that Float (`score = 3`
            // on `score: Float` kept an Int; `p.x + p.y` of JSON 1 and 2 gave Int 3)
            (TypeExpr::Simple(t), Value::Int(i)) if t == "Float" => promote_int_to_float(&i),
            (TypeExpr::Generic { name, args }, Value::List(xs)) if name == "List" => {
                let Some(t) = args.first() else { return Ok(Value::List(xs)) };
                let mut out = Vec::with_capacity(xs.len());
                for (i, x) in xs.into_iter().enumerate() { out.push(self.coerce_records(&t.node, x).map_err(|m| format!("element {}: {}", i, m))?); }
                Ok(Value::List(out))
            }
            (TypeExpr::Generic { name, args }, Value::Map(m)) if name == "Map" => {
                let Some(t) = args.last() else { return Ok(Value::Map(m)) };
                let mut out = IndexMap::new();
                for (k, x) in m.into_iter() { let c = self.coerce_records(&t.node, x).map_err(|e| format!("key {:?}: {}", k, e))?; out.insert(k, c); }
                Ok(Value::Map(out))
            }
            // JSON text for a record parameter (`soma run app.cell one '{"sku":…}'`)
            (TypeExpr::Simple(t), Value::String(txt)) if self.type_variants.get(t).map_or(false, |vs| vs.len() == 1) && txt.trim_start().starts_with('{') => {
                match serde_json::from_str::<serde_json::Value>(&txt) {
                    Ok(j) if builtins::string::json_has_inf(&j) => Err("a number beyond the Float range".to_string()),
                    Ok(j @ serde_json::Value::Object(_)) => {
                        let v = builtins::serde_json_to_value(&j);
                        if matches!(&v, Value::Map(m) if m.contains_key("_type") || m.contains_key("_variant")) {
                            return Err("the JSON may not carry _type / _variant".to_string());
                        }
                        self.coerce_records(ty, v)
                    }
                    _ => Ok(Value::String(txt)),
                }
            }
            (TypeExpr::Simple(t), Value::Map(m)) if self.type_variants.get(t).map_or(false, |vs| vs.len() == 1) => {
                let variant = self.type_variants[t][0].clone();
                let Some(crate::ast::VariantFields::Struct(fields)) = self.variant_fields.get(&(t.clone(), variant.clone())).cloned() else {
                    return Ok(Value::Map(m));
                };
                if let Some(extra) = m.keys().find(|k| !fields.iter().any(|(f, _)| f == *k)) {
                    return Err(format!("{} has no field '{}' (fields: {})", t, extra, fields.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", ")));
                }
                let mut out = IndexMap::new();
                for (f, fty) in &fields {
                    let Some(x) = m.get(f).cloned() else {
                        return Err(format!("{}: field '{}' is missing", t, f));
                    };
                    let x = self.coerce_records(&fty.node, x).map_err(|e| format!("{}: field '{}': {}", t, f, e))?;
                    self.value_fits(&fty.node, &x).map_err(|e| format!("{}: field '{}': {}", t, f, e))?;
                    out.insert(f.clone(), x);
                }
                Ok(Value::Variant { type_name: t.clone(), variant, fields: VariantValue::Struct(out) })
            }
            (_, v) => Ok(v),
        }
    }

    fn check_sum_param(&self, param: &Param, val: Value) -> Result<Value, String> {
        let val = self.coerce_records(&param.ty.node, val)
            .map_err(|m| format!("parameter '{}' expects {}: {}", param.name, crate::commands::describe::format_type(&param.ty.node), m))?;
        // a generic type is checked all the way down, as a slot is:
        // `xs: List<Map<String, Int>>` took `[{"a": "x"}]` from HTTP, the
        // bus and an LLM tool call (only the first level was checked)
        if let TypeExpr::Generic { .. } = &param.ty.node {
            return self.value_fits(&param.ty.node, &val).map(|_| val)
                .map_err(|m| format!("parameter '{}' expects {}: {}", param.name, crate::commands::describe::format_type(&param.ty.node), m));
        }
        let TypeExpr::Simple(t) = &param.ty.node else { return Ok(val) };
        let Some(variants) = self.type_variants.get(t) else { return Ok(val) };
        match &val {
            Value::Variant { type_name, variant, fields } if type_name == t => self.variant_ok(type_name, variant, fields).map(|_| val.clone())
                .map_err(|m| format!("parameter '{}': {}", param.name, m)),
            Value::String(s) if variants.contains(s) && matches!(self.variant_registry.get(s), Some((ty, VariantShape::Unit)) if ty == t) =>
                Ok(Value::Variant { type_name: t.clone(), variant: s.clone(), fields: VariantValue::Unit }),
            _ => Err(format!("parameter '{}' expects a {} variant ({}), got {} {}", param.name, t, variants.join(" | "), value_type_name(&val),
                { let x: String = format!("{}", val).chars().take(40).collect(); x })),
        }
    }

    /// a `cell test` (its helpers read every cell's slots by their bare
    /// name, like its rules)
    fn is_test_cell(&self, cell_name: &str) -> bool {
        self.cells.get(cell_name).map_or(false, |c| c.kind == CellKind::Test)
    }

    fn slot_invariants_use_size(&self, cell_name: &str, slot_name: &str) -> bool {
        let owner = self.slot_owner(cell_name, slot_name);
        let cell_name = owner.as_str();
        let invs = self.invariants.get(&format!("{}.{}", cell_name, slot_name)).or_else(|| if cell_name.is_empty() { self.invariants.get(slot_name) } else { None });
        let Some(invs) = invs else { return false };
        let mut names: HashSet<String> = HashSet::new();
        for inv in invs { free_names_expr(inv, &mut names); }
        names.iter().any(|n| matches!(n.as_str(), "size" | "_slot_len" | "len" | "count"))
    }

    fn slot_invariants_use_key(&self, cell_name: &str, slot_name: &str) -> bool {
        let owner = self.slot_owner(cell_name, slot_name);
        let cell_name = owner.as_str();
        let invs = self.invariants.get(&format!("{}.{}", cell_name, slot_name)).or_else(|| if cell_name.is_empty() { self.invariants.get(slot_name) } else { None });
        let Some(invs) = invs else { return false };
        let mut names: HashSet<String> = HashSet::new();
        for inv in invs { free_names_expr(inv, &mut names); }
        names.contains("key") || names.contains("_key")
    }

    /// Map slots of the cell whose invariants read `status`: a transition of
    /// instance `id` re-checks them at key `id` with the target state.
    fn slots_reading_status(&self, cell_name: &str) -> Vec<String> {
        let prefix = format!("{}.", cell_name);
        let mut out: Vec<String> = self.invariants.iter()
            .filter(|(k, invs)| k.starts_with(&prefix) && invs.iter().any(invariant_reads_status))
            .map(|(k, _)| k[prefix.len()..].to_string())
            .filter(|slot| self.slot_kind(cell_name, slot) == Some("Map"))
            .collect();
        out.sort();
        out
    }

    /// Does a rule BETWEEN slots guard this one? Then a delete matters too:
    /// dropping an entry takes the other side to () (a "value invariant
    /// cannot break on a delete" only holds for a single-slot rule).
    fn slot_invariants_cross(&self, cell_name: &str, slot_name: &str) -> bool {
        let owner = self.slot_owner(cell_name, slot_name);
        let cell = owner.as_str();
        let invs = self.invariants.get(&format!("{}.{}", cell, slot_name)).or_else(|| if cell.is_empty() { self.invariants.get(slot_name) } else { None });
        let Some(invs) = invs else { return false };
        invs.iter().any(|inv| {
            let mut names: HashSet<String> = HashSet::new();
            free_names_expr(inv, &mut names);
            names.iter().any(|n| n != slot_name && self.slot_kind(cell, n).is_some())
        })
    }

    fn slot_has_invariants(&self, cell_name: &str, slot_name: &str) -> bool {
        let owner = self.slot_owner(cell_name, slot_name);
        let cell_name = owner.as_str();
        // the unqualified key only without a cell (a test rule): another
        // cell's invariant on a slot of the same name refused valid writes
        self.invariants.get(&format!("{}.{}", cell_name, slot_name)).or_else(|| if cell_name.is_empty() { self.invariants.get(slot_name) } else { None }).map_or(false, |v| !v.is_empty())
    }

    fn check_invariants(
        &mut self,
        cell_name: &str,
        slot_name: &str,
        key_str: &str,
        val: &Value,
        size_after: i64,
        op: &str,
    ) -> Result<(), ExecError> {
        let owner = self.slot_owner(cell_name, slot_name);
        let cell_name = owner.as_str();
        let prefixed = format!("{}.{}", cell_name, slot_name);
        let invs = match self.invariants.get(&prefixed).or_else(|| if cell_name.is_empty() { self.invariants.get(slot_name) } else { None }) {
            Some(v) if !v.is_empty() => v.clone(),
            _ => return Ok(()),
        };
        // a delete only matters to a rule BETWEEN slots (the entry it drops
        // takes that side to ()): evaluate those, not the slot's own rules
        let invs: Vec<Expr> = if op == "delete_cross" {
            invs.into_iter().filter(|inv| {
                let mut names: HashSet<String> = HashSet::new();
                free_names_expr(inv, &mut names);
                names.iter().any(|n| n != slot_name && self.slot_kind(cell_name, n).is_some())
            }).collect()
        } else if op == "transition" {
            // a transition re-checks only the rules that read `status`: the
            // slot's own value rules were checked when the value was written
            // (and the entry may not exist yet — `attempts >= 0` on ())
            invs.into_iter().filter(invariant_reads_status).collect()
        } else { invs };
        if invs.is_empty() { return Ok(()); }
        for inv in &invs {
            // the start-up audit has no "value before the write": an
            // invariant reading `slot.get(key)` (write-once, monotone) is a
            // rule about WRITES — checked against the stored row itself,
            // every row of a write-once log was reported as violating it
            if op == "read" {
                let mut reads_before = false;
                crate::checker::literals::for_each_in_expr(inv, &mut |e| match e {
                    Expr::MethodCall { target, method, .. } if matches!(method.as_str(), "get" | "has" | "contains" | "contains_key")
                        && matches!(&target.node, Expr::Ident(n) if n == slot_name) => reads_before = true,
                    Expr::Index { target, .. } if matches!(&target.node, Expr::Ident(n) if n == slot_name) => reads_before = true,
                    _ => {}
                });
                if reads_before { continue; }
            }
            let mut env = FxHashMap::default();
            env.insert(slot_name.to_string(), val.clone());
            env.insert(INV_SLOT.to_string(), Value::String(slot_name.to_string()));
            env.insert("value".to_string(), val.clone());
            // a List slot's key is the element's INDEX, an Int (`key` was ""
            // for push and the text "0" for rows[0] = v, so `entries.get(key)`
            // never found the element a write-once invariant guards)
            let key_val = if self.slot_kind(cell_name, slot_name) == Some("List") {
                key_str.trim_start_matches('#').parse::<i64>().map(|i| Value::Int(SomaInt::from_i64(i))).unwrap_or_else(|_| Value::String(key_str.to_string()))
            } else { Value::String(key_str.to_string()) };
            env.insert("key".to_string(), key_val);
            env.insert("size".to_string(), Value::Int(SomaInt::from_i64(size_after)));
            // `status`: the state of the machine instance whose id is the key
            // (`invariant status != "released" || (balances ?? 0) == 0` ties a
            // lifecycle to its data); during a transition it is the TARGET
            if invariant_reads_status(inv) {
                let st = match &self.pending_status {
                    Some((id, target)) if id == key_str => Value::String(target.clone()),
                    _ => self.do_get_status_for(cell_name, key_str).map_err(|e| ExecError::Runtime(RuntimeError::RequireFailed(format!(
                        "memory invariant on '{}' reads `status`, but {}", slot_name, e))))?,
                };
                env.insert("status".to_string(), st);
            }
            // a rule BETWEEN slots (`invariant reserved <= stock`): the other
            // slots of the cell read at the same key (a Map / List slot), or
            // whole (anything else) — the written slot is its new value
            let inv = {
                let others: Vec<String> = crate::checker::invariants::deep_idents(inv).into_iter()
                    .filter(|n| n != slot_name && self.slot_kind(cell_name, n).is_some())
                    .collect();
                for other in &others {
                    let v = self.call_storage_method(cell_name, other, "get", &[env.get("key").cloned().unwrap_or(Value::Unit)])
                        .unwrap_or(Value::Unit);
                    env.insert(other.clone(), v);
                    // `other.size` / `len(other)`: that slot's entry count
                    let n = self.call_storage_method(cell_name, other, "len", &[]).unwrap_or(Value::Int(SomaInt::from_i64(0)));
                    env.insert(format!("__count__{}", other), n);
                }
                count_of_other_slots(inv, &others)
            };
            let inv = &inv;
            // legacy bindings (pre-V1.8 invariants)
            env.insert("_slot_len".to_string(), Value::Int(SomaInt::from_i64(size_after)));
            env.insert("_slot_name".to_string(), Value::String(slot_name.to_string()));
            env.insert("_key".to_string(), Value::String(key_str.to_string()));
            match self.eval_expr(inv, &mut env, cell_name, "") {
                Ok(v) if invariant_holds(&v) => {}
                Ok(_) => {
                    return Err(ExecError::Runtime(RuntimeError::RequireFailed(format!(
                        "memory invariant violated on '{}': {} — rejected {} of {} (key \"{}\"); the slot is unchanged",
                        slot_name, show_invariant(inv, slot_name), if op == "delete_cross" { "delete" } else { op }, val, key_str
                    ))));
                }
                Err(e) => {
                    let detail = match &e {
                        ExecError::Runtime(re) => re.to_string(),
                        other => format!("{:?}", other),
                    };
                    return Err(ExecError::Runtime(RuntimeError::RequireFailed(format!(
                        "memory invariant on '{}' could not be evaluated: {} ({})",
                        slot_name, crate::ast::render_expr(inv), detail
                    ))));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn apply_lambda(&mut self, lambda: &Value, arg: Value, cell_name: &str) -> Result<Value, ExecError> {
        // a lambda call counts toward the recursion guard like a handler
        // call: `let f = g => g(g)  f(f)` overflowed the OS stack and
        // aborted the server
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.current_depth -= 1;
            return Err(ExecError::Runtime(RuntimeError::StackOverflow));
        }
        let r = self.apply_lambda_inner(lambda, arg, cell_name);
        self.current_depth -= 1;
        r
    }

    /// The environment of a pure-expression lambda, built ONCE for a whole
    /// `map` / `filter` / … over a list: rebuilding it per element cloned
    /// every captured list and map (`range(0, n) |> map(i => xs[i])` was
    /// quadratic — 2.8 s for 16 000 items). None for a lambda with
    /// statements (its lets must start fresh each call).
    pub(crate) fn prepare_lambda_env(&self, lambda: &Value) -> Option<Env> {
        let Value::Lambda { body, env: closed_env, .. } = lambda else { return None };
        // Interpolation can execute assignments too; its captured environment
        // must start fresh for every item just like an ordinary block lambda.
        if expr_may_assign(&body.node) { return None; }
        Some(closed_env.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    /// apply_lambda with an environment from prepare_lambda_env
    pub(crate) fn apply_prepared(&mut self, lambda: &Value, env: &mut Env, arg: Value, cell_name: &str) -> Result<Value, ExecError> {
        let Value::Lambda { param, body, .. } = lambda else { return self.apply_lambda(lambda, arg, cell_name) };
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.current_depth -= 1;
            return Err(ExecError::Runtime(RuntimeError::StackOverflow));
        }
        env.insert(param.clone(), arg);
        let r = self.eval_expr(&body.node, env, cell_name, "");
        self.current_depth -= 1;
        r
    }

    fn apply_lambda_inner(&mut self, lambda: &Value, arg: Value, cell_name: &str) -> Result<Value, ExecError> {
        match lambda {
            Value::Lambda { param, body, env: closed_env } => {
                let mut env: Env = closed_env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                env.insert(param.clone(), arg);
                self.eval_expr(&body.node, &mut env, cell_name, "")
            }
            Value::LambdaBlock { param, stmts, result, env: closed_env } => {
                let mut env: Env = closed_env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                env.insert(param.clone(), arg);
                let mut last_val = Value::Unit;
                for stmt in stmts {
                    last_val = self.exec_stmt(&stmt.node, &mut env, cell_name, "")?;
                }
                if matches!(result.node, Expr::Literal(Literal::Unit)) && !stmts.is_empty() {
                    return Ok(last_val);
                }
                self.eval_expr(&result.node, &mut env, cell_name, "")
            }
            _ => Err(ExecError::Runtime(RuntimeError::TypeError(
                format!("expected lambda, got {}", lambda)
            )))
        }
    }

    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        if matches!(a, Value::List(_) | Value::Map(_) | Value::Variant { .. }) {
            return deep_equal(a, b);
        }
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => x == y,
            (Value::Float(x), Value::Float(y)) => x == y,
            (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_)) => numeric_cmp(a, b) == Some(std::cmp::Ordering::Equal),
            (Value::String(x), Value::String(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        }
    }

    fn eval_binop(&self, l: &Value, op: BinOp, r: &Value) -> Result<Value, RuntimeError> {
        // First-class matrix operators. `A * B` is the matrix product when
        // both are matrices (List<List>); `k * M` / `M * k` scale; `A + B`
        // and `A - B` are elementwise on equal-shape matrices. Flat-list
        // `+` (concat) and scalar arithmetic are untouched.
        if let Some(res) = builtins::linalg::try_matrix_binop(l, op, r) {
            return res;
        }
        match (l, r) {
            (Value::Int(a), Value::Int(b)) => match op {
                BinOp::Add => Ok(Value::Int(a.clone().add(b.clone()))),
                BinOp::Sub => Ok(Value::Int(a.clone().sub(b.clone()))),
                BinOp::Mul => a.clone().checked_big_mul(b.clone()).map(Value::Int).map_err(|m| RuntimeError::Domain { kind: "range".to_string(), message: m }),
                BinOp::Div => int_div_value(a, b),
                BinOp::Mod => {
                    if b.to_i64() == Some(0) {
                        Err(RuntimeError::TypeError("modulo by zero".to_string()))
                    } else {
                        Ok(Value::Int(a.clone().modulo(b.clone())))
                    }
                }
                BinOp::And => Ok(Value::Bool(a.to_i64() != Some(0) && b.to_i64() != Some(0))),
                BinOp::Or => Ok(Value::Bool(a.to_i64() != Some(0) || b.to_i64() != Some(0))),
            },
            // (`"a" + 1.5` said "expected Float, got String": it is the same
            // refusal as `"a" + 1`, reported below)
            (Value::Float(_), Value::Int(_) | Value::Float(_)) | (Value::Int(_), Value::Float(_)) => {
                let a = l.as_float()?;
                let b = r.as_float()?;
                match op {
                    BinOp::Add => Ok(Value::Float(a + b)),
                    BinOp::Sub => Ok(Value::Float(a - b)),
                    BinOp::Mul => Ok(Value::Float(a * b)),
                    BinOp::Div => Ok(Value::Float(a / b)),
                    BinOp::Mod => Ok(Value::Float(a % b)),
                    _ => Err(RuntimeError::TypeError("invalid op for floats".to_string())),
                }
            }
            (Value::String(a), Value::String(b)) => match op {
                BinOp::Add => Ok(Value::String(format!("{}{}", a, b))),
                _ => Err(RuntimeError::TypeError("invalid op for strings".to_string())),
            },
            (Value::List(a), Value::List(b)) => match op {
                BinOp::Add => {
                    let mut result = a.clone();
                    result.extend(b.clone());
                    Ok(Value::List(result))
                }
                _ => Err(RuntimeError::TypeError("this operator does not apply to these two lists — numeric vectors of equal length take + - * / elementwise, a matrix times a matrix is the product (a matrix times a vector: write the vector as a one-column matrix [[a], [b]]), and concat(a, b) joins lists".to_string())),
            },
            (Value::Bool(a), Value::Bool(b)) => match op {
                BinOp::And => Ok(Value::Bool(*a && *b)),
                BinOp::Or => Ok(Value::Bool(*a || *b)),
                _ => Err(RuntimeError::TypeError("invalid op for bools".to_string())),
            },
            _ => {
                // `counts[k] += 1` on a key that is not there yet
                let hint = if matches!(l, Value::Unit) || matches!(r, Value::Unit) {
                    match op {
                        BinOp::Sub => " — one side is () (a missing key or field?): default it, `(m.get(k) ?? 0) - 1`",
                        BinOp::Mul => " — one side is () (a missing key or field?): default it, `(m.get(k) ?? 1) * n`",
                        _ => " — one side is () (a missing key or field?): default it, `(m.get(k) ?? 0) + 1`",
                    }
                } else { "" };
                Err(RuntimeError::TypeError(format!(
                    "cannot {} {} and {}: {} {} {}{}",
                    binop_verb(op), value_type_name(l), value_type_name(r), l, op, r, hint
                )))
            }
        }
    }

    /// Public comparison for use by the test runner
    pub fn eval_cmpop_values(&self, l: &Value, op: CmpOp, r: &Value) -> Result<bool, RuntimeError> {
        match self.eval_cmpop(l, op, r)? {
            Value::Bool(b) => Ok(b),
            _ => Ok(false),
        }
    }

    fn eval_cmpop(&self, l: &Value, op: CmpOp, r: &Value) -> Result<Value, RuntimeError> {
        // Vectorized comparison masks: `A > 2` → 0/1 matrix, `v > 2` →
        // 0/1 vector (numpy-style; feed the mask to where_mask).
        if let Some(res) = builtins::linalg::try_tensor_cmpop(l, op, r) {
            return res;
        }
        // Handle Unit comparisons first (before type coercion)
        if matches!(l, Value::Unit) || matches!(r, Value::Unit) {
            let result = match op {
                CmpOp::Eq => matches!((l, r), (Value::Unit, Value::Unit)),
                CmpOp::Ne => !matches!((l, r), (Value::Unit, Value::Unit)),
                // Ordering against () used to be a silent `false`: a typo'd
                // field (`r.statuss >= 500`) reads as () and every record
                // quietly failed the test. `() + 1` already raises; so does this.
                _ => {
                    let side = if matches!(l, Value::Unit) { "left" } else { "right" };
                    return Err(RuntimeError::TypeError(format!(
                        "cannot order () with {op}: the {side} side is () (null) — a missing map field \
                         or an unset slot reads as (). Test it first (`x != ()`) or default it (`x ?? 0`)",
                    )));
                }
            };
            return Ok(Value::Bool(result));
        }

        match (l, r) {
            (Value::Int(a), Value::Int(b)) => {
                let c = a.cmp(b);
                let result = match op {
                    CmpOp::Lt => c < 0,
                    CmpOp::Gt => c > 0,
                    CmpOp::Le => c <= 0,
                    CmpOp::Ge => c >= 0,
                    CmpOp::Eq => c == 0,
                    CmpOp::Ne => c != 0,
                };
                Ok(Value::Bool(result))
            }
            (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_)) => {
                Ok(Value::Bool(compare_order(numeric_cmp(l, r), op)))
            }
            (Value::Float(_), _) | (_, Value::Float(_)) => {
                let a = l.as_float()?;
                let b = r.as_float()?;
                let result = match op {
                    CmpOp::Lt => a < b,
                    CmpOp::Gt => a > b,
                    CmpOp::Le => a <= b,
                    CmpOp::Ge => a >= b,
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                };
                Ok(Value::Bool(result))
            }
            // String comparison
            (Value::String(a), Value::String(b)) => {
                let result = match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    CmpOp::Lt => a < b,
                    CmpOp::Gt => a > b,
                    CmpOp::Le => a <= b,
                    CmpOp::Ge => a >= b,
                };
                Ok(Value::Bool(result))
            }
            // Bool comparison
            (Value::Bool(a), Value::Bool(b)) => {
                let result = match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    _ => return Err(RuntimeError::TypeError("cannot order booleans".to_string())),
                };
                Ok(Value::Bool(result))
            }
            // Unit comparison (null checks)
            (Value::Unit, Value::Unit) => {
                Ok(Value::Bool(matches!(op, CmpOp::Eq | CmpOp::Le | CmpOp::Ge)))
            }
            // Anything compared to Unit (null check)
            (Value::Unit, _) | (_, Value::Unit) => {
                Ok(Value::Bool(matches!(op, CmpOp::Ne)))
            }
            // V1.6: variant equality. Same type + same variant + same fields.
            (Value::Variant { type_name: t1, variant: v1, fields: f1 },
             Value::Variant { type_name: t2, variant: v2, fields: f2 }) => {
                let eq = t1 == t2 && v1 == v2 && variant_fields_equal(f1, f2);
                let result = match op {
                    CmpOp::Eq => eq,
                    CmpOp::Ne => !eq,
                    _ => return Err(RuntimeError::TypeError("cannot order variants".to_string())),
                };
                Ok(Value::Bool(result))
            }
            // Structural equality on collections: [1, 2] == [1, 2], and two
            // maps are equal when they hold the same keys with equal values,
            // whatever the insertion order.
            (Value::List(_), Value::List(_)) | (Value::Map(_), Value::Map(_)) => {
                let eq = deep_equal(l, r);
                match op {
                    CmpOp::Eq => Ok(Value::Bool(eq)),
                    CmpOp::Ne => Ok(Value::Bool(!eq)),
                    _ => Err(RuntimeError::TypeError(format!(
                        "cannot order {}s with <, >, <=, >= — compare a field or len() instead",
                        value_type_name(l)
                    ))),
                }
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot compare {} and {}",
                value_type_name(l), value_type_name(r)
            ))),
        }
    }

    /// Undo journaled effects back to `mark` (newest first).
    /// A push to SSE / WebSocket clients: sent at commit (now, outside one)
    pub(crate) fn send_bus(&mut self, event: BusEvent) {
        match self.journal.as_mut() {
            Some(j) => j.push(UndoOp::Push(event)),
            None => self.send_bus_now(event),
        }
    }

    fn send_bus_now(&self, event: BusEvent) {
        if let Some(ref bus) = self.event_bus {
            if let Ok(mut senders) = bus.lock() {
                // a full queue is a client that is not reading: drop it —
                // and say so (a silent drop looked like a lost event)
                let before = senders.len();
                senders.retain(|sender| sender.try_send(event.clone()).is_ok());
                let dropped = before - senders.len();
                if dropped > 0 {
                    eprintln!("sse: {} subscriber(s) stopped reading ({} events queued each) — dropped; {} left",
                        dropped, BUS_QUEUE, senders.len());
                }
            }
        }
    }

    /// A failing `try`: undo its writes, keep the ids it drew.
    pub(crate) fn rollback_savepoint(&mut self, mark: usize) {
        let Some(journal) = self.journal.as_mut() else { return };
        let mut kept: Vec<UndoOp> = Vec::new();
        let mut rest: Vec<UndoOp> = Vec::new();
        while journal.len() > mark {
            match journal.pop() {
                Some(op @ UndoOp::Counter { .. }) => kept.push(op),
                Some(op) => rest.push(op),
                None => break,
            }
        }
        for op in rest { undo(op); }
        kept.reverse();
        if let Some(journal) = self.journal.as_mut() { journal.extend(kept); }
    }

    pub(crate) fn rollback_to(&mut self, mark: usize) {
        if let Err(e) = self.check_storage_write() {
            self.fail_transaction(format!("cannot restore savepoint: {e}"));
            return;
        }
        let Some(journal) = self.journal.as_mut() else { return };
        while journal.len() > mark {
            match journal.pop() {
                Some(UndoOp::Counter { backend, key, prev }) => match prev {
                    Some(v) => backend.set(&key, v),
                    None => { backend.delete(&key); }
                },
                Some(UndoOp::Restore { backend, key, prev }) => match prev {
                    Some(v) => backend.set(&key, v),
                    None => {
                        backend.delete(&key);
                    }
                },
                Some(UndoOp::Unappend { backend }) => backend.unappend(),
                Some(UndoOp::RestoreList { backend, prev }) => backend.replace_list(prev),
                Some(UndoOp::ListSet { backend, index, prev }) => { backend.list_set(index, prev); }
                Some(UndoOp::ListRestore { backend, token, prev }) => backend.list_restore(token, prev),
                Some(UndoOp::Push(_)) | Some(UndoOp::PeerSend(_)) | Some(UndoOp::Cluster(_)) | Some(UndoOp::Horde(_)) => {}
                None => break,
            }
        }
    }

    fn fail_transaction(&mut self, message: String) -> RuntimeError {
        self.transaction_failure = Some(message.clone());
        RuntimeError::StorageTransaction(message)
    }

    fn check_transaction(&self) -> Result<(), RuntimeError> {
        match &self.transaction_failure {
            Some(message) => Err(RuntimeError::StorageTransaction(message.clone())),
            None => Ok(()),
        }
    }

    /// Consume backend failures at the operation that caused them, with a
    /// final check at commit for internal writes (replication, for example).
    pub(crate) fn check_storage_write(&mut self) -> Result<(), RuntimeError> {
        let pending = crate::runtime::storage::take_write_error();
        let lost_transaction = self.journal.is_some() && crate::runtime::storage::shared_connection()
            .is_some_and(|c| c.lock().unwrap_or_else(|e| e.into_inner()).is_autocommit());
        if lost_transaction {
            return Err(self.fail_transaction(format!("the database ended the transaction unexpectedly{}",
                pending.map(|e| format!(": {e}")).unwrap_or_default())));
        }
        if let Some(e) = pending {
            if e.read { return Err(self.fail_transaction(format!("storage read failed ({e})"))); }
            return Err(RuntimeError::Domain { kind: "storage".into(),
                message: format!("storage: the database refused a write ({e})") });
        }
        self.check_transaction()
    }

    /// Open auxiliary tables before BEGIN. Creating them lazily inside a
    /// handler both missed the first transaction and cached tables that a
    /// later rollback could remove.
    fn prepare_persistent_auxiliary_storage(&mut self) -> Result<(), RuntimeError> {
        if PERSIST_MACHINES.load(std::sync::atomic::Ordering::Relaxed)
            || self.storage.values().any(|b| b.backend_name() == "sqlite") {
            for cell in self.cells.keys() {
                for name in ["_counters", "_agent_memory"] {
                    let key = format!("{cell}._{name}");
                    if !self.storage.contains_key(&key) {
                        let backend = crate::runtime::storage::SqliteBackend::try_new(cell, name)
                            .map_err(|e| RuntimeError::StorageTransaction(format!("cannot prepare storage: {e}")))?;
                        self.storage.insert(key, Arc::new(backend));
                    }
                }
            }
        }
        Ok(())
    }

    /// A refused BEGIN never falls back to running without a transaction.
    fn unit_begin(&mut self) -> Result<Unit, RuntimeError> {
        let serial = HANDLER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.last_commit_writes = 0;
        self.check_storage_write()?;
        self.prepare_persistent_auxiliary_storage()?;
        let txn = crate::runtime::storage::shared_connection();
        if let Some(c) = &txn {
            c.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("BEGIN IMMEDIATE")
                .map_err(|e| RuntimeError::StorageTransaction(format!("cannot begin transaction: {e}")))?;
        }
        let cross = if txn.is_none() { Some(CrossProcessLock::acquire()?) } else { None };
        if txn.is_some() { crate::runtime::storage::transaction_started(); }
        self.journal = Some(Vec::new());
        self.journal_gen = self.journal_gen.wrapping_add(1);
        Ok(Unit { _serial: serial, txn, _cross: cross })
    }

    /// Commit before releasing journaled notifications. On failure SQLite
    /// restores its own data; only non-SQLite backends need journal undo.
    fn unit_end(&mut self, unit: Unit, ok: bool) -> Result<(), RuntimeError> {
        let mut failure = self.check_storage_write().err();
        if ok && failure.is_none() {
            if let Some(c) = &unit.txn {
                if let Err(e) = c.lock().unwrap_or_else(|e| e.into_inner()).execute_batch("COMMIT") {
                    failure = Some(RuntimeError::StorageTransaction(format!("cannot commit transaction: {e}")));
                }
            }
        }
        let committed = ok && failure.is_none();
        if !committed {
            if let Some(c) = &unit.txn {
                let c = c.lock().unwrap_or_else(|e| e.into_inner());
                if !c.is_autocommit() {
                    if let Err(e) = c.execute_batch("ROLLBACK") {
                        failure = Some(RuntimeError::StorageTransaction(format!("cannot roll back transaction: {e}")));
                    }
                }
            }
        }
        crate::runtime::storage::transaction_ended();
        let journal = self.journal.take().unwrap_or_default();
        self.last_commit_writes = if committed { journal.iter().filter(|u| !matches!(u,
            UndoOp::Push(_) | UndoOp::PeerSend(_) | UndoOp::Cluster(_) | UndoOp::Horde(_))).count() } else { 0 };
        if !committed {
            for op in journal.into_iter().rev() {
                let sqlite = match &op {
                    UndoOp::Restore { backend, .. } | UndoOp::Counter { backend, .. }
                    | UndoOp::Unappend { backend } | UndoOp::RestoreList { backend, .. } | UndoOp::ListSet { backend, .. } | UndoOp::ListRestore { backend, .. } => backend.backend_name() == "sqlite",
                    _ => false,
                };
                if !sqlite { undo(op); }
            }
            if let Some(e) = crate::runtime::storage::take_write_error() {
                failure = Some(RuntimeError::StorageTransaction(format!("cannot restore storage: {e}")));
            }
            return match failure { Some(e) => Err(e), None => Ok(()) };
        }
        let mut peer_lines = Vec::new();
        let mut cluster_updates = Vec::new();
        for op in journal {
            match op {
                UndoOp::Push(e) => self.send_bus_now(e),
                UndoOp::PeerSend(line) => peer_lines.push(line),
                UndoOp::Cluster(update) => cluster_updates.push(update),
                UndoOp::Horde(horde::Commit::Sync { spec, inputs }) => self.deferred_hordes.push((spec, inputs)),
                UndoOp::Horde(commit) => horde::apply_commit(commit),
                _ => {},
            }
        }
        if let Some(cluster) = &self.cluster {
            for update in cluster_updates { cluster.broadcast(crate::runtime::cluster::Frame::Update(update)); }
            for line in &peer_lines { cluster.broadcast(crate::runtime::cluster::Frame::Signal(line.trim_end_matches('\n').to_string())); }
        }
        if !peer_lines.is_empty() {
            if let Some(ref peers) = self.peer_bus {
                if let Ok(mut senders) = peers.lock() {
                    for l in &peer_lines { send_to_peers(&mut senders, l); }
                }
            }
        }
        drop(unit);
        Ok(())
    }

    /// Nested handlers join the running atomic unit.
    pub fn atomically<T, E: From<RuntimeError>>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, E>) -> Result<T, E> {
        if self.current_depth == 0 && self.journal.is_none() { self.transaction_failure = None; }
        self.check_transaction()?;
        if self.journal.is_some() { return f(self); }
        let unit = self.unit_begin()?;
        let result = f(self);
        let end = self.unit_end(unit, result.is_ok());
        self.run_deferred_hordes();
        end?;
        result
    }

    /// A task commits each step before waiting outside the handler lock.
    fn run_task<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, RuntimeError>) -> Result<T, RuntimeError> {
        if self.current_depth == 0 { self.transaction_failure = None; }
        self.check_transaction()?;
        self.task_unit = Some(self.unit_begin()?);
        let result = f(self);
        let end = if let Some(u) = self.task_unit.take() { self.unit_end(u, result.is_ok()) } else { self.check_transaction() };
        self.run_deferred_hordes();
        end?;
        result
    }

    /// A failed boundary aborts the task, even if a tool tries to catch it.
    pub(crate) fn outside_unit<T>(&mut self, f: impl FnOnce() -> T) -> Result<T, RuntimeError> {
        self.check_transaction()?;
        match self.task_unit.take() {
            Some(unit) => {
                if let Err(e) = self.unit_end(unit, true) {
                    return Err(self.fail_transaction(e.to_string()));
                }
                let r = f();
                match self.unit_begin() {
                    Ok(unit) => self.task_unit = Some(unit),
                    Err(e) => return Err(self.fail_transaction(e.to_string())),
                }
                Ok(r)
            }
            None => Ok(f()),
        }
    }

    fn handler_is_task(&self, cell_name: &str, signal_name: &str) -> bool {
        self.cells.get(cell_name).map_or(false, |c| c.sections.iter().any(|s| matches!(&s.node,
            Section::OnSignal(on) if on.signal_name == signal_name && on.properties.iter().any(|p| p == "task"))))
    }

    /// Does the program define a handler `name` taking exactly `argc`
    /// arguments? Then a call resolves to it rather than to a builtin.
    #[inline]
    fn cell_defines_handler(&self, cell_name: &str, name: &str) -> bool {
        // a test rule or `soma run` names the program's handlers directly
        if cell_name.is_empty() { return true; }
        self.cells.get(cell_name).map_or(false, |c| c.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == name)))
    }

    /// Dispatch already evaluated arguments once, for calls and pipes alike.
    fn eval_named_call(&mut self, name: &String, arg_vals: Vec<Value>, env: &mut Env, cell_name: &str, signal_name: &str) -> Result<Value, ExecError> {
        // Tuple-variant constructor: `Up(3)`, `Move(x, y)`.
        if let Some((vtype, shape)) = self.variant_registry.get(name).cloned() {
            if let VariantShape::Tuple(arity) = shape {
                if arg_vals.len() != arity {
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "variant '{}' takes {} argument{}, got {}",
                        name,
                        arity,
                        if arity == 1 { "" } else { "s" },
                        arg_vals.len()
                    ))));
                }
                let fields = self.coerce_variant_fields(&vtype, name, VariantValue::Tuple(arg_vals))
                    .map_err(|m| ExecError::Runtime(RuntimeError::TypeError(m)))?;
                if let Err(m) = self.variant_ok(&vtype, name, &fields) {
                    return Err(ExecError::Runtime(RuntimeError::Domain { kind: "type".to_string(), message: m }));
                }
                return Ok(Value::Variant {
                    type_name: vtype,
                    variant: name.clone(),
                    fields,
                });
            }
        }

        // YOUR handler of that name and arity wins over these network
        // builtins too (`on subscribe(a, b, c)` ran the WebSocket
        // subscribe; `link(user_text)` opened a socket)
        let user_net = matches!(name.as_str(), "ws_connect" | "ws_send" | "link" | "subscribe")
            && self.user_handler_takes(name, arg_vals.len());
        // raw sockets are outside a capability-scoped tool
        if !user_net && matches!(name.as_str(), "ws_connect" | "connect" | "subscribe") {
            if let Some(caps) = self.current_tool_caps.as_ref() {
                if !caps.iter().any(|c| c == "*") {
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!("capability denied: {}() is outside this tool's capabilities {:?}", name, caps))));
                }
            }
        }
        // WebSocket builtins — need &mut self
        if !user_net && name == "ws_connect" {
            if let Some(Value::String(url)) = arg_vals.first() {
                return self.do_ws_connect(url, cell_name)
                    .map_err(ExecError::Runtime);
            }
            return Err(ExecError::Runtime(RuntimeError::TypeError("ws_connect(url)".to_string())));
        }
        if !user_net && name == "ws_send" {
            if let Some(ref out) = self.ws_out {
                let msg = match arg_vals.first() {
                    Some(Value::String(s)) => s.clone(),
                    Some(v) => format!("{}", v),
                    None => "{}".to_string(),
                };
                if let Ok(sender) = out.lock() {
                    let _ = sender.send(msg);
                }
                return Ok(Value::Unit);
            }
            return Err(ExecError::Runtime(RuntimeError::TypeError("ws_send: not connected".to_string())));
        }
        // connect(host:port) — open a TCP signal bus link
        if !user_net && name == "link" {
            if let Some(Value::String(addr)) = arg_vals.first() {
                return self.do_connect(addr, cell_name)
                    .map_err(ExecError::Runtime);
            }
            return Err(ExecError::Runtime(RuntimeError::TypeError("connect(\"host:port\")".to_string())));
        }
        if !user_net && name == "subscribe" {
            if let Some(Value::String(url)) = arg_vals.first() {
                return self.do_subscribe(url, cell_name)
                    .map_err(ExecError::Runtime);
            }
            return Err(ExecError::Runtime(RuntimeError::TypeError("subscribe(url)".to_string())));
        }

        // First-class lambdas: a local variable bound to a lambda is callable
        if let Some(lam @ (Value::Lambda { .. } | Value::LambdaBlock { .. })) = env.get(name) {
            let lam = lam.clone();
            if arg_vals.len() != 1 {
                return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                    "lambda '{}' takes exactly 1 argument, got {}", name, arg_vals.len()
                ))));
            }
            return self.apply_lambda(&lam, arg_vals.into_iter().next().unwrap(), cell_name);
        }
        // Resolution: a handler of the program taking this many
        // arguments shadows a builtin of the same name. `on list()`
        // can still call the builtin list(1, 2) — 2 ≠ 0 arguments.
        // …but only the CALLING cell's own handler: a library's
        // `on escape_html(s) { return s }` replaced the builtin for
        // the whole importing program (XSS), and `on clamp(x, lo, hi)`
        // made a proven invariant false
        let user_wins = self.handler_shadows_builtin(name, arg_vals.len(), cell_name);
        // Check lambda builtins first (map, filter, find, etc.) — need &mut self
        if !user_wins && arg_vals.iter().any(|v| matches!(v, Value::Lambda { .. } | Value::LambdaBlock { .. })) {
            if let Some(val) = builtins::call_lambda_builtin(self, name, &arg_vals, cell_name) {
                return val.map_err(ExecError::Runtime);
            }
        }
        if name == "transition" {
            self.transition_env = Some(env.clone());
        }
        let builtin_result = if user_wins { None } else { self.call_builtin(name, &arg_vals, cell_name) };
        if let Some(val) = builtin_result {
            val.map_err(ExecError::Runtime)
        }
        // Then check for recursive call to current signal — use cached handler
        else if name == signal_name {
            // Fast path: use cached handler to avoid HashMap lookup + key allocation
            let cached = self.current_handler.as_ref().and_then(|(ch_cell, ch_sig, ch_params, ch_body)| {
                if ch_cell == cell_name && ch_sig == signal_name {
                    Some((Arc::clone(ch_params), Arc::clone(ch_body)))
                } else {
                    None
                }
            });
            if let Some((params, body)) = cached {
                self.call_signal_resolved(cell_name, signal_name, arg_vals, &params, &body)
                    .map_err(ExecError::Runtime)
            } else {
                self.call_signal(cell_name, signal_name, arg_vals)
                    .map_err(ExecError::Runtime)
            }
        }
        // Is it a call to another cell's signal?
        else {
            // Try to find a cell with a matching on-handler: the
            // CALLING cell's own handler first, then declaration
            // order (a HashMap walk ran another cell's same-named
            // handler for a bare call inside a cell that defines it)
            let defines = |cn: &str| self.cells.get(cn).map_or(false, |c| c.sections.iter().any(|s| {
                matches!(&s.node, Section::OnSignal(on) if on.signal_name == *name)
            }));
            // a rule of a test cell: that test cell's own helper (a
            // same-named helper of another test cell ran instead)
            let test_cell = if cell_name.is_empty() { self.current_test_cell.clone().filter(|t| defines(t)) } else { None };
            let found_cell = if defines(cell_name) {
                Some(cell_name.to_string())
            } else if let Some(t) = test_cell {
                Some(t)
            } else {
                let mut all: Vec<&String> = self.cells.keys().filter(|cn| defines(cn)).collect();
                all.sort_by_key(|c| self.cell_order.iter().position(|o| o == *c).unwrap_or(usize::MAX));
                all.first().map(|c| (*c).clone())
            };

            if let Some(target_cell) = found_cell {
                self.call_signal(&target_cell, name, arg_vals)
                    .map_err(ExecError::Runtime)
            } else {
                // A builtin that refused the call (wrong argument
                // count or kinds) is not "undefined": show its signature.
                if let Some(b) = builtins::registry::BUILTINS.iter()
                    .find(|b| b.name == name && b.category != "reserved")
                {
                    let kinds: Vec<&str> = arg_vals.iter().map(value_type_name).collect();
                    return Err(ExecError::Runtime(RuntimeError::TypeError(format!(
                        "{} — called with {} argument{} ({})",
                        b.signature, arg_vals.len(), if arg_vals.len() == 1 { "" } else { "s" },
                        if kinds.is_empty() { "none".to_string() } else { kinds.join(", ") }
                    ))));
                }
                // Collect known names for "did you mean?" suggestion.
                // Builtins come from the registry (single source of
                // truth) so the suggester can never advertise a name
                // that is not actually callable.
                let mut all_names: Vec<String> = self.handler_cache.keys()
                    .map(|(_, sig)| sig.clone())
                    .collect();
                for b in builtins::registry::BUILTINS.iter().filter(|b| b.category != "reserved") {
                    all_names.push(b.name.to_string());
                }

                let suggestion = all_names.iter()
                    .filter(|n| n.as_str() != name)
                    .filter(|n| levenshtein(n, name) <= 3)
                    .min_by_key(|n| levenshtein(n, name))
                    .cloned()
                    .or_else(|| {
                        // Fallback: prefix match (e.g. "length" starts with "len")
                        all_names.iter()
                            .find(|n| n.as_str() != name
                                && (name.starts_with(n.as_str()) || n.starts_with(name)))
                            .cloned()
                    });

                if let Some(did_you_mean) = suggestion {
                    Err(ExecError::Runtime(RuntimeError::UndefinedFn(
                        format!("{} (did you mean '{}'?)", name, did_you_mean)
                    )))
                } else {
                    Err(ExecError::Runtime(RuntimeError::UndefinedFn(name.clone())))
                }
            }
        }
    }

    fn handler_shadows_builtin(&self, name: &str, argc: usize, cell_name: &str) -> bool {
        self.user_handler_takes(name, argc)
            && (!crate::checker::names::builtin_names().contains(name) || self.cell_defines_handler(cell_name, name))
    }

    /// Optimizations may bypass only the unmodified builtin dispatcher.
    fn builtin_fast_path(&self, name: &str, argc: usize, env: &Env, cell_name: &str) -> bool {
        !env.contains_key(name) && !self.variant_registry.contains_key(name)
            && !self.handler_shadows_builtin(name, argc, cell_name)
            && !self.handler_stubs.get(name).is_some_and(|q| !q.is_empty())
    }

    fn user_handler_takes(&self, name: &str, argc: usize) -> bool {
        self.handler_arities.get(name).is_some_and(|a| a.contains(&argc))
    }

    /// Native function boundary. The names here correspond to `native "name"`
    /// in `cell builtin` definitions. This is the thin kernel — everything
    /// above is Soma. Delegates to sub-modules in builtins/.
    pub fn call_builtin(&mut self, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
        // `mock http_post map(...)` in a test cell scripts a builtin like a
        // handler (it used to be accepted and ignored — the test then made
        // a real network call)
        if !self.handler_stubs.is_empty() {
            if let Some(answer) = self.handler_stubs.get_mut(name).and_then(|q| q.pop_front()) {
                let is_http = name.starts_with("http_");
                return Some(match answer {
                    Ok(v) => Ok(v),
                    Err(msg) => {
                        let (kind, detail) = match msg.split_once(": ") {
                            Some((k, d)) if !k.is_empty() && !k.contains(' ') => (k.to_string(), d.to_string()),
                            _ => (msg.clone(), msg.clone()),
                        };
                        if is_http {
                            // http_* never raise: a scripted failure is the error map
                            let status = kind.strip_prefix("status_").and_then(|c| c.parse::<i64>().ok()).unwrap_or(0);
                            Ok(map_from_pairs(vec![
                                ("error".to_string(), Value::String(format!("{}: {}", kind, detail))),
                                ("kind".to_string(), Value::String(if status > 0 { "http_status".to_string() } else { kind.clone() })),
                                ("status".to_string(), Value::Int(SomaInt::from_i64(status))),
                                ("body".to_string(), Value::Unit),
                            ]))
                        } else {
                            Err(RuntimeError::Domain { kind: kind.clone(), message: format!("{}: {}", kind, detail) })
                        }
                    }
                });
            }
        }
        // `with(record, "field", v)` (also `xs[0].qty = v`): the declared
        // field type holds, as for `l.qty = v`
        if name == "with" && args.len() >= 3 && !self.handler_shadows_builtin(name, args.len(), cell_name) {
            if let Value::Variant { type_name, variant, fields: VariantValue::Struct(_) } = &args[0] {
                if let Some(crate::ast::VariantFields::Struct(fields)) = self.variant_fields.get(&(type_name.clone(), variant.clone())).cloned() {
                    let mut coerced = args.to_vec();
                    let mut i = 1;
                    while i + 1 < coerced.len() {
                        let key = format!("{}", coerced[i]);
                        if let Some((_, t)) = fields.iter().find(|(f, _)| *f == key) {
                            let v = match self.coerce_records(&t.node, coerced[i + 1].clone()) { Ok(v) => v, Err(e) => return Some(Err(RuntimeError::TypeError(format!("{}.{}: {}", variant, key, e)))) };
                            if let Err(e) = self.value_fits(&t.node, &v) { return Some(Err(RuntimeError::TypeError(format!("{}.{}: {}", variant, key, e)))); }
                            coerced[i + 1] = v;
                        }
                        i += 2;
                    }
                    return builtins::call_builtin(self, name, &coerced, cell_name);
                }
            }
        }
        if self.test_auto_mock && name.starts_with("http_") && !self.net_noted {
            self.net_noted = true;
            if let Some(Value::String(url)) = args.first() {
                eprintln!("note: this test makes a REAL network call ({} {}) — script it with `mock {} map(...)` or `mock {} error \"timeout: …\"`", name, url, name, name);
            }
        }
        // `mock now 1000` in a test: a frozen clock (now / now_ms / today
        // derive from it) — sticky until the next `mock now`
        if let Some(frozen_ms) = self.frozen_now {
            let frozen = frozen_ms.div_euclid(1000);
            match name {
                "now" => return Some(Ok(Value::Int(SomaInt::from_i64(frozen)))),
                "now_ms" => return Some(Ok(Value::Int(SomaInt::from_i64(frozen_ms)))),
                "today" => return Some(Ok(Value::String(builtins::time::format_unix_date(frozen)))),
                _ => {}
            }
        }
        let out = builtins::call_builtin(self, name, args, cell_name);
        // `from_json(text)` names a declared variant: it must have the
        // declared shape, or a match `soma check` proved exhaustive raised
        // "non-exhaustive match" at run time (a `Charged` without `tx`)
        if name == "from_json" {
            if let Some(Ok(v)) = &out {
                if let Err(m) = self.decoded_variants_ok(v) {
                    return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("from_json: {}", m) }));
                }
            }
        }
        out
    }

    fn decoded_variants_ok(&self, v: &Value) -> Result<(), String> {
        match v {
            Value::Variant { type_name, variant, fields } => {
                // an UNDECLARED type is refused too: `{"_type": "Nope",
                // "_variant": "Charged", "tx": 5}` matched the real Pay.Charged
                // arm with a mistyped field
                if !self.type_variants.contains_key(type_name.as_str()) {
                    return Err(format!("no type `{}` is declared (a `_type` / `_variant` object names a declared sum type)", type_name));
                }
                self.variant_ok(type_name, variant, fields)?;
                match fields {
                    VariantValue::Unit => Ok(()),
                    VariantValue::Tuple(vs) => vs.iter().try_for_each(|x| self.decoded_variants_ok(x)),
                    VariantValue::Struct(m) => m.values().try_for_each(|x| self.decoded_variants_ok(x)),
                }
            }
            Value::List(xs) => xs.iter().try_for_each(|x| self.decoded_variants_ok(x)),
            Value::Map(m) => m.values().try_for_each(|x| self.decoded_variants_ok(x)),
            _ => Ok(()),
        }
    }

    /// Execute a state transition
    /// A lambda that calls transition() captures the names the transition
    /// guards read (the calling handler's locals): `ids |> map(i =>
    /// transition(i, "b"))` raised "undefined variable: cents" in a guard
    /// `soma check` had accepted.
    fn add_guard_names(&self, names: &mut HashSet<String>) {
        if !names.contains("transition") { return; }
        for cell in self.cells.values() {
            for sec in &cell.sections {
                if let Section::State(sm) = &sec.node {
                    for t in &sm.transitions {
                        if let Some(g) = &t.node.guard { free_names_expr(&g.node, names); }
                    }
                }
            }
        }
    }

    pub(crate) fn do_transition(&mut self, id: &str, target: &str) -> Result<Value, RuntimeError> {
        self.do_transition_for("", id, target)
    }

    pub(crate) fn do_transition_for(&mut self, cell_name: &str, id: &str, target: &str) -> Result<Value, RuntimeError> {
        self.do_transition_from_for(cell_name, id, None, target)
    }

    /// `transition(id, from, to)`: the handler declares where the instance
    /// must be; anywhere else is an invalid transition even when an edge
    /// into `to` exists from the actual state.
    pub(crate) fn do_transition_from_for(&mut self, cell_name: &str, id: &str, expected_from: Option<&str>, target: &str) -> Result<Value, RuntimeError> {
        let (sm, status_slot) = self.find_state_machine_for(cell_name)
            .ok_or_else(|| RuntimeError::TypeError(format!(
                "transition(): cell '{}' has no state machine{} — a transition moves the machine of the calling cell: call a handler of the cell that owns it",
                cell_name, if self.state_machines.len() > 1 { " and the program has several" } else { "" })))?;

        // Get current state
        let current = status_slot.get(id)
            .map(|v| match v {
                crate::runtime::storage::StoredValue::String(s) => s,
                _ => format!("{}", v),
            })
            .unwrap_or(sm.initial.clone());

        if let Some(from) = expected_from {
            if from != current {
                return Err(RuntimeError::RequireFailed(format!(
                    "invalid transition: {} → {} — transition(id, \"{}\", \"{}\") declares its source, and '{}' is in '{}', not '{}'",
                    current, target, from, target, id, current, from
                )));
            }
        }

        // Find matching transition
        let transition = sm.transitions.iter().find(|t| {
            (t.node.from == current || (t.node.from == "*" && sm.wildcard_applies(&t.node, &current))) && t.node.to == target
        });

        let transition = match transition {
            Some(t) => t,
            None => {
                let valid: Vec<String> = sm.transitions.iter()
                    .filter(|t| t.node.from == current || (t.node.from == "*" && sm.wildcard_applies(&t.node, &current)))
                    .map(|t| t.node.to.clone())
                    .collect();
                return Err(RuntimeError::RequireFailed(format!(
                    "invalid transition: {} → {}. Current state: '{}'. Valid targets: [{}]",
                    current, target, current, valid.join(", ")
                )));
            }
        };

        // Clone guard if present (to release borrow on self before eval_expr)
        let guard_clone = transition.node.guard.as_ref().map(|g| g.node.clone());

        // Evaluate guard expression if present
        let caller_env = self.transition_env.take();
        if let Some(guard_expr) = guard_clone {
            // Guard scope: the locals of the handler that called
            // transition(), the cell's memory slots, and _id / _from / _to.
            let mut env = caller_env.unwrap_or_default();
            env.insert("_from".to_string(), Value::String(current.clone()));
            env.insert("_to".to_string(), Value::String(target.to_string()));
            env.insert("_id".to_string(), Value::String(id.to_string()));
            let result = self.eval_expr(&guard_expr, &mut env, cell_name, "")
                .map_err(|e| match e {
                    ExecError::Runtime(r) => r,
                    ExecError::Return(v) => RuntimeError::TypeError(format!("guard returned {:?}", v)),
                    _ => RuntimeError::TypeError("guard evaluation failed".to_string()),
                })?;
            match result {
                Value::Bool(true) => {} // guard passed
                Value::Bool(false) | Value::Unit => {
                    // name the condition: "condition is false" told nobody which
                    return Err(RuntimeError::RequireFailed(format!(
                        "guard failed for transition {} → {}: `{}` is false",
                        current, target, crate::ast::render_expr(&guard_expr)
                    )));
                }
                _ => {
                    return Err(RuntimeError::TypeError(format!(
                        "guard must return Bool, got {}",
                        value_type_name(&result)
                    )));
                }
            }
        }

        // Invariants that tie the lifecycle to the data (`status`): the
        // slots of this cell keyed by the instance id are re-checked with the
        // TARGET state before the move (a `released` escrow with its balance
        // still full was invisible to every rule)
        let status_slots = self.slots_reading_status(cell_name);
        if !status_slots.is_empty() {
            self.pending_status = Some((id.to_string(), target.to_string()));
            for slot in &status_slots {
                let val = self.call_storage_method(cell_name, slot, "get", &[Value::String(id.to_string())]).unwrap_or(Value::Unit);
                let size = self.storage.get(&format!("{}.{}", cell_name, slot)).map(|b| b.len()).unwrap_or(0) as i64;
                let checked = self.check_invariants(cell_name, slot, id, &val, size, "transition");
                if let Err(e) = checked {
                    self.pending_status = None;
                    return Err(match e {
                        ExecError::Runtime(r) => r,
                        other => RuntimeError::TypeError(format!("invariant evaluation failed: {:?}", other)),
                    });
                }
            }
            self.pending_status = None;
        }

        // Re-find state machine storage after potential mutation from eval_expr
        let (_sm, status_slot) = self.find_state_machine_for(cell_name)
            .ok_or_else(|| RuntimeError::TypeError("no state machine found".to_string()))?;

        // Perform transition
        let status_backend = status_slot.clone();
        let prev_status = status_backend.get(id);
        status_backend.set(id, crate::runtime::storage::StoredValue::String(target.to_string()));
        if let Some(j) = self.journal.as_mut() {
            j.push(UndoOp::Restore { backend: status_backend, key: id.to_string(), prev: prev_status });
        }

        self.check_storage_write()?;

        // V1.6: structured trace entry — TraceStep::Transition variant.
        self.agent_trace.push(
            crate::interpreter::builtins::llm::trace_transition(id, &current, target)
        );

        Ok(map_from_pairs(vec![
            ("id".to_string(), Value::String(id.to_string())),
            ("from".to_string(), Value::String(current)),
            ("to".to_string(), Value::String(target.to_string())),
        ]))
    }

    pub(crate) fn do_get_status(&self, id: &str) -> Result<Value, RuntimeError> {
        self.do_get_status_for("", id)
    }

    pub(crate) fn do_get_status_for(&self, cell_name: &str, id: &str) -> Result<Value, RuntimeError> {
        let (sm, status_slot) = self.find_state_machine_for(cell_name)
            .ok_or_else(|| RuntimeError::TypeError("no state machine found".to_string()))?;

        let current = status_slot.get(id)
            .map(|v| match v {
                crate::runtime::storage::StoredValue::String(s) => s,
                _ => format!("{}", v),
            })
            .unwrap_or(sm.initial.clone());

        Ok(Value::String(current))
    }

    /// True when `id` has a recorded state, i.e. it was transitioned at
    /// least once. `get_status` answers the initial state for unknown ids,
    /// which is indistinguishable from a fresh instance — this is the check.
    /// "List" / "Map" when `name` is a memory slot of `cell_name` (or, for
    /// cells without a declaration in scope, of any cell), else None.
    pub(crate) fn slot_kind(&self, cell_name: &str, name: &str) -> Option<&'static str> {
        if !self.storage.contains_key(name) && !self.storage.contains_key(&format!("{}.{}", cell_name, name)) {
            return None;
        }
        let declared = |cell: &CellDef| cell.sections.iter().find_map(|s| match s.node {
            Section::Memory(ref mem) => mem.slots.iter().find(|sl| sl.node.name == name).map(|sl| {
                match &sl.node.ty.node {
                    TypeExpr::Generic { name: t, .. } | TypeExpr::Simple(t) if matches!(t.as_str(), "List" | "Log" | "Ledger") => "List",
                    _ => "Map",
                }
            }),
            _ => None,
        });
        self.cells.get(cell_name).and_then(declared)
            .or_else(|| self.cells.values().find_map(declared))
            .or(Some("Map"))
    }

    /// The cell that DECLARES `slot`: a `cell test` helper writing
    /// `inv.set(..)` by bare name ran with the test cell's name, found no
    /// invariant under it and wrote past every rule, `[immutable]` included
    fn slot_owner<'a>(&self, cell_name: &'a str, slot: &str) -> String {
        let declares = |c: &CellDef| c.sections.iter().any(|s| matches!(&s.node, Section::Memory(m) if m.slots.iter().any(|sl| sl.node.name == slot)));
        if self.cells.get(cell_name).map_or(false, |c| declares(c)) { return cell_name.to_string(); }
        let mut owners = self.cell_order.iter().filter(|n| self.cells.get(*n).map_or(false, |c| c.kind != CellKind::Test && declares(c)));
        match (owners.next(), owners.next()) {
            (Some(o), None) => o.clone(),
            _ => cell_name.to_string(),
        }
    }

    /// `[immutable]` slot: entries never change after they are written —
    /// append / add a new key only (the property was a promise nothing
    /// enforced; an audit log's tail could be deleted)
    fn slot_immutable(&self, cell_name: &str, name: &str) -> bool {
        let owner = self.slot_owner(cell_name, name);
        let cell_name = owner.as_str();
        let declared = |cell: &CellDef| cell.sections.iter().find_map(|s| match s.node {
            Section::Memory(ref mem) => mem.slots.iter().find(|sl| sl.node.name == name)
                .map(|sl| sl.node.properties.iter().any(|p| p.node.name() == "immutable")),
            _ => None,
        });
        self.cells.get(cell_name).and_then(declared).unwrap_or(false)
    }

    fn immutable_refusal(slot_name: &str, what: &str) -> ExecError {
        ExecError::Runtime(RuntimeError::RequireFailed(format!(
            "memory invariant violated on '{}': the slot is [immutable] — {} is refused (entries never change after they are written); the slot is unchanged", slot_name, what)))
    }

    /// `Map<String, Int>` / `List<Map>`: the declared value type of a slot
    /// (the last type argument), when it is a plain name.
    /// A value read from a slot: a slot of Strings gives back its text
    /// (`"{\"x\": 1}"` stored in a `Map<String, String>` came back a Map);
    /// JSON text in other slots is legacy data, decoded as before.
    fn from_slot(&self, cell_name: &str, slot_name: &str, v: Value) -> Value {
        // only where a String could not have been stored (a Map / List /
        // record slot, legacy `set(k, to_json(x))`): an `Any` slot gave back
        // a client's text `{"_type": "Admin", …}` as a forged record
        match self.slot_value_type(cell_name, slot_name).as_deref() {
            Some("String") | Some("Any") | None => v,
            _ => auto_deserialize(v),
        }
    }

    /// The declared VALUE type of a slot as a type expression (the last
    /// generic argument: `V` of `Map<K, V>`, `T` of `List<T>`).
    fn slot_inner_type(&self, cell_name: &str, name: &str) -> Option<TypeExpr> {
        let declared = |cell: &CellDef| cell.sections.iter().find_map(|s| match s.node {
            Section::Memory(ref mem) => mem.slots.iter().find(|sl| sl.node.name == name).and_then(|sl| match &sl.node.ty.node {
                TypeExpr::Generic { args, .. } => args.last().map(|a| a.node.clone()),
                _ => None,
            }),
            _ => None,
        });
        self.cells.get(cell_name).and_then(declared)
    }

    /// Does `v` fit the (possibly nested) type `ty`? Only the generic
    /// structure is checked here; the scalar rules are check_slot_value_type's.
    fn value_fits(&self, ty: &TypeExpr, v: &Value) -> Result<(), String> {
        match ty {
            TypeExpr::Generic { name, args } if name == "List" => match v {
                Value::List(xs) => {
                    if let Some(t) = args.first() { for (i, x) in xs.iter().enumerate() { self.value_fits(&t.node, x).map_err(|m| format!("element {}: {}", i, m))?; } }
                    Ok(())
                }
                Value::Unit => Ok(()),
                other => Err(format!("expected a List, got {}", value_type_name(other))),
            },
            TypeExpr::Generic { name, args } if name == "Map" => match v {
                Value::Map(m) | Value::Variant { fields: VariantValue::Struct(m), .. } => {
                    if let Some(t) = args.last() { for (k, x) in m.iter() { self.value_fits(&t.node, x).map_err(|e| format!("key {:?}: {}", k, e))?; } }
                    Ok(())
                }
                Value::Variant { fields: VariantValue::Tuple(xs), .. } => {
                    if let Some(t) = args.last() { for (i, x) in xs.iter().enumerate() { self.value_fits(&t.node, x).map_err(|e| format!("field {}: {}", i, e))?; } }
                    Ok(())
                }
                Value::Variant { fields: VariantValue::Unit, .. } | Value::Unit => Ok(()),
                other => Err(format!("expected a Map, got {}", value_type_name(other))),
            },
            TypeExpr::Simple(t) => {
                if let (true, Value::Variant { type_name, variant, fields }) = (self.type_variants.contains_key(t.as_str()), v) {
                    if type_name != t { return Err(format!("expected {}, got a {} variant", t, type_name)); }
                    return self.variant_ok(type_name, variant, fields);
                }
                let ok = match (t.as_str(), v) {
                    // an element declared Int / Float / String / Bool is not ():
                    // `ls.set("a", [1, ()])` into Map<String, List<Int>> passed
                    // while `rows.push(())` was refused
                    ("Int" | "Float" | "String" | "Bool", Value::Unit) => false,
                    ("Any", _) | (_, Value::Unit) => true,
                    ("Int", Value::Int(_)) | ("Float", Value::Float(_) | Value::Int(_)) | ("String", Value::String(_)) | ("Bool", Value::Bool(_)) => true,
                    ("List", Value::List(_)) | ("Map", Value::Map(_) | Value::Variant { .. }) => true,
                    ("Int" | "Float" | "String" | "Bool" | "List" | "Map", _) => false,
                    (t, Value::Variant { type_name, variant, fields }) if self.type_variants.contains_key(t) => type_name == t && self.variant_ok(type_name, variant, fields).is_ok(),
                    (t, _) if self.type_variants.contains_key(t) => false,
                    _ => true,
                };
                if ok { Ok(()) } else { Err(format!("expected {}, got {} {}", t, value_type_name(v), { let s: String = format!("{}", v).chars().take(30).collect(); s })) }
            }
            _ => Ok(()),
        }
    }

    /// The face's declared return type against a returned value (both
    /// backends: a `[native]` `-> Int` returned 3.5 unchecked).
    fn check_face_return(&self, cell_name: &str, signal_name: &str, val: Value) -> Result<Value, RuntimeError> {
        if signal_name == "request" { return Ok(val); }
        match self.face_return_type(cell_name, signal_name) {
            Some(ret) => check_return_type(signal_name, &ret, val).and_then(|v| match &ret.node {
                // `-> List<Int>` returning ["a"]: the elements too, as for parameters
                TypeExpr::Generic { .. } => self.value_fits(&ret.node, &v)
                    .map(|_| v).map_err(|m| format!("{}(): the face declares `-> {}` but the handler returned {}", signal_name, crate::commands::describe::format_type(&ret.node), m)),
                TypeExpr::Simple(t) if self.type_variants.contains_key(t) => self.value_fits(&ret.node, &v)
                    .map(|_| v).map_err(|m| format!("{}(): the face declares `-> {}` but the handler returned {}", signal_name, t, m)),
                _ => Ok(v),
            }).map_err(RuntimeError::TypeError),
            None => Ok(val),
        }
    }

    /// A variant value against its declaration: the variant exists, and its
    /// payload has the declared fields with the declared types (from_json
    /// built `Charged` with `tx: 5, amt: "x"`, or with no fields at all).
    pub(crate) fn variant_ok(&self, type_name: &str, variant: &str, fields: &VariantValue) -> Result<(), String> {
        let Some(decl) = self.variant_fields.get(&(type_name.to_string(), variant.to_string())) else {
            return Err(format!("{} declares no variant {}", type_name, variant));
        };
        let scalar_fits = |t: &Spanned<TypeExpr>, v: &Value| -> bool {
            match (&t.node, v) {
                (TypeExpr::Simple(t), v) => match (t.as_str(), v) {
                    ("Int", Value::Int(_)) | ("Float", Value::Float(_) | Value::Int(_)) | ("String", Value::String(_)) | ("Bool", Value::Bool(_)) => true,
                    ("Int" | "Float" | "String" | "Bool", _) => false,
                    ("List", Value::List(_)) | ("Map", Value::Map(_) | Value::Variant { .. }) => true,
                    ("List" | "Map", _) => false,
                    (t, Value::Variant { type_name, variant, fields }) if self.type_variants.contains_key(t) => type_name == t && self.variant_ok(type_name, variant, fields).is_ok(),
                    (t, _) if self.type_variants.contains_key(t) => false,
                    _ => true,
                },
                // `xs: List<Int>` / `m: Map<String, Int>` fields: their elements too
                (other, v) => self.value_fits(other, v).is_ok(),
            }
        };
        match (decl, fields) {
            (VariantFields::Unit, VariantValue::Unit) => Ok(()),
            (VariantFields::Unit, VariantValue::Struct(m)) if m.is_empty() => Ok(()),
            (VariantFields::Tuple(ts), VariantValue::Tuple(vs)) if ts.len() == vs.len() => {
                for (i, (t, v)) in ts.iter().zip(vs).enumerate() {
                    if !scalar_fits(t, v) { return Err(format!("{}.{}: field {} is {} {}", type_name, variant, i, value_type_name(v), v)); }
                }
                Ok(())
            }
            (VariantFields::Struct(fs), VariantValue::Struct(m)) => {
                for (n, t) in fs {
                    match m.get(n) {
                        None => return Err(format!("{}.{}: field '{}' is missing", type_name, variant, n)),
                        Some(v) if !scalar_fits(t, v) => return Err(format!("{}.{}: field '{}' expects {}, got {} {}", type_name, variant, n, crate::commands::describe::format_type(&t.node), value_type_name(v), v)),
                        _ => {}
                    }
                }
                if let Some(extra) = m.keys().find(|k| !fs.iter().any(|(n, _)| n == *k)) {
                    return Err(format!("{}.{}: no field '{}'", type_name, variant, extra));
                }
                Ok(())
            }
            _ => Err(format!("{}.{}: the payload does not have the declared shape", type_name, variant)),
        }
    }

    fn slot_value_type(&self, cell_name: &str, name: &str) -> Option<String> {
        let declared = |cell: &CellDef| cell.sections.iter().find_map(|s| match s.node {
            Section::Memory(ref mem) => mem.slots.iter().find(|sl| sl.node.name == name).and_then(|sl| {
                match &sl.node.ty.node {
                    TypeExpr::Generic { args, .. } => args.last().and_then(|a| match &a.node {
                        TypeExpr::Simple(t) => Some(t.clone()),
                        TypeExpr::Generic { name, .. } => Some(name.clone()),
                        _ => None,
                    }),
                    _ => None,
                }
            }),
            _ => None,
        });
        self.cells.get(cell_name).and_then(declared)
            .or_else(|| self.cells.values().find_map(declared))
    }

    /// A write into a slot must match its declared value type (a String in a
    /// `Map<String, Int>` used to be stored silently). Same rules as a
    /// parameter: Int fits Float when representable, `Map` also takes a
    /// record/variant, unknown type names are not checked.
    /// Returns the value to store: an Int written to a `Float` slot is
    /// stored as a Float; everything else must already be of the type (a
    /// whole Float is NOT an Int — `1.0` in `Map<String, Int>` is refused;
    /// a declared sum type takes only its variants).
    fn check_slot_value_type(&self, cell_name: &str, slot_name: &str, val: &Value) -> Result<Value, ExecError> {
        validate_storable(val).map_err(|m| ExecError::Runtime(RuntimeError::Domain {
            kind: "type".to_string(), message: format!("slot '{}': {}", slot_name, m),
        }))?;
        // the NESTED declared types too (`Map<String, List<Int>>` took ["x"])
        if let Some(inner) = self.slot_inner_type(cell_name, slot_name) {
            if let Err(m) = self.value_fits(&inner, val) {
                return Err(ExecError::Runtime(RuntimeError::Domain {
                    kind: "type".to_string(),
                    message: format!("slot '{}': {}", slot_name, m),
                }));
            }
        }
        let Some(ty) = self.slot_value_type(cell_name, slot_name) else { return Ok(val.clone()) };
        let ok = match (ty.as_str(), val) {
            ("Any", _) | ("Int", Value::Int(_)) | ("Float", Value::Float(_))
            | ("String", Value::String(_)) | ("Bool", Value::Bool(_)) | ("List", Value::List(_))
            | ("Map", Value::Map(_) | Value::Variant { .. }) => true,
            ("Float", Value::Int(i)) => return promote_int_to_float(i).map_err(|m| ExecError::Runtime(RuntimeError::TypeError(format!("slot '{}': {}", slot_name, m)))),
            ("Int" | "Float" | "String" | "Bool" | "Map" | "List", _) => false,
            // the variant must be one the type declares: from_json of a
            // client string could build `Pay.Refund` for a Pay without it
            (t, Value::Variant { type_name, variant, fields }) if self.type_variants.contains_key(t) =>
                type_name == t && self.variant_ok(type_name, variant, fields).is_ok(),
            (t, _) if self.type_variants.contains_key(t) => false,
            _ => true,
        };
        if ok { return Ok(val.clone()); }
        let shown: String = format!("{}", val).chars().take(40).collect();
        Err(ExecError::Runtime(RuntimeError::Domain {
            kind: "type".to_string(),
            message: format!("slot '{}' holds {} values, got {} {} — convert the value or change the slot's declared type", slot_name, ty, value_type_name(val), shown),
        }))
    }

    /// The whole content of a slot, for a bare-name read.
    fn materialize_slot(&mut self, cell_name: &str, name: &str) -> Option<Value> {
        let kind = self.slot_kind(cell_name, name)?;
        let backend = self.storage.get(&format!("{}.{}", cell_name, name))
            .or_else(|| if cell_name.is_empty() || self.is_test_cell(cell_name) { self.storage.get(name) } else { None })?.clone();
        Some(match kind {
            "List" => Value::List(backend.list().into_iter().map(|v| self.from_slot(cell_name, name, stored_to_value(v))).collect()),
            _ => {
                let mut m = std::collections::BTreeMap::new();
                for k in backend.keys() {
                    if let Some(v) = backend.get(&k) {
                        m.insert(k, self.from_slot(cell_name, name, stored_to_value(v)));
                    }
                }
                Value::Map(m.into_iter().collect())
            }
        })
    }

    /// The backend that holds `next_id()`'s counter for a cell: its first
    /// declared Map slot, then its own state-machine backend. Migration
    /// must never read a different cell's legacy counter.
    pub(crate) fn next_id_backend(&self, cell_name: &str) -> Option<Arc<dyn StorageBackend>> {
        // only a Map slot can hold a keyed counter (a List backend has no keys)
        if let Some(cell) = self.cells.get(cell_name) {
            for section in &cell.sections {
                if let Section::Memory(ref mem) = section.node {
                    for slot in &mem.slots {
                        let is_list = matches!(&slot.node.ty.node,
                            TypeExpr::Generic { name, .. } | TypeExpr::Simple(name) if name == "List");
                        if is_list { continue; }
                        if let Some(b) = self.storage.get(&format!("{}.{}", cell_name, slot.node.name)) {
                            return Some(b.clone());
                        }
                    }
                }
            }
        }
        if self.state_machines.keys().any(|(owner, _)| owner == cell_name) {
            if let Some((_, backend)) = self.find_state_machine_for(cell_name) {
                return Some(backend.clone());
            }
        }
        None
    }

    /// `Cell.handler(args)` — explicit cross-cell call.
    fn call_cell_handler(&mut self, cell: &str, handler: &str, args: Vec<Value>) -> Result<Value, ExecError> {
        let exists = self.cells.get(cell).map(|c| c.sections.iter().any(|s| {
            matches!(s.node, Section::OnSignal(ref on) if on.signal_name == handler)
        })).unwrap_or(false);
        if !exists {
            return Err(ExecError::Runtime(RuntimeError::UndefinedFn(format!("{cell}.{handler} — cell '{cell}' has no handler '{handler}'"))));
        }
        self.call_signal(cell, handler, args).map_err(ExecError::Runtime)
    }

    pub(crate) fn do_has_state_for(&self, cell_name: &str, id: &str) -> bool {
        match self.find_state_machine_for(cell_name) {
            Some((_, status_slot)) => status_slot.get(id).is_some(),
            None => false,
        }
    }

    pub(crate) fn do_valid_transitions(&self, id: &str) -> Value {
        self.do_valid_transitions_for("", id)
    }

    pub(crate) fn do_valid_transitions_for(&self, cell_name: &str, id: &str) -> Value {
        let Some((sm, status_slot)) = self.find_state_machine_for(cell_name) else {
            return Value::List(vec![]);
        };

        let current = status_slot.get(id)
            .map(|v| match v {
                crate::runtime::storage::StoredValue::String(s) => s,
                _ => format!("{}", v),
            })
            .unwrap_or(sm.initial.clone());

        let targets: Vec<Value> = sm.transitions.iter()
            .filter(|t| t.node.from == current || (t.node.from == "*" && sm.wildcard_applies(&t.node, &current)))
            .map(|t| Value::String(t.node.to.clone()))
            .collect();

        Value::List(targets)
    }

    /// Find the first state machine and its backing storage slot
    pub(crate) fn find_state_machine(&self) -> Option<(&StateMachineSection, &Arc<dyn StorageBackend>)> {
        self.find_state_machine_for("")
    }

    pub(crate) fn find_state_machine_for(&self, cell_name: &str) -> Option<(&StateMachineSection, &Arc<dyn StorageBackend>)> {
        // First: try cell-scoped key (multi-cell programs)
        if !cell_name.is_empty() {
            for ((cn, sm_name), sm) in &self.state_machines {
                if cn == cell_name {
                    let scoped_key = format!("__sm_{}_{}", cn, sm_name);
                    if let Some(backend) = self.storage.get(&scoped_key) {
                        return Some((sm, backend));
                    }
                    // Fallback to legacy key
                    let legacy_key = format!("__sm_{}", sm_name);
                    if let Some(backend) = self.storage.get(&legacy_key) {
                        return Some((sm, backend));
                    }
                }
            }
        }
        // Fallback (a cell without a machine, a test cell): only when the
        // program has exactly ONE machine — with two, a HashMap walk picked
        // one at random and moved some other cell's instance
        let candidates: Vec<_> = self.state_machines.iter().filter_map(|((cn, sm_name), sm)| {
            let scoped_key = format!("__sm_{}_{}", cn, sm_name);
            let legacy_key = format!("__sm_{}", sm_name);
            self.storage.get(&scoped_key).or_else(|| self.storage.get(&legacy_key)).map(|b| (sm, b))
        }).collect();
        if candidates.len() == 1 { candidates.into_iter().next() } else { None }
    }
}

/// Compare numbers without rounding an Int through f64. NaN is unordered.
pub(crate) fn numeric_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y).cmp(&0)),
        (Value::Int(x), Value::Float(y)) => x.to_rug().partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => y.to_rug().partial_cmp(x).map(std::cmp::Ordering::reverse),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        _ => None,
    }
}

pub(crate) fn compare_order(order: Option<std::cmp::Ordering>, op: CmpOp) -> bool {
    use std::cmp::Ordering::*;
    match op {
        CmpOp::Eq => order == Some(Equal),
        CmpOp::Ne => order != Some(Equal),
        CmpOp::Lt => order == Some(Less),
        CmpOp::Gt => order == Some(Greater),
        CmpOp::Le => matches!(order, Some(Less | Equal)),
        CmpOp::Ge => matches!(order, Some(Greater | Equal)),
    }
}

/// Structural equality: numbers by value, lists element-wise, maps by key
/// set, variants by tag and payload. Different kinds are unequal.
pub(crate) fn deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y) == 0,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_)) => numeric_cmp(a, b) == Some(std::cmp::Ordering::Equal),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Unit, Value::Unit) => true,
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| deep_equal(p, q))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| deep_equal(v, w)))
        }
        (
            Value::Variant { type_name: t1, variant: v1, fields: f1 },
            Value::Variant { type_name: t2, variant: v2, fields: f2 },
        ) => t1 == t2 && v1 == v2 && variant_fields_equal(f1, f2),
        _ => false,
    }
}

/// V1.6: structural equality between variant payloads.
fn variant_fields_equal(a: &VariantValue, b: &VariantValue) -> bool {
    match (a, b) {
        (VariantValue::Unit, VariantValue::Unit) => true,
        (VariantValue::Tuple(xs), VariantValue::Tuple(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| value_eq(x, y))
        }
        (VariantValue::Struct(xs), VariantValue::Struct(ys)) => {
            xs.len() == ys.len()
                && xs.iter().all(|(k, x)| ys.get(k).map(|y| value_eq(x, y)).unwrap_or(false))
        }
        _ => false,
    }
}

fn value_eq(a: &Value, b: &Value) -> bool {
    // payloads compare structurally — a Map or List inside a variant used
    // to make `==` false while both sides printed identically
    deep_equal(a, b)
}

/// Validate values before any persistent or in-memory storage conversion.
/// Count the actual encoded layers, including variants and escaped maps.
pub(crate) fn validate_storable(val: &Value) -> Result<(), &'static str> {
    fn storable(v: &Value, depth: usize) -> Result<(), &'static str> {
        let layers = match v {
            Value::Lambda { param, .. } if param != HTTP_MARK => return Err("a function (lambda) cannot be stored — store the data it works on"),
            Value::LambdaBlock { .. } => return Err("a function (lambda) cannot be stored — store the data it works on"),
            Value::List(_) => 1,
            Value::Map(m) => if m.keys().any(|k| k.starts_with("__")) { 2 } else { 1 },
            Value::Variant { fields: VariantValue::Struct(_), .. } => 3,
            Value::Variant { fields: VariantValue::Tuple(_), .. } => 2,
            Value::Variant { fields: VariantValue::Unit, .. } => 1,
            Value::Int(i) if i.to_i64().is_none() => 1,
            Value::Float(f) if !f.is_finite() => 1,
            _ => 0,
        };
        let depth = depth + layers;
        if depth > 100 {
            return Err("a value whose storage encoding is nested deeper than 100 levels cannot be stored — flatten it");
        }
        match v {
            Value::List(xs) | Value::Variant { fields: VariantValue::Tuple(xs), .. } => {
                for x in xs { storable(x, depth)?; }
            }
            Value::Map(m) | Value::Variant { fields: VariantValue::Struct(m), .. } => {
                for x in m.values() { storable(x, depth)?; }
            }
            _ => {}
        }
        Ok(())
    }
    storable(val, 0)
}

/// Convert a runtime Value to a StoredValue
pub(crate) fn value_to_stored(val: &Value) -> StoredValue {
    match val {
        Value::Int(si) => {
            if let Some(n) = si.to_i64() {
                StoredValue::Int(n)
            } else {
                StoredValue::BigInt(format!("{}", si))
            }
        }
        Value::Float(n) => StoredValue::Float(*n),
        Value::String(s) => StoredValue::String(s.clone()),
        Value::Bool(b) => StoredValue::Bool(*b),
        Value::List(items) => StoredValue::List(items.iter().map(value_to_stored).collect()),
        Value::Map(entries) => StoredValue::Map(
            entries.iter()
                .filter(|(k, v)| !(k.as_str() == "_response" && matches!(v, Value::Lambda { param, .. } if param == HTTP_MARK)))
                .map(|(k, v)| (k.clone(), value_to_stored(v))).collect()
        ),
        Value::Lambda { .. } | Value::LambdaBlock { .. } => StoredValue::String("<lambda>".to_string()),
        Value::Variant { type_name, variant, fields } => {
            use crate::runtime::storage::StoredVariantFields;
            let stored_fields = match fields {
                VariantValue::Unit => StoredVariantFields::Unit,
                VariantValue::Tuple(items) => {
                    StoredVariantFields::Tuple(items.iter().map(value_to_stored).collect())
                }
                VariantValue::Struct(entries) => {
                    StoredVariantFields::Struct(
                        entries.iter().map(|(k, v)| (k.clone(), value_to_stored(v))).collect()
                    )
                }
            };
            StoredValue::Variant {
                type_name: type_name.clone(),
                variant: variant.clone(),
                fields: stored_fields,
            }
        }
        Value::Unit => StoredValue::Null,
    }
}

/// Implicit Float promotion must not turn a finite integer into infinity.
fn promote_int_to_float(i: &SomaInt) -> Result<Value, String> {
    let f = i.to_f64();
    if f.is_finite() { Ok(Value::Float(f)) }
    else { Err("this Int is past the Float range (~1.8e308)".to_string()) }
}

/// Declared parameter type vs. the value actually passed. Ints are
/// accepted for Float when representable (promoted); a Float is not an Int.
/// Sum types are validated by check_sum_param after this primitive check.
pub(crate) fn check_param_type(param: &Param, val: Value) -> Result<Value, String> {
    // the ELEMENT type of `List<Int>` / `Map<String, Int>` too (["x", "y"]
    // entered `g(xs: List<Int>)` and failed deep in the body)
    if let TypeExpr::Generic { name, args } = &param.ty.node {
        let elem = args.last().and_then(|a| match &a.node { TypeExpr::Simple(t) => Some(t.as_str()), _ => None });
        let fits = |t: &str, v: &Value| match (t, v) {
            ("Int", Value::Int(_)) | ("Float", Value::Float(_) | Value::Int(_)) | ("String", Value::String(_)) | ("Bool", Value::Bool(_)) => true,
            ("Int" | "Float" | "String" | "Bool", _) => false,
            _ => true,
        };
        if let Some(t) = elem {
            let bad = match (name.as_str(), &val) {
                ("List", Value::List(xs)) => xs.iter().find(|x| !fits(t, x)).cloned(),
                ("Map", Value::Map(m)) => m.values().find(|x| !fits(t, x)).cloned(),
                _ => None,
            };
            if let Some(b) = bad {
                return Err(format!("parameter '{}' expects {}<…{}>, got an element {} {}", param.name, name, t, value_type_name(&b), { let s: String = format!("{}", b).chars().take(30).collect(); s }));
            }
        }
    }
    let ty = match &param.ty.node {
        TypeExpr::Simple(t) => t.as_str(),
        TypeExpr::Generic { name, .. } => name.as_str(),
        _ => return Ok(val),
    };
    let got = value_type_name(&val);
    let shown = || {
        let s = format!("{}", val);
        let s: String = s.chars().take(40).collect();
        s
    };
    match (ty, &val) {
        ("Any", _) | ("Int", Value::Int(_)) | ("Float", Value::Float(_)) | ("String", Value::String(_))
        | ("Bool", Value::Bool(_)) | ("Map", Value::Map(_)) | ("List", Value::List(_)) => Ok(val),
        // `Map` is the corpus's spelling for "a record": variants, an absent
        // record (`()`) and callbacks pass; `List` accepts an absent list.
        // `Map` is the corpus's spelling for "a record": variants, an absent
        // record (`()`, an empty tree) and callbacks pass. A missing value is
        // NOT a List: `f(body.seats)` with `seats` absent ran with `()`
        ("Map", Value::Variant { .. } | Value::Unit | Value::Lambda { .. } | Value::LambdaBlock { .. }) => Ok(val),
        // a Float is not an Int, whatever its value (a computed 1.0 was taken
        // while `[1.0]` for List<Int>, an Int slot and a literal were refused);
        // HTTP / CLI text already converts integral numbers at the boundary
        ("Float", Value::Int(i)) => promote_int_to_float(i).map_err(|m| format!("parameter '{}': {}", param.name, m)),
        ("Int" | "Float" | "String" | "Bool" | "Map" | "List", _) => Err(format!(
            "parameter '{}' expects {}, got {} {}", param.name, ty, got, shown()
        )),
        _ => Ok(val),
    }
}

/// `signal f(...) -> T` vs. the value the handler returned.
pub(crate) fn check_return_type(signal: &str, ret: &Spanned<TypeExpr>, val: Value) -> Result<Value, String> {
    let ty = match &ret.node {
        TypeExpr::Simple(t) => t.as_str(),
        TypeExpr::Generic { name, .. } => name.as_str(),
        _ => return Ok(val),
    };
    let ok = match (ty, &val) {
        ("Any", _) | ("Int", Value::Int(_)) | ("Float", Value::Float(_) | Value::Int(_))
        | ("String", Value::String(_)) | ("Bool", Value::Bool(_)) | ("List", Value::List(_) | Value::Unit)
        | ("Map", Value::Map(_) | Value::Variant { .. } | Value::Unit) => true,
        ("Int", Value::Float(f)) => f.fract() == 0.0,
        ("Int" | "Float" | "String" | "Bool" | "Map" | "List", _) => false,
        _ => true,
    };
    if ok {
        Ok(val)
    } else {
        let shown: String = format!("{}", val).chars().take(40).collect();
        Err(format!(
            "{}(): the face declares `-> {}` but the handler returned {} {}",
            signal, ty, value_type_name(&val), shown
        ))
    }
}

/// An error escaping a lambda keeps its identity: a `fail("kind", …)`
/// inside `xs |> map(…)` must reach the caller's `try` as that kind, not as
/// a Debug dump of the Rust enum under kind "type".
pub(crate) fn lambda_error(e: ExecError) -> RuntimeError {
    match e {
        ExecError::Runtime(r) => r,
        ExecError::Return(v) => RuntimeError::TypeError(format!("lambda used `return` (value {}) — a lambda is an expression: `x => expr`", v)),
        ExecError::Break | ExecError::Continue => RuntimeError::TypeError("break/continue inside a lambda".to_string()),
    }
}

/// One atomic unit in progress (see `Interpreter::unit_begin`).
pub(crate) struct Unit {
    _serial: std::sync::MutexGuard<'static, ()>,
    txn: Option<Arc<std::sync::Mutex<rusqlite::Connection>>>,
    _cross: Option<CrossProcessLock>,
}

/// Exclusive lock shared by every soma process using this directory's
/// `.soma_data` (held for one handler invocation). No persistent storage
/// here → no lock.
struct CrossProcessLock(Option<rusqlite::Connection>);

impl CrossProcessLock {
    fn acquire() -> Result<Self, RuntimeError> {
        if IN_TEST.load(std::sync::atomic::Ordering::Relaxed) { return Ok(CrossProcessLock(None)); }
        let dir = crate::runtime::storage::data_dir();
        if !dir.is_dir() {
            return Ok(CrossProcessLock(None));
        }
        let err = |e| RuntimeError::StorageTransaction(format!("cannot acquire storage lock: {e}"));
        let conn = rusqlite::Connection::open(dir.join("lock.db")).map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_secs(120)).map_err(err)?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS lock (k INTEGER PRIMARY KEY); BEGIN IMMEDIATE").map_err(err)?;
        Ok(CrossProcessLock(Some(conn)))
    }
}

impl Drop for CrossProcessLock {
    fn drop(&mut self) {
        if let Some(c) = self.0.take() {
            let _ = c.execute_batch("COMMIT");
        }
    }
}

pub(crate) fn slot_assign_message(name: &str, kind: Option<&str>) -> String {
    match kind {
        Some("List") => format!(
            "'{name}' is a memory slot, not a variable — it is never assigned as a whole: append with {name}.push(x), read it with {name} (all items) or {name}.all"
        ),
        _ => format!(
            "'{name}' is a memory slot, not a variable — it is never assigned as a whole: write one entry with {name}.set(key, value) or {name}[key] = value, read with {name}.get(key)"
        ),
    }
}

pub(crate) fn stored_to_value(stored: StoredValue) -> Value {
    use crate::runtime::storage::StoredVariantFields;
    match stored {
        StoredValue::Int(n) => Value::Int(SomaInt::from_i64(n)),
        // damaged digits read as 0 (a balance silently reset): they come
        // back as the text they are, which the start-up audit reports and
        // arithmetic refuses
        StoredValue::BigInt(d) => {
            let t = d.trim();
            let digits = t.strip_prefix('-').unwrap_or(t);
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) { Value::Int(SomaInt::from_decimal_str(t)) } else { Value::String(d) }
        }
        StoredValue::Float(n) => Value::Float(n),
        StoredValue::String(s) => Value::String(s),
        StoredValue::Bool(b) => Value::Bool(b),
        StoredValue::Null => Value::Unit,
        StoredValue::List(items) => Value::List(items.into_iter().map(stored_to_value).collect()),
        StoredValue::Map(map) => Value::Map(
            map.into_iter().map(|(k, v)| (k, stored_to_value(v))).collect()
        ),
        StoredValue::Variant { type_name, variant, fields } => {
            let v_fields = match fields {
                StoredVariantFields::Unit => VariantValue::Unit,
                StoredVariantFields::Tuple(items) => {
                    VariantValue::Tuple(items.into_iter().map(stored_to_value).collect())
                }
                StoredVariantFields::Struct(entries) => {
                    VariantValue::Struct(
                        entries.into_iter().map(|(k, v)| (k, stored_to_value(v))).collect()
                    )
                }
            };
            Value::Variant { type_name, variant, fields: v_fields }
        }
    }
}

/// Auto-deserialize: if a Value::String looks like JSON (starts with { or [),
/// parse it into a Map or List. This handles the common case where old code
/// used to_json() before .set(), making .get() return a raw JSON string.
pub(crate) fn auto_deserialize(val: Value) -> Value {
    if let Value::String(ref s) = val {
        let trimmed = s.trim();
        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                return json_to_value(&parsed);
            }
        }
    }
    val
}

/// Convert serde_json::Value to interpreter Value
pub fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(SomaInt::from_i64(i))
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let entries: indexmap::IndexMap<String, Value> = obj.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::Map(entries)
        }
    }
}

/// Internal error type to handle return-as-control-flow
#[derive(Debug)]
pub(crate) enum ExecError {
    Return(Value),
    Break,
    Continue,
    Runtime(RuntimeError),
}

impl From<RuntimeError> for ExecError {
    fn from(error: RuntimeError) -> Self { Self::Runtime(error) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    /// Convert big SomaInt values to strings before crossing thread boundaries,
    /// then reconstruct on the receiving side. Small ints (inline) survive as-is.
    enum PortableValue {
        Small(Value),
        BigIntStr(String),
    }

    fn to_portable(val: Value) -> PortableValue {
        match &val {
            Value::Int(si) if !si.is_small() => PortableValue::BigIntStr(format!("{}", si)),
            _ => PortableValue::Small(val),
        }
    }

    fn from_portable(pv: PortableValue) -> Value {
        match pv {
            PortableValue::Small(v) => v,
            PortableValue::BigIntStr(s) => {
                let bi = s.parse::<num_bigint::BigInt>().unwrap_or_default();
                Value::Int(SomaInt::from_bigint(&bi))
            }
        }
    }

    fn run(source: &str, cell: &str, signal: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        // Run on a thread with 8 MB stack so recursive tests don't SIGABRT
        let source = source.to_string();
        let cell = cell.to_string();
        let signal = signal.to_string();
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let mut lexer = Lexer::new(&source);
                let tokens = lexer.tokenize().unwrap();
                let mut parser = Parser::new(tokens);
                let program = parser.parse_program().unwrap();
                let mut interp = Interpreter::new(&program);
                // Set up storage for memory sections
                for prog_cell in &program.cells {
                    for section in &prog_cell.node.sections {
                        if let crate::ast::Section::Memory(ref mem) = section.node {
                            let mut slots = std::collections::HashMap::new();
                            for slot in &mem.slots {
                                let backend: std::sync::Arc<dyn crate::runtime::storage::StorageBackend> =
                                    std::sync::Arc::new(crate::runtime::storage::MemoryBackend::new());
                                slots.insert(slot.node.name.clone(), backend);
                            }
                            interp.set_storage(&prog_cell.node.name, &slots);
                        }
                    }
                }
                interp.ensure_state_machine_storage();
                // Convert big SomaInt to portable form before thread exit
                interp.call_signal(&cell, &signal, args).map(to_portable)
            })
            .expect("failed to spawn test thread")
            .join()
            .expect("test thread panicked")
            .map(from_portable)
    }

    #[test]
    fn test_factorial() {
        let source = r#"
            cell Fact {
                on compute(n: Int) {
                    if n <= 1 {
                        return 1
                    }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(SomaInt::from_i64(5))]).unwrap();
        assert_eq!(result.as_int().unwrap(), 120);
    }

    #[test]
    fn test_factorial_int_auto_promotes_to_bigint() {
        // Int overflow auto-promotes to BigInt instead of erroring
        let source = r#"
            cell Fact {
                on compute(n: Int) {
                    if n <= 1 { return 1 }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(SomaInt::from_i64(30))]);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val.is_big());
        assert_eq!(val.to_string(), "265252859812191058636308480000000");
    }

    #[test]
    fn test_factorial_bigint() {
        // BigInt handles arbitrary precision
        let source = r#"
            cell Fact {
                on compute(n: BigInt) {
                    if n <= 1 { return 1 }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(SomaInt::from_i64(30))]).unwrap();
        assert!(result.is_big());
        assert_eq!(
            result.to_string(),
            "265252859812191058636308480000000"
        );
    }

    #[test]
    fn test_fibonacci() {
        let source = r#"
            cell Fib {
                on compute(n: Int) {
                    if n <= 1 {
                        return n
                    }
                    return compute(n - 1) + compute(n - 2)
                }
            }
        "#;
        let result = run(source, "Fib", "compute", vec![Value::Int(SomaInt::from_i64(10))]).unwrap();
        assert_eq!(result.as_int().unwrap(), 55);
    }

    #[test]
    fn test_let_and_arithmetic() {
        let source = r#"
            cell Math {
                on add(a: Int, b: Int) {
                    let sum = a + b
                    return sum * 2
                }
            }
        "#;
        let result = run(source, "Math", "add", vec![Value::Int(SomaInt::from_i64(3)), Value::Int(SomaInt::from_i64(4))]).unwrap();
        assert_eq!(result.as_int().unwrap(), 14);
    }

    #[test]
    fn test_if_else() {
        let source = r#"
            cell Logic {
                on max(a: Int, b: Int) {
                    if a > b {
                        return a
                    } else {
                        return b
                    }
                }
            }
        "#;
        let result = run(source, "Logic", "max", vec![Value::Int(SomaInt::from_i64(3)), Value::Int(SomaInt::from_i64(7))]).unwrap();
        assert_eq!(result.as_int().unwrap(), 7);
    }

    // ── Match patterns ──────────────────────────────────────────────

    #[test]
    fn test_match_variable_binding() {
        let source = r#"
            cell T {
                on run(x: Int) {
                    return match x {
                        name -> name + 100
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![Value::Int(SomaInt::from_i64(7))]).unwrap();
        assert_eq!(result.as_int().unwrap(), 107);
    }

    #[test]
    fn test_match_or_pattern() {
        let source = r#"
            cell T {
                on run(s: String) {
                    return match s {
                        "a" || "b" -> "matched"
                        _ -> "no"
                    }
                }
            }
        "#;
        let a = run(source, "T", "run", vec![Value::String("a".into())]).unwrap();
        assert_eq!(a.to_string(), "matched");
        let b = run(source, "T", "run", vec![Value::String("b".into())]).unwrap();
        assert_eq!(b.to_string(), "matched");
        let c = run(source, "T", "run", vec![Value::String("z".into())]).unwrap();
        assert_eq!(c.to_string(), "no");
    }

    #[test]
    fn test_match_map_destructure() {
        let source = r#"
            cell T {
                on run() {
                    let req = map("method", "GET", "path", "/home")
                    return match req {
                        {method: "GET", path} -> path
                        _ -> "other"
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "/home");
    }

    #[test]
    fn test_match_string_prefix() {
        let source = r#"
            cell T {
                on run(s: String) {
                    return match s {
                        "/api/" + rest -> rest
                        _ -> "no match"
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![Value::String("/api/users".into())]).unwrap();
        assert_eq!(result.to_string(), "users");
        let miss = run(source, "T", "run", vec![Value::String("/home".into())]).unwrap();
        assert_eq!(miss.to_string(), "no match");
    }

    #[test]
    fn test_match_guard_clause() {
        let source = r#"
            cell T {
                on run(n: Int) {
                    return match n {
                        x if x > 0 -> "positive"
                        x if x < 0 -> "negative"
                        _ -> "zero"
                    }
                }
            }
        "#;
        let pos = run(source, "T", "run", vec![Value::Int(SomaInt::from_i64(5))]).unwrap();
        assert_eq!(pos.to_string(), "positive");
        let neg = run(source, "T", "run", vec![Value::Int(SomaInt::from_i64(-3))]).unwrap();
        assert_eq!(neg.to_string(), "negative");
        let zero = run(source, "T", "run", vec![Value::Int(SomaInt::from_i64(0))]).unwrap();
        assert_eq!(zero.to_string(), "zero");
    }

    #[test]
    fn test_match_range_pattern() {
        let source = r#"
            cell T {
                on run(n: Int) {
                    return match n {
                        0..10 -> "small"
                        _ -> "big"
                    }
                }
            }
        "#;
        let small = run(source, "T", "run", vec![Value::Int(SomaInt::from_i64(5))]).unwrap();
        assert_eq!(small.to_string(), "small");
        let big = run(source, "T", "run", vec![Value::Int(SomaInt::from_i64(15))]).unwrap();
        assert_eq!(big.to_string(), "big");
    }

    #[test]
    fn test_match_nested_destructure() {
        let source = r#"
            cell T {
                on run() {
                    let req = map("method", "POST", "path", "/api/orders")
                    return match req {
                        {method: "POST", path: "/api/" + r} -> r
                        _ -> "no"
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "orders");
    }

    // ── Expressions ─────────────────────────────────────────────────

    #[test]
    fn test_if_expression() {
        let source = r#"
            cell T {
                on run() {
                    let x = if true { 42 } else { 0 }
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_if_expression_else_if() {
        let source = r#"
            cell T {
                on run() {
                    let x = if false { 1 } else if true { 2 } else { 3 }
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 2);
    }

    #[test]
    fn test_match_as_expression() {
        let source = r#"
            cell T {
                on run() {
                    let x = match "a" {
                        "a" -> 1
                        _ -> 0
                    }
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 1);
    }

    // ── Operators ───────────────────────────────────────────────────

    #[test]
    fn test_compound_minus_eq() {
        let source = r#"
            cell T {
                on run() {
                    let x = 10
                    x -= 3
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 7);
    }

    #[test]
    fn test_compound_star_eq() {
        let source = r#"
            cell T {
                on run() {
                    let x = 5
                    x *= 2
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 10);
    }

    #[test]
    fn test_compound_slash_eq() {
        let source = r#"
            cell T {
                on run() {
                    let x = 20
                    x /= 4
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 5);
    }

    #[test]
    fn test_try_propagate() {
        let source = r#"
            cell T {
                on run() {
                    let v = try { 42 }?
                    return v
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_try_propagate_error() {
        let source = r#"
            cell T {
                on run() {
                    let v = try { 1 / 0 }?
                    return v
                }
            }
        "#;
        // `?` RE-RAISES the error the try caught, with its kind (returning
        // the error map answered 200 and committed earlier writes)
        match run(source, "T", "run", vec![]) {
            Err(RuntimeError::Domain { kind, .. }) => assert_eq!(kind, "division_by_zero"),
            other => panic!("expected the division_by_zero to propagate, got {:?}", other),
        }
    }

    #[test]
    fn test_null_coalesce_precedence() {
        // () ?? 5 should evaluate to 5, and then == 5 should be true
        // This tests that ?? binds tighter than == (or at least correctly)
        let source = r#"
            cell T {
                on run() {
                    let x = () ?? 5
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 5);
    }

    // ── Statements ──────────────────────────────────────────────────

    #[test]
    fn test_ensure_pass() {
        let source = r#"
            cell T {
                on run() {
                    ensure true
                    return "ok"
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "ok");
    }

    #[test]
    fn test_ensure_fail() {
        let source = r#"
            cell T {
                on run() {
                    ensure false
                    return "ok"
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_implicit_return() {
        // Handler without explicit return should return the last expression
        let source = r#"
            cell T {
                on run() {
                    let x = 10
                    let y = 20
                    x + y
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 30);
    }

    // ── Storage auto-deserialize ────────────────────────────────────

    #[test]
    fn test_auto_deserialize_json_string() {
        let json_str = r#"{"name": "alice", "age": 30}"#;
        let val = Value::String(json_str.to_string());
        let result = auto_deserialize(val);
        assert!(matches!(result, Value::Map(_)));
        if let Value::Map(ref entries) = result {
            assert_eq!(entries.get("name").unwrap().to_string(), "alice");
            assert_eq!(entries.get("age").unwrap().as_int().unwrap(), 30);
        }
    }

    // ── Inter-agent communication ──────────────────────────────────

    #[test]
    fn test_emit_dispatches_to_sibling_cell() {
        let source = r#"
            cell A {
                on run() {
                    emit ping(map("from", "A"))
                    return "emitted"
                }
            }
            cell B {
                memory { log: Map<String, String> [ephemeral] }
                on ping(data: Map) {
                    log.set("got", data.from)
                }
            }
        "#;
        let result = run(source, "A", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "emitted");
    }


    #[test]
    fn test_delegate_cross_cell() {
        let source = r#"
            cell Helper {
                on double(n: Int) { return n * 2 }
            }
            cell Main {
                on run() {
                    return delegate("Helper", "double", 21)
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    // ── Agent cell features ────────────────────────────────────────

    #[test]
    fn test_cell_agent_with_state_machine() {
        let source = r#"
            cell agent Bot {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    return get_status("t")
                }
            }
        "#;
        let result = run(source, "Bot", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "done");
    }

    #[test]
    fn test_cell_agent_emit_and_delegate() {
        // Agent A delegates to agent B, B transitions and returns
        let source = r#"
            cell agent Worker {
                state w { initial: idle  idle -> done  * -> failed }
                on process(x: Int) {
                    transition("t", "done")
                    return x * 10
                }
            }
            cell Main {
                on run() {
                    return delegate("Worker", "process", 5)
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 50);
    }

    #[test]
    fn test_state_machine_isolation_between_cells() {
        let source = r#"
            cell agent A {
                state sa { initial: idle  idle -> doneA  * -> fail }
                on go() {
                    transition("t", "doneA")
                    return get_status("t")
                }
            }
            cell agent B {
                state sb { initial: idle  idle -> doneB  * -> fail }
                on go() {
                    transition("t", "doneB")
                    return get_status("t")
                }
            }
            cell Main {
                on run() {
                    let a = delegate("A", "go")
                    let b = delegate("B", "go")
                    return list(a, b)
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items[0].to_string(), "doneA");
            assert_eq!(items[1].to_string(), "doneB");
        } else {
            panic!("expected list");
        }
    }

    // ── Mock mode ──────────────────────────────────────────────────

    #[test]
    fn test_think_mock_and_budget_and_trace() {
        // Combined test to avoid env var races in parallel test execution
        std::env::set_var("SOMA_LLM_MOCK", "echo");

        // Echo mode
        let source = r#"
            cell agent Bot {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    return think("hello world")
                }
            }
        "#;
        let result = run(source, "Bot", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "hello world");

        // Fixed mode
        std::env::set_var("SOMA_LLM_MOCK", "fixed:42");
        let source2 = r#"
            cell agent Bot2 {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    return think("anything")
                }
            }
        "#;
        let result2 = run(source2, "Bot2", "run", vec![]).unwrap();
        assert_eq!(result2.to_string(), "42");

        // Token tracking in mock mode
        std::env::set_var("SOMA_LLM_MOCK", "echo");
        let source3 = r#"
            cell agent Bot3 {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    set_budget(1000)
                    transition("t", "done")
                    think("test")
                    return tokens_used()
                }
            }
        "#;
        // a mocked think() costs an estimate (~4 chars per token) so
        // budgets are testable offline: "test" + "test" = 8 chars → 2
        let result3 = run(source3, "Bot3", "run", vec![]).unwrap();
        assert_eq!(result3.as_int().unwrap(), 2);

        // Trace records think calls
        let source4 = r#"
            cell agent Bot4 {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    think("test prompt")
                    return trace()
                }
            }
        "#;
        let result4 = run(source4, "Bot4", "run", vec![]).unwrap();
        if let Value::List(entries) = result4 {
            assert!(!entries.is_empty(), "trace should have entries");
        } else {
            panic!("expected list from trace()");
        }

        std::env::remove_var("SOMA_LLM_MOCK");
    }

    // ── Transition error messages ──────────────────────────────────

    #[test]
    fn test_transition_typo_shows_valid_states() {
        let source = r#"
            cell T {
                state w { initial: idle  idle -> done }
                memory { x: Map<String, String> [ephemeral] }
                on run() {
                    return transition("t", "doen")
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("done"), "error should show valid state 'done': {}", err);
    }

    #[test]
    fn test_transition_without_memory_section() {
        let source = r#"
            cell T {
                state w { initial: idle  idle -> done }
                on run() {
                    transition("t", "done")
                    return get_status("t")
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "done");
    }
}

/// Run a lexical block without cloning unrelated locals. Assignments persist;
/// `let` bindings are restored even when evaluation raises or returns early.
fn with_lexical_scope<T>(body: &[Spanned<Statement>], env: &mut Env, f: impl FnOnce(&mut Env) -> T) -> T {
    if !body_has_let(body) { return f(env); }
    let saved: Vec<(String, Option<Value>)> = body.iter().filter_map(|stmt| {
        if let Statement::Let { name, .. } = &stmt.node {
            Some((name.clone(), env.get(name).cloned()))
        } else { None }
    }).collect();
    let outcome = f(env);
    for (name, old) in saved {
        match old {
            Some(value) => { env.insert(name, value); }
            None => { env.remove(&name); }
        }
    }
    outcome
}

/// CSS/format text, regex counts and semicolon-containing segments stay literal.
/// Quoted colons in real expressions must not hide calls from static analyses.
/// Does an invariant read the `status` binding (the machine state of the
/// written key)?
pub(crate) fn invariant_reads_status(inv: &Expr) -> bool {
    let mut names: HashSet<String> = HashSet::new();
    free_names_expr(inv, &mut names);
    names.contains("status")
}

pub(crate) fn interp_segment_is_literal(segment: &str) -> bool {
    let (mut quoted, mut escaped, mut colon) = (false, false, false);
    for ch in segment.chars() {
        if escaped { escaped = false; continue; }
        if quoted && ch == '\\' { escaped = true; continue; }
        if ch == '"' { quoted = !quoted; }
        if !quoted && ch == ':' { colon = true; }
    }
    segment.is_empty() || segment.contains(';')
        || segment.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ')
        || (colon && !segment.contains('(') && !segment.contains('['))
}

/// If/match expression bodies can assign caller locals. Called handlers and
/// lambdas have their own environments; inspect their argument expressions only.
fn expr_may_assign(expr: &Expr) -> bool {
    match expr {
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            !then_body.is_empty() || !else_body.is_empty()
                || expr_may_assign(&condition.node) || expr_may_assign(&then_result.node) || expr_may_assign(&else_result.node)
        }
        Expr::Match { subject, arms } => expr_may_assign(&subject.node) || arms.iter().any(|arm| {
            !arm.body.is_empty() || arm.guard.as_ref().map_or(false, |g| expr_may_assign(&g.node)) || expr_may_assign(&arm.result.node)
        }),
        Expr::Literal(Literal::String(s)) => crate::checker::desugar::segments(s).iter()
            .filter_map(|seg| crate::checker::desugar::parse_segment(seg)).any(|e| expr_may_assign(&e)),
        Expr::FieldAccess { target, .. } => expr_may_assign(&target.node),
        Expr::Index { target, index } => expr_may_assign(&target.node) || expr_may_assign(&index.node),
        Expr::MethodCall { target, args, .. } => expr_may_assign(&target.node) || args.iter().any(|a| expr_may_assign(&a.node)),
        Expr::FnCall { args, .. } | Expr::ListLiteral(args) => args.iter().any(|a| expr_may_assign(&a.node)),
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => expr_may_assign(&left.node) || expr_may_assign(&right.node),
        Expr::Not(x) | Expr::Try(x) | Expr::TryPropagate(x) => expr_may_assign(&x.node),
        Expr::Record { fields, .. } => fields.iter().any(|(_, x)| expr_may_assign(&x.node)),
        Expr::Literal(_) | Expr::Ident(_) | Expr::Lambda { .. } | Expr::LambdaBlock { .. } => false,
    }
}

/// Every name a lambda body could read — an over-approximation on purpose
/// (identifiers, call names, `{name…}` inside string literals): capturing
/// a name the body never uses costs a clone, missing one is a runtime error.
pub(crate) fn free_names_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Literal(Literal::String(s)) => {
            for seg in crate::checker::desugar::segments(s) {
                if let Some(expr) = crate::checker::desugar::parse_segment(&seg) {
                    free_names_expr(&expr, out);
                }
            }
        }
        Expr::Literal(_) => {}
        Expr::Ident(n) => { out.insert(n.clone()); }
        Expr::FieldAccess { target, .. } => free_names_expr(&target.node, out),
        Expr::Index { target, index } => { free_names_expr(&target.node, out); free_names_expr(&index.node, out); }
        Expr::MethodCall { target, args, .. } => { free_names_expr(&target.node, out); for a in args { free_names_expr(&a.node, out); } }
        Expr::FnCall { name, args } => { out.insert(name.clone()); for a in args { free_names_expr(&a.node, out); } }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => {
            free_names_expr(&left.node, out); free_names_expr(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => free_names_expr(&i.node, out),
        Expr::Record { fields, .. } => { for (_, v) in fields { free_names_expr(&v.node, out); } }
        Expr::Match { subject, arms } => {
            free_names_expr(&subject.node, out);
            for arm in arms {
                if let Some(g) = &arm.guard { free_names_expr(&g.node, out); }
                free_names_stmts(&arm.body, out);
                free_names_expr(&arm.result.node, out);
            }
        }
        Expr::Lambda { body, .. } => free_names_expr(&body.node, out),
        Expr::LambdaBlock { stmts, result, .. } => { free_names_stmts(stmts, out); free_names_expr(&result.node, out); }
        Expr::ListLiteral(items) => { for i in items { free_names_expr(&i.node, out); } }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            free_names_expr(&condition.node, out);
            free_names_stmts(then_body, out); free_names_expr(&then_result.node, out);
            free_names_stmts(else_body, out); free_names_expr(&else_result.node, out);
        }
    }
}

pub(crate) fn free_names_stmts(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
    for st in stmts {
        match &st.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => { out.insert(name.clone()); free_names_expr(&value.node, out); }
            Statement::Return { value } | Statement::Ensure { condition: value } | Statement::ExprStmt { expr: value } => free_names_expr(&value.node, out),
            Statement::If { condition, then_body, else_body } => { free_names_expr(&condition.node, out); free_names_stmts(then_body, out); free_names_stmts(else_body, out); }
            Statement::For { var, iter, body, .. } => { out.insert(var.clone()); free_names_expr(&iter.node, out); free_names_stmts(body, out); }
            Statement::While { condition, body, .. } => { free_names_expr(&condition.node, out); free_names_stmts(body, out); }
            Statement::Emit { args, .. } => { for a in args { free_names_expr(&a.node, out); } }
            Statement::Require { constraint, else_signal } => {
                free_names_constraint(&constraint.node, out);
                free_names_expr(&Expr::Literal(Literal::String(else_signal.clone())), out);
            }
            Statement::MethodCall { target, args, .. } => { out.insert(target.clone()); for a in args { free_names_expr(&a.node, out); } }
            Statement::IndexSet { name, index, value } => { out.insert(name.clone()); free_names_expr(&index.node, out); free_names_expr(&value.node, out); }
            Statement::Break | Statement::Continue => {}
        }
    }
}

fn free_names_constraint(c: &Constraint, out: &mut HashSet<String>) {
    match c {
        Constraint::Comparison { left, right, .. } => { free_names_expr(&left.node, out); free_names_expr(&right.node, out); }
        Constraint::Predicate { args, .. } => { for a in args { free_names_expr(&a.node, out); } }
        Constraint::And(a, b) | Constraint::Or(a, b) => { free_names_constraint(&a.node, out); free_names_constraint(&b.node, out); }
        Constraint::Not(i) => free_names_constraint(&i.node, out),
        Constraint::Descriptive(_) => {}
    }
}

/// A value as a message shows it: Soma's own rendering, at most 40 chars.
pub(crate) fn short_value(v: &Value) -> String {
    let t = match v { Value::String(s) => format!("{:?}", s), other => format!("{}", other) };
    if t.chars().count() > 40 { format!("{}…", t.chars().take(40).collect::<String>()) } else { t }
}

/// `xs[i]` on a list or a string: an Int, negative from the end (`xs[-1]`
/// is the last), else kind `index` (out of range, a BigInt) or `type` (not
/// an Int) — the same answer for a local, a List slot and a string.
pub(crate) fn list_position(idx: &Value, len: usize, what: &str) -> Result<usize, RuntimeError> {
    let Value::Int(si) = idx else {
        return Err(RuntimeError::TypeError(format!("{} index must be an Int, got {} {}", what, value_type_name(idx), short_value(idx))));
    };
    let Some(raw) = si.to_i64() else {
        return Err(RuntimeError::TypeError(format!("{} index {} out of bounds (length {})", what, si, len)));
    };
    let i = if raw < 0 { raw + len as i64 } else { raw };
    if i < 0 || i as usize >= len {
        return Err(RuntimeError::TypeError(format!("{} index {} out of bounds (length {})", what, raw, len)));
    }
    Ok(i as usize)
}

fn undo(op: UndoOp) {
    match op {
        UndoOp::Restore { backend, key, prev } | UndoOp::Counter { backend, key, prev } => match prev {
            Some(v) => backend.set(&key, v),
            None => { backend.delete(&key); }
        },
        UndoOp::Unappend { backend } => backend.unappend(),
        UndoOp::RestoreList { backend, prev } => backend.replace_list(prev),
        UndoOp::ListSet { backend, index, prev } => { backend.list_set(index, prev); }
        UndoOp::ListRestore { backend, token, prev } => backend.list_restore(token, prev),
        UndoOp::Push(_) | UndoOp::PeerSend(_) | UndoOp::Cluster(_) | UndoOp::Horde(_) => {}
    }
}

/// Int / Int: an exact quotient is an Int (BigInt-exact), else a Float.
pub(crate) fn int_div_value(a: &SomaInt, b: &SomaInt) -> Result<Value, RuntimeError> {
    if b.to_i64() == Some(0) {
        Err(RuntimeError::TypeError("division by zero".to_string()))
    } else if let (Some(ai), Some(bi)) = (a.to_i64(), b.to_i64()) {
        // checked_rem: i64::MIN % -1 overflows; that division is
        // exact, so it takes the Int path (which promotes to big)
        match ai.checked_rem(bi) {
            Some(0) | None => Ok(Value::Int(a.clone().div(b.clone()))),
            Some(_) if ai.unsigned_abs() <= (1u64 << 53) && bi.unsigned_abs() <= (1u64 << 53) => Ok(Value::Float(ai as f64 / bi as f64)),
            Some(_) => Ok(Value::Float(rational_to_f64(rug::Rational::from((a.to_rug(), b.to_rug()))))),
        }
    } else if a.clone().modulo(b.clone()).to_i64() == Some(0) {
        // exact big division stays an Int — same rule as small ints
        Ok(Value::Int(a.clone().div(b.clone())))
    } else {
        Ok(Value::Float(rational_to_f64(rug::Rational::from((a.to_rug(), b.to_rug())))))
    }
}

/// Round an exact ratio once, to nearest/even, including subnormals.
/// GMP's Rational::to_f64 truncates; converting numerator/denominator
/// separately loses precision and can produce inf/inf for a finite ratio.
pub(crate) fn rational_to_f64(value: rug::Rational) -> f64 {
    let negative = value < 0;
    let magnitude = value.abs();
    let lower = magnitude.to_f64();
    if !lower.is_finite() { return if negative { -lower } else { lower }; }
    let upper = f64::from_bits(lower.to_bits() + 1);
    let mut midpoint = rug::Rational::from_f64(lower).unwrap();
    if upper.is_infinite() {
        midpoint += rug::Integer::from(1) << 970u32;
    } else {
        midpoint += rug::Rational::from_f64(upper).unwrap();
        midpoint /= 2;
    }
    let rounded = match magnitude.cmp(&midpoint) {
        std::cmp::Ordering::Less => lower,
        std::cmp::Ordering::Greater => upper,
        std::cmp::Ordering::Equal => if lower.to_bits() & 1 == 0 { lower } else { upper },
    };
    if negative { -rounded } else { rounded }
}

/// The longest line the signal bus reads (a peer that sent 300 MB with no
/// newline grew the receiver by 300 MB): a longer one ends the connection.
pub const BUS_MAX_LINE: u64 = 16 * 1024 * 1024;

/// `lines()` with a length cap, for the bus sockets.
/// Values one JSON text from a client or a peer may hold: a 15 MB line of
/// `[0,0,0,…]` (7.5 M elements) became ~2.4 GB of parsed values before any
/// check ran — the byte cap alone does not bound the parsed size.
pub const JSON_MAX_VALUES: usize = 1_000_000;

/// True when `text` holds more than JSON_MAX_VALUES values (a count of the
/// `,` `[` `{` outside strings — cheap, before any parse)
pub fn json_too_many_values(text: &str) -> bool {
    let mut n = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for b in text.bytes() {
        if in_str {
            if esc { esc = false } else if b == b'\\' { esc = true } else if b == b'"' { in_str = false }
            continue;
        }
        match b {
            b'"' => in_str = true,
            // weighted by what a value costs once parsed: an empty `{}` is a
            // whole map (490 000 of them, 1.5 MB, took 180 MB)
            b',' => { n += 1; if n > JSON_MAX_VALUES { return true; } }
            b'[' => { n += 2; if n > JSON_MAX_VALUES { return true; } }
            b'{' => { n += 4; if n > JSON_MAX_VALUES { return true; } }
            _ => {}
        }
    }
    false
}

pub fn bus_lines<R: std::io::BufRead>(mut r: R) -> impl Iterator<Item = std::io::Result<String>> {
    std::iter::from_fn(move || {
        use std::io::{BufRead, Read};
        let mut buf = Vec::new();
        match (&mut r).take(BUS_MAX_LINE + 1).read_until(b'\n', &mut buf) {
            Ok(0) => None,
            Ok(_) if buf.len() as u64 > BUS_MAX_LINE => {
                eprintln!("bus: a line longer than {} bytes — connection closed", BUS_MAX_LINE);
                Some(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bus line too long")))
            }
            Ok(_) => {
                if buf.last() == Some(&b'\n') { buf.pop(); if buf.last() == Some(&b'\r') { buf.pop(); } }
                match String::from_utf8(buf) {
                    Ok(line) if json_too_many_values(&line) => {
                        eprintln!("bus: an event with more than {} JSON values — connection closed", JSON_MAX_VALUES);
                        Some(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bus event too large")))
                    }
                    Ok(line) => Some(Ok(line)),
                    Err(e) => Some(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e))),
                }
            }
            Err(e) => Some(Err(e)),
        }
    })
}

fn and_or_err(e: RuntimeError, op: &str) -> RuntimeError {
    match e {
        RuntimeError::TypeError(m) => RuntimeError::TypeError(format!("{} needs Bool operands: {}", op, m)),
        other => other,
    }
}

/// A thread that may run handlers: the 64 MB stack of the request threads,
/// so the 512-frame recursion guard fires before the OS stack does (a
/// websocket, tick or bus thread on the default stack aborted the whole
/// process at ~250 frames).
pub fn spawn_handler_thread<F>(f: F) -> std::thread::JoinHandle<()>
where F: FnOnce() + Send + 'static {
    std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(f).expect("spawn a handler thread")
}

/// A map key anywhere in `v` that the storage encoding reserves (`__…`).
pub(crate) fn reserved_storage_key(v: &Value) -> Option<String> {
    match v {
        Value::Map(m) => m.iter().find_map(|(k, x)| if k.starts_with("__") { Some(k.clone()) } else { reserved_storage_key(x) }),
        Value::List(xs) => xs.iter().find_map(reserved_storage_key),
        Value::Variant { fields: VariantValue::Struct(m), .. } => m.values().find_map(reserved_storage_key),
        Value::Variant { fields: VariantValue::Tuple(xs), .. } => xs.iter().find_map(reserved_storage_key),
        _ => None,
    }
}

/// Length of the `{…}` interpolation segment opening at byte `open`
/// (relative to `open + 1`): the first `}` — or, when the segment holds a
/// block (`{if c { 1 } else { 2 }}`, `{xs.map(x => { x })}`), the brace
/// that closes it at depth 0. The first-`}` rule printed a block as
/// garbage text with no check error.
pub(crate) fn interp_segment_end(s: &str, open: usize) -> Option<usize> {
    let rest = &s[open + 1..];
    let first = rest.find('}')?;
    if !rest[..first].contains('{') || rest.trim_start().starts_with('{') {
        return Some(first);
    }
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                if depth == 0 { return Some(i); }
                depth -= 1;
            }
            _ => {}
        }
    }
    Some(first)
}


/// In a rule between slots, `other.size` (or `len(other)`) is that slot's
/// entry count — bound as `__count__other` (a bare `other` stays the value
/// at the written key).
fn count_of_other_slots(e: &Expr, others: &[String]) -> Expr {
    let is_count = |f: &str| matches!(f, "size" | "len" | "count" | "length");
    let named = |x: &Expr| match x { Expr::Ident(n) if others.iter().any(|o| o == n) => Some(n.clone()), _ => None };
    let sub = |x: &Spanned<Expr>| Box::new(Spanned::new(count_of_other_slots(&x.node, others), x.span));
    match e {
        Expr::FieldAccess { target, field } if is_count(field) => match named(&target.node) {
            Some(o) => Expr::Ident(format!("__count__{}", o)),
            None => Expr::FieldAccess { target: sub(target), field: field.clone() },
        },
        Expr::MethodCall { target, method, args } if args.is_empty() && is_count(method) => match named(&target.node) {
            Some(o) => Expr::Ident(format!("__count__{}", o)),
            None => Expr::MethodCall { target: sub(target), method: method.clone(), args: args.clone() },
        },
        Expr::FnCall { name, args } if is_count(name) && args.len() == 1 => match named(&args[0].node) {
            Some(o) => Expr::Ident(format!("__count__{}", o)),
            None => Expr::FnCall { name: name.clone(), args: args.iter().map(|a| Spanned::new(count_of_other_slots(&a.node, others), a.span)).collect() },
        },
        Expr::BinaryOp { left, op, right } => Expr::BinaryOp { left: sub(left), op: *op, right: sub(right) },
        Expr::CmpOp { left, op, right } => Expr::CmpOp { left: sub(left), op: *op, right: sub(right) },
        Expr::Not(i) => Expr::Not(sub(i)),
        Expr::FnCall { name, args } => Expr::FnCall { name: name.clone(), args: args.iter().map(|a| Spanned::new(count_of_other_slots(&a.node, others), a.span)).collect() },
        other => other.clone(),
    }
}


/// The invariant as the author wrote it: `__count__stock` reads `stock.size`
/// and the bare `size` is the written slot's.
fn show_invariant(inv: &Expr, slot: &str) -> String {
    let text = crate::ast::render_expr(inv);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find("__count__") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "__count__".len()..];
        let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
        out.push_str(&format!("{}.size", &after[..end]));
        rest = &after[end..];
    }
    out.push_str(rest);
    // the bare `size` binding is the written slot's own count
    out.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
        .filter(|w| *w == "size")
        .count();
    out.replace(" size ", &format!(" {}.size ", slot)).replace("(size ", &format!("({}.size ", slot))
}

#[cfg(test)]
mod transaction_tests;

#[cfg(test)]
mod backend_tests;
