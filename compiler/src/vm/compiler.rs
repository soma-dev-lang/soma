use crate::ast::*;
use super::bytecode::*;

/// Compiles AST signal handlers into bytecode chunks.
pub struct BytecodeCompiler {
    /// All compiled chunks, indexed by (cell_name, signal_name)
    pub chunks: Vec<Chunk>,
}

impl BytecodeCompiler {
    pub fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Compile all handlers in a program
    pub fn compile_program(&mut self, program: &Program) {
        for cell in &program.cells {
            if cell.node.kind != CellKind::Cell {
                continue;
            }
            self.compile_cell(&cell.node);
        }
    }

    fn compile_cell(&mut self, cell: &CellDef) {
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
                    BinOp::Add => chunk.emit(Op::Add),
                    BinOp::Sub => chunk.emit(Op::Sub),
                    BinOp::Mul => chunk.emit(Op::Mul),
                    BinOp::Div => chunk.emit(Op::Div),
                    BinOp::Mod => chunk.emit(Op::Mod),
                    BinOp::And => chunk.emit(Op::Mul), // truthy multiply
                    BinOp::Or => chunk.emit(Op::Add),  // truthy add
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
                // Check if it's a recursive call (same signal name)
                if chunk.signal_name == *name {
                    let cell_idx = chunk.add_constant(Constant::Name(chunk.cell_name.clone()));
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
        }
    }

    fn compile_literal(&self, chunk: &mut Chunk, lit: &Literal) {
        match lit {
            Literal::Int(n) => {
                let idx = chunk.add_constant(Constant::Int(*n));
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
            Constraint::Predicate { name, .. } => {
                chunk.emit(Op::True); // Unknown predicates pass
            }
            Constraint::Descriptive(_) => {
                chunk.emit(Op::True);
            }
        }
    }
}
