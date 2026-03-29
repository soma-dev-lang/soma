/// Soma VM bytecode instruction set.
/// Stack-based: operations pop from and push to a value stack.

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum Op {
    // ── Constants & Variables ─────────────────────────────────────
    /// Push a constant from the constants pool onto the stack
    Const,      // [idx:u16] → push constants[idx]
    /// Push Unit (null) onto the stack
    Unit,
    /// Push true
    True,
    /// Push false
    False,

    // ── Local variables ──────────────────────────────────────────
    /// Load a local variable onto the stack
    GetLocal,   // [slot:u16] → push locals[slot]
    /// Store top of stack into a local variable
    SetLocal,   // [slot:u16] → locals[slot] = pop()

    // ── Arithmetic ───────────────────────────────────────────────
    Add,        // pop b, pop a, push a+b
    Sub,        // pop b, pop a, push a-b
    Mul,        // pop b, pop a, push a*b
    Div,        // pop b, pop a, push a/b
    Mod,        // pop b, pop a, push a%b
    Neg,        // pop a, push -a

    // ── Comparison ───────────────────────────────────────────────
    Eq,         // pop b, pop a, push a==b
    Ne,         // pop b, pop a, push a!=b
    Lt,         // pop b, pop a, push a<b
    Gt,         // pop b, pop a, push a>b
    Le,         // pop b, pop a, push a<=b
    Ge,         // pop b, pop a, push a>=b

    // ── Logic ────────────────────────────────────────────────────
    Not,        // pop a, push !a

    // ── Strings ──────────────────────────────────────────────────
    Concat,     // pop b, pop a, push concat(a, b)

    // ── Control flow ─────────────────────────────────────────────
    /// Jump unconditionally
    Jump,       // [offset:u16] → ip = offset
    /// Jump if top of stack is falsy (pop)
    JumpIfFalse,// [offset:u16] → if !pop() { ip = offset }
    /// Return top of stack from current frame
    Return,

    // ── Function calls ───────────────────────────────────────────
    /// Call a built-in function
    CallBuiltin,// [name_idx:u16, argc:u8] → pop argc args, call builtin, push result
    /// Call a signal handler (recursive or cross-cell)
    CallSignal, // [cell_idx:u16, sig_idx:u16, argc:u8]
    /// Call a storage method
    CallStorage,// [slot_idx:u16, method_idx:u16, argc:u8]

    // ── Object access ────────────────────────────────────────────
    /// Get field from map/object on top of stack
    GetField,   // [name_idx:u16] → pop obj, push obj.field
    /// Get method result
    CallMethod, // [name_idx:u16, argc:u8] → pop args, pop obj, push obj.method(args)

    // ── Iteration ────────────────────────────────────────────────
    /// Set up iteration: pop iterable, push iterator state
    IterInit,   // → pop iterable, push iter_state
    /// Advance iterator: push next value or jump to end
    IterNext,   // [end_offset:u16, local:u16] → locals[local] = next, or jump
    /// Pop value (cleanup)
    Pop,
}

/// A compiled chunk of bytecode — one per signal handler
#[derive(Debug, Clone)]
pub struct Chunk {
    /// The bytecode instructions
    pub code: Vec<u8>,
    /// Constants pool (strings, ints, floats)
    pub constants: Vec<Constant>,
    /// Local variable names (for debugging)
    pub locals: Vec<String>,
    /// Source cell and signal name
    pub cell_name: String,
    pub signal_name: String,
}

impl Chunk {
    pub fn new(cell_name: String, signal_name: String) -> Self {
        Self {
            code: Vec::with_capacity(256),
            constants: Vec::new(),
            locals: Vec::new(),
            cell_name,
            signal_name,
        }
    }

    /// Emit a single-byte instruction
    pub fn emit(&mut self, op: Op) -> usize {
        let offset = self.code.len();
        self.code.push(op as u8);
        offset
    }

    /// Emit an instruction with a u16 operand
    pub fn emit_u16(&mut self, op: Op, operand: u16) -> usize {
        let offset = self.code.len();
        self.code.push(op as u8);
        self.code.push((operand >> 8) as u8);
        self.code.push((operand & 0xff) as u8);
        offset
    }

