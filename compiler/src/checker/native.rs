//! Validates that [native] handlers only use the allowed numeric subset.
//!
//! Allowed: Int, Float, Bool params/returns. Arithmetic, comparison, logic ops.
//! if/else, while, for, break, continue. let, assignment. Math builtins.
//! Calls to other [native] handlers in the same cell.
//!
//! Forbidden: String, Map, pipes, storage access, print, HTTP, signals, lambdas.

use crate::ast::*;

/// Error from native handler validation
#[derive(Debug)]
pub struct NativeCheckError {
    pub handler_name: String,
    pub reason: String,
}

impl std::fmt::Display for NativeCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error: handler '{}' is marked [native] but {}\n  → [native] handlers can only use Int, Float, Bool and numeric operations",
            self.handler_name, self.reason
        )
    }
}

/// Set of handler names that are also [native] in the same cell.
/// Used to allow calls between native handlers.
pub type NativeSiblings = std::collections::HashSet<String>;

/// Validate a [native] handler's parameter types.
fn check_params(handler_name: &str, params: &[Param]) -> Result<(), NativeCheckError> {
    for p in params {
        if !is_native_type(&p.ty.node) {
            return Err(NativeCheckError {
                handler_name: handler_name.to_string(),
                reason: format!("uses unsupported parameter type '{:?}' for '{}'", p.ty.node, p.name),
            });
        }
    }
    Ok(())
}

fn is_native_type(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Simple(name) => matches!(name.as_str(), "Int" | "Float" | "Bool" | "String"),
        TypeExpr::Generic { name, args } => {
            name == "List" && args.len() == 1 && matches!(&args[0].node, TypeExpr::Simple(s) if s == "Int" || s == "Float")
        }
        _ => false,
    }
}

const ALLOWED_BUILTINS: &[&str] = &[
    "sqrt", "log", "exp", "pow", "abs", "min", "max", "random",
    "len", "nth", "range", "floor", "ceil", "round", "sin", "cos",
    // Pipe operations (generate parallel native code)
    "map", "filter", "reduce", "fold",
    // Type conversions
    "to_float", "to_int", "to_string",
    // Bit operations (Int)
    "band", "bor", "bxor", "bnot", "shl", "shr",
    // Number theory
    "pow_mod", "gcd", "sqrt_int",
    // String introspection
    "str_len", "str_at", "str_eq",
];

/// Validate all statements in a [native] handler body.
pub fn check_native_handler(
    handler_name: &str,
    params: &[Param],
    body: &[Spanned<Statement>],
    siblings: &NativeSiblings,
) -> Result<(), NativeCheckError> {
    check_params(handler_name, params)?;
    for stmt in body {
        check_stmt(handler_name, &stmt.node, siblings)?;
    }
    Ok(())
}

fn check_stmt(handler_name: &str, stmt: &Statement, siblings: &NativeSiblings) -> Result<(), NativeCheckError> {
    match stmt {
        Statement::Let { value, .. } => check_expr(handler_name, &value.node, siblings),
        Statement::Assign { value, .. } => check_expr(handler_name, &value.node, siblings),
        Statement::Return { value } => check_expr(handler_name, &value.node, siblings),
        Statement::If { condition, then_body, else_body } => {
            check_expr(handler_name, &condition.node, siblings)?;
            for s in then_body { check_stmt(handler_name, &s.node, siblings)?; }
            for s in else_body { check_stmt(handler_name, &s.node, siblings)?; }
            Ok(())
        }
        Statement::While { condition, body } => {
            check_expr(handler_name, &condition.node, siblings)?;
            for s in body { check_stmt(handler_name, &s.node, siblings)?; }
            Ok(())
        }
        Statement::For { iter, body, .. } => {
            check_expr(handler_name, &iter.node, siblings)?;
            for s in body { check_stmt(handler_name, &s.node, siblings)?; }
            Ok(())
        }
        Statement::Break | Statement::Continue => Ok(()),
        Statement::ExprStmt { expr } => check_expr(handler_name, &expr.node, siblings),
        Statement::Emit { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses 'emit' (signals not allowed in native handlers)".to_string(),
        }),
        Statement::Require { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses 'require' (not allowed in native handlers)".to_string(),
        }),
        Statement::MethodCall { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses method call (not allowed in native handlers)".to_string(),
        }),
        Statement::Ensure { condition } => check_expr(handler_name, &condition.node, siblings),
    }
}

fn check_expr(handler_name: &str, expr: &Expr, siblings: &NativeSiblings) -> Result<(), NativeCheckError> {
    match expr {
        Expr::Literal(lit) => {
            match lit {
                Literal::Int(_) | Literal::Float(_) | Literal::Bool(_) => Ok(()),
                Literal::String(_) => Ok(()),
                _ => Err(NativeCheckError {
                    handler_name: handler_name.to_string(),
                    reason: format!("uses unsupported literal type {:?}", lit),
                }),
            }
        }
        Expr::Ident(_) => Ok(()),
        Expr::BinaryOp { left, right, .. } => {
            check_expr(handler_name, &left.node, siblings)?;
            check_expr(handler_name, &right.node, siblings)
        }
        Expr::CmpOp { left, right, .. } => {
            check_expr(handler_name, &left.node, siblings)?;
            check_expr(handler_name, &right.node, siblings)
        }
        Expr::Not(inner) => check_expr(handler_name, &inner.node, siblings),
        Expr::FnCall { name, args } => {
            // Allow known math builtins and calls to other native handlers
            if !ALLOWED_BUILTINS.contains(&name.as_str())
                && !siblings.contains(name)
                && name != handler_name
            {
                return Err(NativeCheckError {
                    handler_name: handler_name.to_string(),
                    reason: format!("calls non-native function '{}'", name),
                });
            }
            for arg in args { check_expr(handler_name, &arg.node, siblings)?; }
            Ok(())
        }
        Expr::Pipe { left, right } => {
            // Pipes are allowed in native — they generate parallel code
            check_expr(handler_name, &left.node, siblings)?;
            check_expr(handler_name, &right.node, siblings)
        }
        Expr::Lambda { param: _, body } => {
            // Lambdas allowed for pipe operations (map, filter, reduce)
            check_expr(handler_name, &body.node, siblings)
        }
        Expr::LambdaBlock { param: _, stmts, result } => {
            for stmt in stmts { check_stmt(handler_name, &stmt.node, siblings)?; }
            check_expr(handler_name, &result.node, siblings)
        }
        Expr::FieldAccess { target, .. } => {
            // Field access allowed for lambda params (p.acc, p.val, s.price)
            check_expr(handler_name, &target.node, siblings)
        }
        Expr::MethodCall { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses method call (not allowed in native handlers)".to_string(),
        }),
        Expr::Record { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses record literal (not allowed in native handlers)".to_string(),
        }),
        Expr::Try(_) => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses try expression (not allowed in native handlers)".to_string(),
        }),
        Expr::Match { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses match expression (not allowed in native handlers)".to_string(),
        }),
        Expr::ListLiteral(elements) => {
            for elem in elements { check_expr(handler_name, &elem.node, siblings)?; }
            Ok(())
        }
        Expr::TryPropagate(inner) => check_expr(handler_name, &inner.node, siblings),
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            check_expr(handler_name, &condition.node, siblings)?;
            for stmt in then_body { check_stmt(handler_name, &stmt.node, siblings)?; }
            check_expr(handler_name, &then_result.node, siblings)?;
            for stmt in else_body { check_stmt(handler_name, &stmt.node, siblings)?; }
            check_expr(handler_name, &else_result.node, siblings)?;
            Ok(())
        }
    }
}
