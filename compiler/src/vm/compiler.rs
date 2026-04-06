use std::collections::HashSet;
use crate::ast::*;
use super::bytecode::*;

/// Compiles AST signal handlers into bytecode chunks.
pub struct BytecodeCompiler {
    /// All compiled chunks, indexed by (cell_name, signal_name)
    pub chunks: Vec<Chunk>,
    /// Handler names known in the current cell (for cross-handler calls)
    current_cell_handlers: HashSet<String>,
    /// Current cell name being compiled
    current_cell_name: String,
}

impl BytecodeCompiler {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            current_cell_handlers: HashSet::new(),
            current_cell_name: String::new(),
        }
    }

    /// Compile all handlers in a program
    pub fn compile_program(&mut self, program: &Program) {
        for cell in &program.cells {
            if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
                continue;
            }
            self.compile_cell(&cell.node);
        }
    }

    fn compile_cell(&mut self, cell: &CellDef) {
        // Collect all handler names in this cell first (for cross-handler calls)
        self.current_cell_name = cell.name.clone();
        self.current_cell_handlers.clear();
        for section in &cell.sections {
            if let Section::OnSignal(ref handler) = section.node {
                self.current_cell_handlers.insert(handler.signal_name.clone());
            }
        }

        for section in &cell.sections {
            if let Section::OnSignal(ref handler) = section.node {
                let chunk = self.compile_handler(cell, handler);
                self.chunks.push(chunk);
            }
            // Recurse into interior cells
            if let Section::Interior(ref interior) = section.node {
                for child in &interior.cells {
                    self.compile_cell(&child.node);
                }
            }
        }
    }

    fn compile_handler(&self, cell: &CellDef, handler: &OnSection) -> Chunk {
        let mut chunk = Chunk::new(cell.name.clone(), handler.signal_name.clone());

        // Register parameters as locals
        for param in &handler.params {
            chunk.add_local(&param.name);
        }

        // Compile body
        for stmt in &handler.body {
            self.compile_stmt(&mut chunk, &stmt.node);
        }

        // Implicit return Unit if no explicit return
        chunk.emit(Op::Unit);
        chunk.emit(Op::Return);

        chunk
    }

    fn compile_stmt(&self, chunk: &mut Chunk, stmt: &Statement) {
        match stmt {
            Statement::Let { name, value } => {
                self.compile_expr(chunk, &value.node);
                let slot = chunk.add_local(name);
                chunk.emit_u16(Op::SetLocal, slot);
            }

            Statement::Assign { name, value } => {
                self.compile_expr(chunk, &value.node);
                let slot = chunk.find_local(name).unwrap_or_else(|| chunk.add_local(name));
                chunk.emit_u16(Op::SetLocal, slot);
            }

            Statement::Return { value } => {
                self.compile_expr(chunk, &value.node);
                chunk.emit(Op::Return);
            }

            Statement::If { condition, then_body, else_body } => {
                self.compile_expr(chunk, &condition.node);
                let jump_to_else = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);

                for s in then_body {
                    self.compile_stmt(chunk, &s.node);
                }

                if else_body.is_empty() {
                    chunk.patch_jump(jump_to_else, chunk.len() as u16);
                } else {
                    let jump_over_else = chunk.emit_u16(Op::Jump, 0xFFFF);
                    chunk.patch_jump(jump_to_else, chunk.len() as u16);
                    for s in else_body {
                        self.compile_stmt(chunk, &s.node);
                    }
                    chunk.patch_jump(jump_over_else, chunk.len() as u16);
                }
            }

            Statement::While { condition, body } => {
                let loop_start = chunk.len() as u16;
                self.compile_expr(chunk, &condition.node);
                let exit_jump = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);

                for s in body {
                    self.compile_stmt(chunk, &s.node);
                }

                chunk.emit_u16(Op::Jump, loop_start);
                chunk.patch_jump(exit_jump, chunk.len() as u16);
            }

            Statement::For { var, iter, body } => {
                // Compile iterator expression
                self.compile_expr(chunk, &iter.node);
                chunk.emit(Op::IterInit);

                let loop_start = chunk.len();
                let var_slot = chunk.add_local(var);
                let iter_next = chunk.emit_iter_next(0xFFFF, var_slot);

                for s in body {
                    self.compile_stmt(chunk, &s.node);
                }

                chunk.emit_u16(Op::Jump, loop_start as u16);
                let end = chunk.len() as u16;
                chunk.patch_iter_next(iter_next, end);
            }

            Statement::Emit { signal_name, args } => {
                // Push args, then emit as builtin call to "_emit"
                for arg in args {
                    self.compile_expr(chunk, &arg.node);
                }
                let name_idx = chunk.add_constant(Constant::Name(signal_name.clone()));
                chunk.emit_u16_u8(Op::CallBuiltin, name_idx, args.len() as u8);
                chunk.emit(Op::Pop);
            }

            Statement::Require { constraint, else_signal } => {
                // Compile as: if !constraint { call_builtin("_require_fail", else_signal) }
                self.compile_constraint(chunk, &constraint.node);
                let skip = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);
                // If true, continue
                let jump_over = chunk.emit_u16(Op::Jump, 0xFFFF);
                chunk.patch_jump(skip, chunk.len() as u16);
                // If false, error
                let sig_idx = chunk.add_constant(Constant::String(else_signal.clone()));
                chunk.emit_u16(Op::Const, sig_idx);
                let fail_idx = chunk.add_constant(Constant::Name("_require_fail".to_string()));
                chunk.emit_u16_u8(Op::CallBuiltin, fail_idx, 1);
                chunk.emit(Op::Return);
                chunk.patch_jump(jump_over, chunk.len() as u16);
            }

            Statement::MethodCall { target, method, args } => {
                // Push args
                for arg in args {
                    self.compile_expr(chunk, &arg.node);
                }
                let slot_idx = chunk.add_constant(Constant::Name(target.clone()));
                let method_idx = chunk.add_constant(Constant::Name(method.clone()));
                chunk.emit_u16_u8(Op::CallStorage, slot_idx, args.len() as u8);
                // Patch: we need method_idx too — encode it after
                chunk.code.push((method_idx >> 8) as u8);
                chunk.code.push((method_idx & 0xff) as u8);
                chunk.emit(Op::Pop);
            }

            Statement::ExprStmt { expr } => {
                self.compile_expr(chunk, &expr.node);
                chunk.emit(Op::Pop);
            }

            Statement::Ensure { .. } => {
                // TODO: implement ensure in bytecode VM
            }

            Statement::Break | Statement::Continue => {
                // Break/continue not yet supported in bytecode VM — emit
                // a runtime error string and return so the handler stops
                // instead of silently ignoring the statement.
                let msg = if matches!(stmt, Statement::Break) {
                    "break is not supported in the bytecode VM"
                } else {
                    "continue is not supported in the bytecode VM"
                };
                let idx = chunk.add_constant(Constant::String(msg.to_string()));
                chunk.emit_u16(Op::Const, idx);
                chunk.emit(Op::Return);
            }
        }
    }

    fn compile_expr(&self, chunk: &mut Chunk, expr: &Expr) {
        match expr {
            Expr::Literal(lit) => self.compile_literal(chunk, lit),

            Expr::Ident(name) => {
                if let Some(slot) = chunk.find_local(name) {
                    chunk.emit_u16(Op::GetLocal, slot);
                } else {
                    // Try as a constant name (shouldn't normally happen)
                    let idx = chunk.add_constant(Constant::String(name.clone()));
                    chunk.emit_u16(Op::Const, idx);
                }
            }

            Expr::BinaryOp { left, op, right } => {
                self.compile_expr(chunk, &left.node);
                self.compile_expr(chunk, &right.node);
                match op {
                    BinOp::Add => { chunk.emit(Op::Add); },
                    BinOp::Sub => { chunk.emit(Op::Sub); },
                    BinOp::Mul => { chunk.emit(Op::Mul); },
                    BinOp::Div => { chunk.emit(Op::Div); },
                    BinOp::Mod => { chunk.emit(Op::Mod); },
                    BinOp::And => {
                        // Stack: [a, b]. b is on top.
                        // Convert b to bool and check it
                        chunk.emit(Op::Not);
                        chunk.emit(Op::Not);
                        // Stack: [a, bool_b]
                        let jump_b_false = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);
                        // b was truthy — result depends on a
                        // Stack: [a]
                        chunk.emit(Op::Not);
                        chunk.emit(Op::Not);
                        // Stack: [bool_a] — this is the AND result
                        let jump_end = chunk.emit_u16(Op::Jump, 0xFFFF);
                        // b was falsy — result is false, but a is still on stack
                        chunk.patch_jump(jump_b_false, chunk.len() as u16);
                        chunk.emit(Op::Pop);
                        chunk.emit(Op::False);
                        chunk.patch_jump(jump_end, chunk.len() as u16);
                    }
                    BinOp::Or => {
                        // Stack: [a, b]. b is on top.
                        // Convert b to bool and check it
                        chunk.emit(Op::Not);
                        chunk.emit(Op::Not);
                        // Stack: [a, bool_b]
                        let jump_b_false = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);
                        // b was truthy — result is true, discard a
                        chunk.emit(Op::Pop);
                        chunk.emit(Op::True);
                        let jump_end = chunk.emit_u16(Op::Jump, 0xFFFF);
                        // b was falsy — result depends on a
                        chunk.patch_jump(jump_b_false, chunk.len() as u16);
                        // Stack: [a]
                        chunk.emit(Op::Not);
                        chunk.emit(Op::Not);
                        // Stack: [bool_a] — this is the OR result
                        chunk.patch_jump(jump_end, chunk.len() as u16);
                    }
                };
            }

            Expr::CmpOp { left, op, right } => {
                self.compile_expr(chunk, &left.node);
                self.compile_expr(chunk, &right.node);
                match op {
                    CmpOp::Eq => chunk.emit(Op::Eq),
                    CmpOp::Ne => chunk.emit(Op::Ne),
                    CmpOp::Lt => chunk.emit(Op::Lt),
                    CmpOp::Gt => chunk.emit(Op::Gt),
                    CmpOp::Le => chunk.emit(Op::Le),
                    CmpOp::Ge => chunk.emit(Op::Ge),
                };
            }

            Expr::Not(inner) => {
                self.compile_expr(chunk, &inner.node);
                chunk.emit(Op::Not);
            }

            Expr::FnCall { name, args } => {
                for arg in args {
                    self.compile_expr(chunk, &arg.node);
                }
                // Check if it's a call to any handler in the current cell
                if self.current_cell_handlers.contains(name.as_str()) {
                    let cell_idx = chunk.add_constant(Constant::Name(self.current_cell_name.clone()));
                    let sig_idx = chunk.add_constant(Constant::Name(name.clone()));
                    chunk.emit_call_signal(cell_idx, sig_idx, args.len() as u8);
                } else {
                    let name_idx = chunk.add_constant(Constant::Name(name.clone()));
                    chunk.emit_u16_u8(Op::CallBuiltin, name_idx, args.len() as u8);
                }
            }

            Expr::FieldAccess { target, field } => {
                self.compile_expr(chunk, &target.node);
                let field_idx = chunk.add_constant(Constant::Name(field.clone()));
                chunk.emit_u16(Op::GetField, field_idx);
            }

            Expr::MethodCall { target, method, args } => {
                self.compile_expr(chunk, &target.node);
                for arg in args {
                    self.compile_expr(chunk, &arg.node);
                }
                let method_idx = chunk.add_constant(Constant::Name(method.clone()));
                chunk.emit_u16_u8(Op::CallMethod, method_idx, args.len() as u8);
            }

            Expr::Pipe { left, right } => {
                self.compile_expr(chunk, &left.node);
                match &right.node {
                    Expr::FnCall { name, args } => {
                        for arg in args {
                            self.compile_expr(chunk, &arg.node);
                        }
                        let name_idx = chunk.add_constant(Constant::Name(name.clone()));
                        chunk.emit_u16_u8(Op::CallBuiltin, name_idx, (args.len() + 1) as u8);
                    }
                    Expr::Ident(name) => {
                        let name_idx = chunk.add_constant(Constant::Name(name.clone()));
                        chunk.emit_u16_u8(Op::CallBuiltin, name_idx, 1);
                    }
                    _ => {
                        self.compile_expr(chunk, &right.node);
                    }
                }
            }

            Expr::Record { type_name, fields } => {
                // Compile as map("_type", name, field1, val1, ...)
                let type_idx = chunk.add_constant(Constant::String("_type".to_string()));
                chunk.emit_u16(Op::Const, type_idx);
                let name_idx = chunk.add_constant(Constant::String(type_name.clone()));
                chunk.emit_u16(Op::Const, name_idx);
                for (fname, fexpr) in fields {
                    let fi = chunk.add_constant(Constant::String(fname.clone()));
                    chunk.emit_u16(Op::Const, fi);
                    self.compile_expr(chunk, &fexpr.node);
                }
                let map_idx = chunk.add_constant(Constant::Name("map".to_string()));
                chunk.emit_u16_u8(Op::CallBuiltin, map_idx, (2 + fields.len() * 2) as u8);
            }

            Expr::Try(inner) => {
                // Store the inner expression as AST and fall back to interpreter
                // at runtime, so that errors (like division by zero) are properly
                // caught and returned as {value: unit, error: "message"}.
                let idx = chunk.add_constant(Constant::TryAst(inner.clone()));
                chunk.emit_u16(Op::Const, idx);
            }

            Expr::Match { subject, arms } => {
                // Compile match expression:
                // For each arm, recompile the subject, compare with pattern,
                // and jump over non-matching arms.
                let mut jump_to_end: Vec<usize> = Vec::new();

                for arm in arms {
                    match &arm.pattern {
                        MatchPattern::Wildcard => {
                            // Wildcard always matches — compile body and result
                            for s in &arm.body {
                                self.compile_stmt(chunk, &s.node);
                            }
                            self.compile_expr(chunk, &arm.result.node);
                            // Jump to end (skip remaining arms)
                            let je = chunk.emit_u16(Op::Jump, 0xFFFF);
                            jump_to_end.push(je);
                        }
                        MatchPattern::Literal(lit) => {
                            // Recompile the subject for comparison
                            self.compile_expr(chunk, &subject.node);
                            // Push the literal pattern value
                            self.compile_literal(chunk, lit);
                            // Compare
                            chunk.emit(Op::Eq);
                            // If not equal, skip this arm
                            let skip = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);

                            // Arm matches — compile body statements and result
                            for s in &arm.body {
                                self.compile_stmt(chunk, &s.node);
                            }
                            self.compile_expr(chunk, &arm.result.node);
                            // Jump to end
                            let je = chunk.emit_u16(Op::Jump, 0xFFFF);
                            jump_to_end.push(je);

                            // Patch the skip target to here
                            chunk.patch_jump(skip, chunk.len() as u16);
                        }
                        MatchPattern::Variable(_) => {
                            // Variable always matches (like wildcard) — compile body
                            // TODO: bind variable in VM scope
                            for s in &arm.body {
                                self.compile_stmt(chunk, &s.node);
                            }
                            self.compile_expr(chunk, &arm.result.node);
                            let je = chunk.emit_u16(Op::Jump, 0xFFFF);
                            jump_to_end.push(je);
                        }
                        MatchPattern::MapDestructure(_) | MatchPattern::StringPrefix { .. } | MatchPattern::Range { .. } => {
                            // TODO: implement in bytecode VM — for now, treat as wildcard
                            for s in &arm.body {
                                self.compile_stmt(chunk, &s.node);
                            }
                            self.compile_expr(chunk, &arm.result.node);
                            let je = chunk.emit_u16(Op::Jump, 0xFFFF);
                            jump_to_end.push(je);
                        }
                        MatchPattern::Or(alternatives) => {
                            // Or-pattern: try each alternative
                            // For VM, just check literals; variables in or not supported
                            let mut skip_past: Vec<usize> = Vec::new();
                            // Check each alternative; if any matches, fall through to body
                            for (i, alt) in alternatives.iter().enumerate() {
                                if let MatchPattern::Literal(lit) = alt {
                                    self.compile_expr(chunk, &subject.node);
                                    self.compile_literal(chunk, lit);
                                    chunk.emit(Op::Eq);
                                    // If false, try next alternative
                                    let next = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);
                                    // If true, jump to body (skip remaining checks)
                                    let to_body = chunk.emit_u16(Op::Jump, 0xFFFF);
                                    chunk.patch_jump(next, chunk.len() as u16);
                                    skip_past.push(to_body);
                                    // If this is the last alternative and it didn't match, skip the arm
                                    if i == alternatives.len() - 1 {
                                        let skip = chunk.emit_u16(Op::Jump, 0xFFFF);
                                        // Patch all "to body" jumps
                                        for j in &skip_past {
                                            chunk.patch_jump(*j, chunk.len() as u16);
                                        }
                                        // Compile body
                                        for s in &arm.body {
                                            self.compile_stmt(chunk, &s.node);
                                        }
                                        self.compile_expr(chunk, &arm.result.node);
                                        let je = chunk.emit_u16(Op::Jump, 0xFFFF);
                                        jump_to_end.push(je);
                                        chunk.patch_jump(skip, chunk.len() as u16);
                                    }
                                }
                            }
                        }
                    }
                }

                // Default: push Unit if no arm matched
                chunk.emit(Op::Unit);

                // Patch all end jumps to here
                let end = chunk.len() as u16;
                for je in jump_to_end {
                    chunk.patch_jump(je, end);
                }
            }
            Expr::Lambda { param, body } => {
                let idx = chunk.add_constant(Constant::LambdaAst {
                    param: param.clone(),
                    body_expr: Some(body.clone()),
                    body_stmts: None,
                    result_expr: None,
                });
                chunk.emit_u16(Op::Const, idx);
            }
            Expr::LambdaBlock { param, stmts, result } => {
                let idx = chunk.add_constant(Constant::LambdaAst {
                    param: param.clone(),
                    body_expr: None,
                    body_stmts: Some(stmts.clone()),
                    result_expr: Some(result.clone()),
                });
                chunk.emit_u16(Op::Const, idx);
            }
            Expr::ListLiteral(elements) => {
                // Compile as a call to the builtin list() function
                for elem in elements {
                    self.compile_expr(chunk, &elem.node);
                }
                let name_idx = chunk.add_constant(Constant::Name("list".to_string()));
                chunk.emit_u16_u8(Op::CallBuiltin, name_idx, elements.len() as u8);
            }
            Expr::TryPropagate(_) => {
                // TODO: implement ? operator in bytecode VM
            }
            Expr::IfExpr { .. } => {
                // TODO: implement if-expression in bytecode VM
            }
        }
    }

    fn compile_literal(&self, chunk: &mut Chunk, lit: &Literal) {
        match lit {
            Literal::Int(n) => {
                let idx = chunk.add_constant(Constant::Int(*n));
                chunk.emit_u16(Op::Const, idx);
            }
            Literal::BigInt(s) => {
                // Store as string constant; the VM interpreter will parse it
                let idx = chunk.add_constant(Constant::String(s.clone()));
                chunk.emit_u16(Op::Const, idx);
            }
            Literal::Float(n) => {
                let idx = chunk.add_constant(Constant::Float(*n));
                chunk.emit_u16(Op::Const, idx);
            }
            Literal::String(s) => {
                let idx = chunk.add_constant(Constant::String(s.clone()));
                chunk.emit_u16(Op::Const, idx);
            }
            Literal::Bool(true) => { chunk.emit(Op::True); }
            Literal::Bool(false) => { chunk.emit(Op::False); }
            Literal::Unit => { chunk.emit(Op::Unit); }
            Literal::Duration(d) => {
                let idx = chunk.add_constant(Constant::Float(d.value));
                chunk.emit_u16(Op::Const, idx);
            }
            Literal::Percentage(p) => {
                let idx = chunk.add_constant(Constant::Float(*p));
                chunk.emit_u16(Op::Const, idx);
            }
        }
    }

    fn compile_constraint(&self, chunk: &mut Chunk, constraint: &Constraint) {
        match constraint {
            Constraint::Comparison { left, op, right } => {
                self.compile_expr(chunk, &left.node);
                self.compile_expr(chunk, &right.node);
                match op {
                    CmpOp::Eq => chunk.emit(Op::Eq),
                    CmpOp::Ne => chunk.emit(Op::Ne),
                    CmpOp::Lt => chunk.emit(Op::Lt),
                    CmpOp::Gt => chunk.emit(Op::Gt),
                    CmpOp::Le => chunk.emit(Op::Le),
                    CmpOp::Ge => chunk.emit(Op::Ge),
                };
            }
            Constraint::And(a, b) => {
                self.compile_constraint(chunk, &a.node);
                let skip = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);
                self.compile_constraint(chunk, &b.node);
                let end = chunk.emit_u16(Op::Jump, 0xFFFF);
                chunk.patch_jump(skip, chunk.len() as u16);
                chunk.emit(Op::False);
                chunk.patch_jump(end, chunk.len() as u16);
            }
            Constraint::Or(a, b) => {
                self.compile_constraint(chunk, &a.node);
                chunk.emit(Op::Not);
                let skip = chunk.emit_u16(Op::JumpIfFalse, 0xFFFF);
                self.compile_constraint(chunk, &b.node);
                let end = chunk.emit_u16(Op::Jump, 0xFFFF);
                chunk.patch_jump(skip, chunk.len() as u16);
                chunk.emit(Op::True);
                chunk.patch_jump(end, chunk.len() as u16);
            }
            Constraint::Not(inner) => {
                self.compile_constraint(chunk, &inner.node);
                chunk.emit(Op::Not);
            }
            Constraint::Predicate { name: _, .. } => {
                chunk.emit(Op::True); // Unknown predicates pass
            }
            Constraint::Descriptive(_) => {
                chunk.emit(Op::True);
            }
        }
    }
}