    /// Emit an instruction with u16 + u8 operands
    pub fn emit_u16_u8(&mut self, op: Op, a: u16, b: u8) -> usize {
        let offset = self.code.len();
        self.code.push(op as u8);
        self.code.push((a >> 8) as u8);
        self.code.push((a & 0xff) as u8);
        self.code.push(b);
        offset
    }

    /// Emit CallSignal: u16 cell, u16 signal, u8 argc
    pub fn emit_call_signal(&mut self, cell: u16, signal: u16, argc: u8) -> usize {
        let offset = self.code.len();
        self.code.push(Op::CallSignal as u8);
        self.code.push((cell >> 8) as u8);
        self.code.push((cell & 0xff) as u8);
        self.code.push((signal >> 8) as u8);
        self.code.push((signal & 0xff) as u8);
        self.code.push(argc);
        offset
    }

    /// Emit IterNext: u16 end_offset, u16 local
    pub fn emit_iter_next(&mut self, end_offset: u16, local: u16) -> usize {
        let offset = self.code.len();
        self.code.push(Op::IterNext as u8);
        self.code.push((end_offset >> 8) as u8);
        self.code.push((end_offset & 0xff) as u8);
        self.code.push((local >> 8) as u8);
        self.code.push((local & 0xff) as u8);
        offset
    }

    /// Patch a jump target
    pub fn patch_jump(&mut self, offset: usize, target: u16) {
        self.code[offset + 1] = (target >> 8) as u8;
        self.code[offset + 2] = (target & 0xff) as u8;
    }

    /// Patch IterNext end offset
    pub fn patch_iter_next(&mut self, offset: usize, end: u16) {
        self.code[offset + 1] = (end >> 8) as u8;
        self.code[offset + 2] = (end & 0xff) as u8;
    }

    /// Add a constant and return its index
    pub fn add_constant(&mut self, constant: Constant) -> u16 {
        // Check for duplicate
        for (i, c) in self.constants.iter().enumerate() {
            if *c == constant {
                return i as u16;
            }
        }
        let idx = self.constants.len();
        self.constants.push(constant);
        idx as u16
    }

    /// Add or find a local variable, return its slot index
    pub fn add_local(&mut self, name: &str) -> u16 {
        if let Some(idx) = self.locals.iter().position(|n| n == name) {
            return idx as u16;
        }
        let idx = self.locals.len();
        self.locals.push(name.to_string());
        idx as u16
    }

    /// Find a local variable's slot
    pub fn find_local(&self, name: &str) -> Option<u16> {
        self.locals.iter().position(|n| n == name).map(|i| i as u16)
    }

    /// Read a u16 at the given offset
    pub fn read_u16(&self, offset: usize) -> u16 {
        ((self.code[offset] as u16) << 8) | (self.code[offset + 1] as u16)
    }

    /// Current code length (for jump targets)
    pub fn len(&self) -> usize {
        self.code.len()
    }
}

/// Constant pool entries
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    String(String),
    /// Name reference (for builtins, fields, methods, storage slots)
    Name(String),
    /// Lambda AST stored for interpreter fallback
    LambdaAst {
        param: String,
        body_expr: Option<Box<crate::ast::Spanned<crate::ast::Expr>>>,
        body_stmts: Option<Vec<crate::ast::Spanned<crate::ast::Statement>>>,
        result_expr: Option<Box<crate::ast::Spanned<crate::ast::Expr>>>,
    },
}

impl PartialEq for Constant {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Constant::Int(a), Constant::Int(b)) => a == b,
            (Constant::Float(a), Constant::Float(b)) => a == b,
            (Constant::String(a), Constant::String(b)) => a == b,
            (Constant::Name(a), Constant::Name(b)) => a == b,
            // Lambdas are never deduplicated
            (Constant::LambdaAst { .. }, _) => false,
            (_, Constant::LambdaAst { .. }) => false,
            _ => false,
        }
    }
}

impl Constant {
    pub fn as_str(&self) -> &str {
        match self {
            Constant::String(s) | Constant::Name(s) => s,
            _ => "",
        }
    }
}
