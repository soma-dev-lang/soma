//! Static interpolation check (V1.7).
//!
//! Every string literal in a handler body is interpolated at runtime
//! (interpreter/mod.rs::interpolate_string). Since the V2.2.1 audit an
//! undefined variable inside `{...}` is a hard runtime error — so a
//! typo like "hello {customr}" passes `soma check` and 500s in
//! production. This pass scans every string literal with EXACTLY the
//! runtime's segmentation rules and reports identifiers that are
//! provably unknown, with a did-you-mean suggestion.
//!
//! Mirrored runtime rules (keep in sync with interpolate_string):
//!   - '{{' and '}}' are escapes for literal braces
//!   - a '{...}' segment that is empty or contains ':' or ';' is
//!     skipped (CSS/HTML), and scanning resumes one byte later
//!   - a segment that is a bare [A-Za-z0-9_]+ word is looked up in the
//!     local environment directly — undefined → runtime error
//!   - any other segment is parsed as an expression; if it does NOT
//!     parse it renders literally (NOT an error), if it does parse it
//!     is evaluated with locals in scope
//!
//! The pass is deliberately conservative: when scope is ambiguous or
//! the expression form is exotic, it stays silent. False negatives are
//! acceptable; false positives are not.

use crate::ast::*;
use std::collections::HashSet;

use super::names::{suggest, ProgramIndex};

#[derive(Debug)]
pub struct InterpolationIssue {
    pub message: String,
    pub span: Span,
    /// Inside `try { }` an interpolation error is catchable by design
    /// (assert_fails blesses error-raising handlers) — demoted to a
    /// warning so check does not contradict a passing test suite.
    pub warning: bool,
    /// A plain advisory (not a try-demoted error): reported as a habit warning.
    pub habit: bool,
    /// Stable machine-readable class for `check --json`.
    pub kind: &'static str,
}

/// `b.size ?? "M"`: on a Map, `.size` / `.len` / `.count` is the entry
/// count and `.keys` / `.values` the lists — never `()` — so the default
/// never applies and a JSON field named `size` is unreachable this way
/// (a clothing size read as 2, the number of fields)
fn count_field_defaults(body: &[Spanned<Statement>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    super::literals::for_each_stmt_deep(body, &mut |st| {
        super::termination::walk_stmt(st, &mut |e| {
            if let Expr::FnCall { name, args } = e {
                // `to_int(pow(3, 40))`: pow is a Float power — past 2^53 the
                // Int is off (12157665459056928768, not …801)
                if name == "to_int" && matches!(args.first().map(|a| &a.node), Some(Expr::FnCall { name: p, .. }) if p == "pow") {
                    let m = "`to_int(pow(…))`: pow() is a Float power, inexact past 2^53 (to_int(pow(3, 40)) is off by 33) — use `ipow(base, exp)` for an exact Int power".to_string();
                    if !out.contains(&m) { out.push(m); }
                    return;
                }
                if name != "_coalesce" || args.len() != 2 { return; }
                if let Expr::FieldAccess { target, field } = &args[0].node {
                    if matches!(field.as_str(), "size" | "len" | "count" | "keys" | "values") {
                        let t = render_expr(&target.node);
                        let m = format!("`{t}.{field} ?? …`: on a Map `.{field}` is the {} (never `()`), so the default never applies — to read a field named `{field}`, write `{t}.get(\"{field}\") ?? …`",
                            if matches!(field.as_str(), "keys" | "values") { format!("list of its {field}") } else { "number of entries".to_string() });
                        if !out.contains(&m) { out.push(m); }
                    }
                }
            }
        });
    });
    out
}

pub fn check_program(program: &Program) -> Vec<InterpolationIssue> {
    let index = ProgramIndex::build(program);
    let mut issues = Vec::new();
    let boundary = step_boundaries(program);

    // Handlers a test cell targets with `assert_fails handler(...)` are
    // EXPECTED to raise — interpolation issues in them demote to
    // warnings, otherwise check contradicts a passing test suite.
    let mut blessed_failing: HashSet<String> = HashSet::new();
    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Test) { continue; }
        for section in &cell.sections {
            if let Section::Rules(rules) = &section.node {
                for rule in &rules.rules {
                    if let Rule::AssertFails(expr) | Rule::AssertFailsMatching(expr, _) = &rule.node {
                        if let Expr::FnCall { name, .. } = &expr.node {
                            blessed_failing.insert(name.clone());
                        }
                    }
                }
            }
        }
    }

    for cell in super::names::collect_cells(program) {
        let cell_slots: HashSet<String> = cell.sections.iter().filter_map(|s| match &s.node {
            Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()),
            _ => None,
        }).flatten().collect();
        // test cells: the rules' expressions, with `let` bindings in scope
        // (an undefined `{var}` inside an assert used to be found at run time)
        if matches!(cell.kind, CellKind::Test) {
            let mut w = Walker::new(&index);
            w.in_test = true;
            // a test helper named like a program handler or a builtin replaces
            // it in this test's assertions: `on total(xs)` in the test made
            // `assert total(…)` test the helper, not Cart.total
            let program_cells: Vec<&CellDef> = super::names::collect_cells(program).into_iter()
                .filter(|c| matches!(c.kind, CellKind::Cell | CellKind::Agent)).collect();
            for section in &cell.sections {
                if let Section::OnSignal(on) = &section.node {
                    let n = &on.signal_name;
                    let owner = program_cells.iter().find(|c| c.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(o) if &o.signal_name == n)));
                    let msg = if let Some(c) = owner {
                        Some(format!("test helper `{n}` has the name of {}.{n} — the test's calls would run the helper instead of the code under test; rename it (`_{n}`)", c.name))
                    } else if super::names::builtin_names().contains(n.as_str()) {
                        Some(format!("test helper `{n}` has the name of the builtin `{n}` — it would replace it in this test; rename it (`_{n}`)"))
                    } else { None };
                    if let Some(message) = msg {
                        w.issues.push(InterpolationIssue { message, span: section.span, warning: false, habit: false, kind: "test_helper_shadows" });
                    }
                }
            }
            for section in &cell.sections {
                if let Section::Rules(rules) = &section.node {
                    for rule in &rules.rules {
                        match &rule.node {
                            Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) => w.walk_expr(e),
                            Rule::Let { name, value } => { w.walk_expr(value); w.scope.insert(name.clone()); }
                            Rule::Property { var, body, .. } => {
                                let var = var.clone();
                                w.scoped(&[var], |w| w.walk_expr(body));
                            }
                            Rule::MockHandler { name, reply, .. } => {
                                // `mock Email.send …` stubs Email's send only; a
                                // mock naming nothing (`mock chrage`, `Nope.charge`)
                                // never fires — the test then proves nothing
                                let bad = match name.split_once('.') {
                                    Some((c, h)) => if !index.cells.contains(c) { Some(format!("`mock {name}`: no cell `{c}` in this program")) }
                                        else if !index.arity.keys().any(|(cc, hh)| cc == c && hh == h) { Some(format!("`mock {name}`: cell `{c}` has no handler `{h}`")) }
                                        else { None },
                                    None => if !index.handler_map.contains_key(name) && !super::names::builtin_names().contains(name.as_str()) && !matches!(name.as_str(), "now" | "now_ms") {
                                        Some(format!("`mock {name}`: no handler or builtin named `{name}` — this mock would never fire"))
                                    } else { None },
                                };
                                if let Some(message) = bad {
                                    w.issues.push(InterpolationIssue { message, span: rule.span, warning: false, habit: false, kind: "mock_target" });
                                }
                                w.walk_expr(reply)
                            }
                            Rule::MockThink { reply, .. } | Rule::MockApprove { reply } => w.walk_expr(reply),
                            _ => {}
                        }
                    }
                }
            }
            issues.extend(w.issues);
            continue;
        }
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            match &section.node {
                Section::OnSignal(on) => {
                    let mut w = Walker::new(&index);
                    w.cell_slots = cell_slots.clone();
                    w.in_test = matches!(cell.kind, CellKind::Test);
                    if blessed_failing.contains(&on.signal_name) {
                        // interpolation issues in it are recoverable (a bare
                        // undefined name stays an error: that is a bug, not
                        // the failure the test expects)
                        w.blessed = true;
                    }
                    for p in &on.params {
                        // a parameter named like a slot of this cell: `len(m)`
                        // read the parameter while `m.set` wrote the slot (a
                        // false size proof), `rows.push(x)` was dropped
                        if cell_slots.contains(&p.name) {
                            w.issues.push(InterpolationIssue {
                                message: format!("parameter `{}` of `{}` has the name of the memory slot `{}` — reads and writes of `{}` in this handler would mix the two; rename the parameter (e.g. `new_{}`)", p.name, on.signal_name, p.name, p.name, p.name),
                                span: section.span,
                                warning: false,
                                habit: false,
                                kind: "param_shadows_slot",
                            });
                        }
                        w.scope.insert(p.name.clone());
                    }
                    w.walk_stmts(&on.body);
                    issues.extend(w.issues);
                    // `ensure` runs where it stands: a `return` before it skips
                    // the postcondition (the write committed, exit 0)
                    {
                        let mut returned = false;
                        for st in &on.body {
                            if let Statement::Ensure { .. } = &st.node {
                                if returned {
                                    issues.push(InterpolationIssue {
                                        message: format!("handler `{}` can `return` before this `ensure`, which then never runs — check the postcondition before each early return, or restructure so the handler falls through to it", on.signal_name),
                                        span: st.span, warning: true, habit: true, kind: "ensure_after_return",
                                    });
                                }
                                continue;
                            }
                            super::literals::for_each_stmt_deep(std::slice::from_ref(st), &mut |x| if matches!(x, Statement::Return { .. }) { returned = true; });
                        }
                    }
                    // a delete / an element overwrite on an `[immutable]` slot can
                    // only fail (it passed check and was refused at run time)
                    {
                        let immutable: Vec<String> = cell.sections.iter().filter_map(|sec| match &sec.node {
                            Section::Memory(m) => Some(m.slots.iter().filter(|sl| sl.node.properties.iter().any(|p| p.node.name() == "immutable")).map(|sl| sl.node.name.clone()).collect::<Vec<_>>()),
                            _ => None,
                        }).flatten().collect();
                        if !immutable.is_empty() {
                            let mut hits: Vec<(String, &str)> = Vec::new();
                            super::literals::for_each_stmt_deep(&on.body, &mut |st| {
                                if let Statement::MethodCall { target, method, .. } = st {
                                    if immutable.contains(target) && matches!(method.as_str(), "delete" | "remove") { hits.push((target.clone(), "delete")); }
                                }
                            });
                            super::literals::for_each_expr(&on.body, &mut |e| {
                                if let Expr::MethodCall { target, method, .. } = e {
                                    if let Expr::Ident(t) = &target.node {
                                        if immutable.contains(t) && matches!(method.as_str(), "delete" | "remove") { hits.push((t.clone(), "delete")); }
                                    }
                                }
                            });
                            hits.dedup();
                            for (slot, what) in hits {
                                {
                                    issues.push(InterpolationIssue {
                                        message: format!("`{}.{}` in handler `{}`: '{}' is [immutable], so this can only fail at run time (an entry never changes once written)", slot, what, on.signal_name, slot),
                                        span: section.span, warning: true, habit: true, kind: "immutable_write",
                                    });
                                }
                            }
                        }
                    }
                    // `[task]` handlers: a think() ends a step (its writes commit)
                    if on.properties.iter().any(|p| p == "task") {
                        task_lints(&format!("`{}`", on.signal_name), &on.body, &cell.name, &cell_slots, &boundary, section.span, on.properties.iter().any(|p| p == "native"), &mut issues);
                    }
                    for m in count_field_defaults(&on.body) {
                        issues.push(InterpolationIssue { message: m, span: section.span, warning: true, habit: true, kind: "count_field_default" });
                    }
                }
                Section::Every(ev) | Section::After(ev) => {
                    let mut w = Walker::new(&index);
                    w.cell_slots = cell_slots.clone();
                    w.walk_stmts(&ev.body);
                    issues.extend(w.issues);
                    if ev.task {
                        task_lints(&format!("tick `{} {}ms`", if matches!(section.node, Section::Every(_)) { "every" } else { "after" }, ev.interval_ms), &ev.body, &cell.name, &cell_slots, &boundary, section.span, false, &mut issues);
                    }
                }
                _ => {}
            }
        }
    }

    // a `[task]` handler reached from a plain handler or tick runs INSIDE
    // that caller's atomic unit: its think() then holds the lock (20 × 1.5 s
    // served one at a time behind `on request`)
    {
        let cells = super::names::collect_cells(program);
        let mut tasks: HashSet<(String, String)> = HashSet::new();
        for c in &cells {
            for sec in &c.sections {
                if let Section::OnSignal(on) = &sec.node {
                    if on.properties.iter().any(|p| p == "task") { tasks.insert((c.name.clone(), on.signal_name.clone())); }
                }
            }
        }
        if !tasks.is_empty() {
            for c in &cells {
                if matches!(c.kind, CellKind::Test) { continue; }
                for sec in &c.sections {
                    let (who, body, fix): (String, &[Spanned<Statement>], String) = match &sec.node {
                        Section::OnSignal(on) if !on.properties.iter().any(|p| p == "task") =>
                            (format!("handler `{}`", on.signal_name), &on.body, format!("mark it `on {}(…) [task]`", on.signal_name)),
                        Section::Every(ev) if !ev.task => ("this `every` tick".to_string(), &ev.body, format!("write `every {}ms [task] {{ … }}`", ev.interval_ms)),
                        Section::After(ev) if !ev.task => ("this `after` tick".to_string(), &ev.body, format!("write `after {}ms [task] {{ … }}`", ev.interval_ms)),
                        _ => continue,
                    };
                    let mut hit: Vec<String> = Vec::new();
                    super::literals::for_each_expr(body, &mut |e| match e {
                        Expr::FnCall { name, .. } if tasks.contains(&(c.name.clone(), name.clone())) => hit.push(name.clone()),
                        Expr::MethodCall { target, method, .. } => if let Expr::Ident(t) = &target.node {
                            if tasks.contains(&(t.clone(), method.clone())) { hit.push(format!("{}.{}", t, method)); }
                        },
                        _ => {}
                    });
                    super::literals::for_each_stmt_deep(body, &mut |st| if let Statement::MethodCall { target, method, .. } = st {
                        if tasks.contains(&(target.clone(), method.clone())) { hit.push(format!("{}.{}", target, method)); }
                    });
                    hit.sort(); hit.dedup();
                    if let Some(h) = hit.first() {
                        issues.push(InterpolationIssue {
                            message: format!("{} calls [task] `{}` but is not a task itself: `{}` then runs inside the caller's atomic unit and its think() holds the lock (concurrent requests wait) — {}", who, h, h, fix),
                            span: sec.span, warning: true, habit: true, kind: "task_called_atomically",
                        });
                    }
                }
            }
        }
    }
    // horde(target, inputs, opts): the target and the callbacks exist with
    // the right arity, the options are known
    {
        let cells = super::names::collect_cells(program);
        let handler = |c: &str, h: &str| cells.iter().find(|x| x.name == c).and_then(|x| x.sections.iter().find_map(|s| match &s.node {
            Section::OnSignal(on) if on.signal_name == h => Some(on), _ => None }));
        for c in &cells {
            for sec in &c.sections {
                let body = match &sec.node {
                    Section::OnSignal(on) => &on.body,
                    Section::Every(e) | Section::After(e) => &e.body,
                    _ => continue,
                };
                let mut calls: Vec<(bool, &Vec<Spanned<Expr>>)> = Vec::new();
                super::literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, args } = e {
                    if name == "horde" { calls.push((false, args)); }
                    if name == "vote" && args.len() == 3 && handler(&c.name, "vote").is_none() { calls.push((true, args)); }
                });
                for (is_vote, args) in calls {
                    let what = if is_vote { "vote" } else { "horde" };
                    let mut err = |m: String, warning: bool, kind: &'static str| issues.push(InterpolationIssue { message: m, span: args.first().map_or(sec.span, |a| a.span), warning, habit: warning, kind });
                    // what runs, and what it may cost, must be read at the call:
                    // a computed target let an HTTP client run a private
                    // handler, computed options hid the cost and the callbacks
                    if !matches!(args.first().map(|a| &a.node), Some(Expr::FieldAccess { .. }) | Some(Expr::Ident(_)) | Some(Expr::Literal(Literal::String(_)))) {
                        err(format!("{}(): write the handler at the call (`Cell.handler`, `handler` or \"Cell.handler\") — a computed target could run any handler, private ones included, and its cost cannot be proven", what), false, "horde_target");
                    }
                    if let Some(a) = args.get(1) {
                        if matches!(&a.node, Expr::Literal(_) | Expr::Record { .. }) && !is_vote || (!is_vote && matches!(&a.node, Expr::FnCall { name, .. } if name == "map")) {
                            err("horde(): the inputs must be a List (one task per element)".to_string(), false, "horde_inputs");
                        }
                    }
                    if is_vote {
                        match args.get(2).map(|a| &a.node) {
                            Some(Expr::Literal(Literal::Int(k))) if (1..=25).contains(k) => {}
                            Some(Expr::Literal(_)) => err("vote(): k must be an Int in 1..25".to_string(), false, "horde_option"),
                            _ => {}
                        }
                    }
                    if !is_vote && args.len() == 3 && !matches!(&args[2].node, Expr::FnCall { name, .. } if name == "map") {
                        err("horde(): write the options as `map(\"key\", value, …)` at the call — they decide which handlers run and what the horde may cost (options from a variable or a request cannot be checked)".to_string(), false, "horde_option");
                    }
                    let target: Option<(String, String)> = match args.first().map(|a| &a.node) {
                        Some(Expr::FieldAccess { target, field }) => match &target.node { Expr::Ident(cn) => Some((cn.clone(), field.clone())), _ => None },
                        Some(Expr::Ident(h)) => Some((c.name.clone(), h.clone())),
                        Some(Expr::Literal(Literal::String(t))) => Some(match t.split_once('.') { Some((a, b)) => (a.to_string(), b.to_string()), None => (c.name.clone(), t.clone()) }),
                        _ => None,
                    };
                    let opt_keys: Vec<String> = match args.get(2).map(|a| &a.node) {
                        Some(Expr::FnCall { name, args: kv }) if name == "map" && !is_vote => kv.chunks(2).filter_map(|c| match &c[0].node { Expr::Literal(Literal::String(k)) => Some(k.clone()), _ => None }).collect(),
                        _ => Vec::new(),
                    };
                    let want_params = if opt_keys.iter().any(|k| k == "snapshot") { 2 } else { 1 };
                    if opt_keys.iter().any(|k| k == "on_result") && opt_keys.iter().any(|k| k == "apply") {
                        err("horde(): on_result (as results arrive) and apply (in input order, at the end) exclude each other — pick one".to_string(), false, "horde_option");
                    }
                    if let Some((tc, th)) = target {
                        // a bare name may live in another cell
                        let found = handler(&tc, &th).map(|on| (tc.clone(), on)).or_else(|| if tc == c.name {
                            cells.iter().find_map(|x| handler(&x.name, &th).map(|on| (x.name.clone(), on)))
                        } else { None });
                        match found {
                            None => err(format!("{}(): no handler `{}.{}`", what, tc, th), false, "horde_target"),
                            Some((tc, on)) => {
                                let want_params = if is_vote { 1 } else { want_params };
                                if on.params.len() != want_params {
                                    err(if want_params == 2 {
                                        format!("horde(): with a snapshot, `{}.{}` takes two parameters (input, snapshot) — it takes {}", tc, th, on.params.len())
                                    } else {
                                        format!("horde(): `{}.{}` takes {} parameter(s) — a horde handler takes exactly one (the input), or two (input, snapshot) with a `snapshot` option", tc, th, on.params.len())
                                    }, false, "horde_target");
                                } else if !on.properties.iter().any(|p| p == "task") && {
                                    let mut t = false;
                                    super::literals::for_each_expr(&on.body, &mut |e| if matches!(e, Expr::FnCall { name, .. } if name == "think" || name == "think_json") { t = true; });
                                    t
                                } {
                                    err(format!("{}(): `{}.{}` is not [task] — each call then holds the lock through its think(), one at a time: mark it `on {}(…) [task]`", what, tc, th, th), true, "horde_not_task");
                                }
                            }
                        }
                    }
                    if let Some(Expr::FnCall { name, args: kv }) = args.get(2).map(|a| &a.node).filter(|_| !is_vote) {
                        if name == "map" {
                            for pair in kv.chunks(2) {
                                let Expr::Literal(Literal::String(k)) = &pair[0].node else {
                                    err("horde(): option names must be literal strings".to_string(), false, "horde_option");
                                    continue
                                };
                                let val = pair.get(1).map(|v| &v.node);
                                let lit_int = |lo: i64, hi: i64| match val { Some(Expr::Literal(Literal::Int(n))) => Some((lo..=hi).contains(n)), Some(Expr::Literal(_)) => Some(false), _ => None };
                                match k.as_str() {
                                    "concurrency" => if lit_int(1, 1000) == Some(false) { err("horde(): concurrency must be an Int in 1..1000".to_string(), false, "horde_option"); },
                                    "max_attempts" => if lit_int(1, 10) == Some(false) { err("horde(): max_attempts must be an Int in 1..10".to_string(), false, "horde_option"); },
                                    "budget_tokens" => match lit_int(1, i64::MAX) {
                                        Some(false) => err("horde(): budget_tokens must be a positive Int".to_string(), false, "horde_option"),
                                        // a computed ceiling: capped at run time, never proven —
                                        // and from a request parameter, a client sets it
                                        None => err("horde(): budget_tokens is computed — the ceiling holds at run time but its bound is not proven (and a value from a request lets a client set it): write a literal, or clamp it (`min(n, 100000)`)".to_string(), true, "horde_budget_computed"),
                                        _ => {}
                                    },
                                    "seed" => if matches!(val, Some(Expr::Literal(l)) if !matches!(l, Literal::Int(_))) { err("horde(): seed must be an Int".to_string(), false, "horde_option"); },
                                    "instance" => if matches!(val, Some(Expr::Literal(l)) if !matches!(l, Literal::String(_))) { err("horde(): instance names an input field: a String".to_string(), false, "horde_option"); },
                                    "snapshot" => {}
                                    "on_result" | "on_done" | "on_error" | "apply" => {
                                        if !matches!(val, Some(Expr::Literal(Literal::String(_)))) {
                                            err(format!("horde(): {} must name a handler with a literal string (\"_store\") — a computed name could run any handler", k), false, "horde_callback");
                                        }
                                        if let Some(Expr::Literal(Literal::String(h))) = val {
                                            if !h.starts_with('_') && handler(&c.name, h).is_some() {
                                                err(format!("horde(): {} handler `{}` is public — it is an HTTP endpoint too, a client can call it with forged data: name it `_{}`", k, h, h), true, "horde_callback_public");
                                            }
                                            let want: &[usize] = match k.as_str() { "on_result" | "apply" => &[1, 2], "on_done" => &[1], _ => &[2] };
                                            match handler(&c.name, h) {
                                                None => err(format!("horde(): {} names `{}`, which is not a handler of cell `{}`", k, h, c.name), false, "horde_callback"),
                                                Some(on) if k == "on_done" && on.params.len() == 1 && matches!(&on.params[0].ty.node, crate::ast::TypeExpr::Simple(t) if t != "String" && t != "Any") =>
                                                    err(format!("horde(): on_done handler `{}` receives the horde id — declare `{}: String`", h, on.params[0].name), false, "horde_callback"),
                                                Some(on) if k == "on_error" && on.params.len() == 2 && matches!(&on.params[1].ty.node, crate::ast::TypeExpr::Simple(t) if t != "Map" && t != "Any") =>
                                                    err(format!("horde(): on_error handler `{}` receives the error as a Map {{error, kind, detail}} — declare `{}: Map`", h, on.params[1].name), false, "horde_callback"),
                                                Some(on) if !want.contains(&on.params.len()) => err(format!("horde(): {} handler `{}` takes {} parameter(s); it is called with {}", k, h, on.params.len(),
                                                    match k.as_str() { "on_result" | "apply" => "(result) or (input, result)", "on_done" => "(horde_id)", _ => "(input, error)" }), false, "horde_callback"),
                                                _ => {}
                                            }
                                        }
                                    }
                                    other => err(format!("horde(): unknown option `{}` — options: concurrency, max_attempts, budget_tokens, seed, snapshot, instance, on_result, apply, on_done, on_error", other), false, "horde_option"),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // a misspelled handler annotation (`[tsak]`, `[nativ]`) was accepted
    // silently — the handler then ran without it
    for c in super::names::collect_cells(program) {
        for sec in &c.sections {
            if let Section::OnSignal(on) = &sec.node {
                for p in &on.properties {
                    const KNOWN: [&str; 4] = ["native", "task", "deterministic", "record"];
                    if !KNOWN.contains(&p.as_str()) {
                        let near = KNOWN.iter().find(|k| strsim_close(k, p)).map(|k| format!(" — did you mean [{}]?", k)).unwrap_or_default();
                        issues.push(InterpolationIssue {
                            message: format!("unknown handler annotation [{}] on `{}`{} (known: [native], [task], [deterministic], [record])", p, on.signal_name, near),
                            span: sec.span, warning: false, habit: false, kind: "unknown_handler_property",
                        });
                    }
                }
            }
        }
    }
    issues
}

/// Walks one handler body, tracking which names are bound.
///
/// The scope set is add-only: once a name is bound by a `let`, an
/// assignment, a loop variable, a lambda parameter or a match-arm
/// pattern it stays known for the remainder of the handler. This is
/// linear enough to catch use-before-definition in straight-line code
/// while staying conservative around branches and loops.
struct Walker<'a> {
    index: &'a ProgramIndex,
    /// names `let`-bound in the innermost block being walked
    block_lets: HashSet<String>,
    scope: HashSet<String>,
    issues: Vec<InterpolationIssue>,
    /// > 0 while walking inside a `try { }` — issues found there are
    /// recoverable by design and demote to warnings.
    try_depth: usize,
    /// > 0 inside a `for` / `while` body (`break` is legal there)
    loop_depth: usize,
    /// inside a block lambda: `return` there raises at run time
    lambda_depth: usize,
    /// a handler a test cell expects to fail (`assert_fails h(…)`)
    blessed: bool,
    /// the memory slots of the cell being walked (a `let` of another
    /// cell's slot name hides nothing)
    cell_slots: HashSet<String>,
    /// walking a `cell test` (its rules and helpers name every cell's slots)
    in_test: bool,
}

impl<'a> Walker<'a> {
    fn new(index: &'a ProgramIndex) -> Self {
        Self { index, scope: HashSet::new(), block_lets: HashSet::new(), issues: Vec::new(), try_depth: 0, loop_depth: 0, lambda_depth: 0, blessed: false, cell_slots: HashSet::new(), in_test: false }
    }

    fn known(&self, name: &str) -> bool {
        self.scope.contains(name) || self.index.known.contains(name)
    }

    fn walk_stmts(&mut self, stmts: &[Spanned<Statement>]) {
        self.walk_stmts_inner(stmts, false)
    }

    fn walk_stmts_discarding(&mut self, stmts: &[Spanned<Statement>]) {
        self.walk_stmts_inner(stmts, true)
    }

    fn walk_stmts_inner(&mut self, stmts: &[Spanned<Statement>], discard_last: bool) {
        for (i, stmt) in stmts.iter().enumerate() {
            // `let j = 1 2`: the `2` is a statement of its own that does
            // nothing (only the LAST statement of a block is a value)
            if i + 1 < stmts.len() || discard_last {
                if let Statement::ExprStmt { expr } = &stmt.node {
                    let inert = matches!(&expr.node,
                        Expr::Literal(_) | Expr::Ident(_) | Expr::BinaryOp { .. } | Expr::CmpOp { .. }
                        | Expr::ListLiteral(_) | Expr::FieldAccess { .. } | Expr::Not(_))
                        && !expr_has_call(&expr.node);
                    if inert {
                        // name the Soma form where the cause is plain
                        let unclosed = |e: &Expr| matches!(e, Expr::Literal(Literal::String(t))
                            if t.matches('{').count() > t.matches('}').count());
                        let prev_open = i > 0 && match &stmts[i - 1].node {
                            Statement::Return { value } | Statement::ExprStmt { expr: value } | Statement::Let { value, .. } | Statement::Assign { value, .. } => {
                                // `s = s + "<td>{m.k ?? "` — the open string may sit in an operand
                                let mut open = unclosed(&value.node);
                                crate::checker::literals::for_each_in_expr(&value.node, &mut |e| if unclosed(e) { open = true; });
                                open
                            }
                            _ => false,
                        };
                        let hint = match &expr.node {
                            Expr::Ident(w) if w == "and" => " — Soma writes `a && b`".to_string(),
                            Expr::Ident(w) if w == "or" => " — Soma writes `a || b`".to_string(),
                            Expr::Ident(w) if w == "not" => " — Soma writes `!a`".to_string(),
                            _ if prev_open => " — an unescaped `\"` inside `{…}` ends the string: escape it (`\"{m[\\\"k\\\"]}\"`) or bind the value first (`let v = m[\"k\"]`, then `\"{v}\"`)".to_string(),
                            _ => " — a value that is not the last statement of its block is thrown away (a missing operator between two values? `let x = a b` is two statements)".to_string(),
                        };
                        self.issues.push(InterpolationIssue {
                            message: format!(
                                "`{}` on its own does nothing{}",
                                crate::ast::render_expr(&expr.node), hint
                            ),
                            span: expr.span,
                            warning: false,
                            habit: false,
                            kind: "unused_value",
                        });
                    }
                }
            }
            self.walk_stmt(stmt);
        }
    }

    /// Walk a nested block with the runtime's scoping: names `let`-bound
    /// inside it (and the extra names given, e.g. a loop variable or a
    /// lambda parameter) are gone afterwards; plain assignments persist.
    fn scoped(&mut self, extra: &[String], f: impl FnOnce(&mut Self)) {
        let before = self.scope.clone();
        for e in extra {
            self.scope.insert(e.clone());
        }
        f(self);
        // keep plain-assigned names (they persist at runtime), drop the rest
        let assigned: HashSet<String> = self.scope.difference(&before).cloned()
            .filter(|n| !self.block_lets.contains(n) && !extra.contains(n))
            .collect();
        self.scope = before;
        self.scope.extend(assigned);
        self.block_lets.clear();
    }

    fn walk_stmt(&mut self, stmt: &Spanned<Statement>) {
        match &stmt.node {
            Statement::Let { name, value } => {
                self.walk_expr(value);
                if self.cell_slots.contains(name) && !self.scope.contains(name) {
                    self.issues.push(InterpolationIssue {
                        message: format!("`let {name}` hides the memory slot `{name}` for the rest of this block — reads, indexes and writes of `{name}` now mean the local; rename it (e.g. `{name}_local`) unless that is intended"),
                        span: stmt.span,
                        warning: true,
                        habit: true,
                        kind: "local_shadows_slot",
                    });
                }
                self.scope.insert(name.clone());
                self.block_lets.insert(name.clone());
            }
            Statement::Assign { name, value } => {
                self.walk_expr(value);
                if !self.scope.contains(name) && !self.index.slots.contains(name) && !self.index.known.contains(name) {
                    // `totl = x` creates a NEW variable: the typo'd name is
                    // never seen again (`total` keeps its old value)
                    self.issues.push(InterpolationIssue {
                        message: format!("assignment creates a new variable '{name}' — declare it with `let {name} = …`, or fix the name if you meant an existing variable"),
                        span: stmt.span,
                        warning: true,
                        habit: true,
                        kind: "assignment_without_let",
                    });
                }
                if !self.scope.contains(name) && self.index.slots.contains(name) {
                    let kind = if self.index.list_slots.contains(name) { Some("List") } else { Some("Map") };
                    self.issues.push(InterpolationIssue {
                        message: crate::interpreter::slot_assign_message(name, kind),
                        span: stmt.span,
                        warning: false,
                habit: false,
                kind: "slot_assignment",
                    });
                }
                self.scope.insert(name.clone());
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                if self.lambda_depth > 0 && matches!(stmt.node, Statement::Return { .. }) {
                    self.issues.push(InterpolationIssue {
                        message: "`return` inside a block lambda — a lambda is an expression: its value is its LAST expression (`x => { let y = x * 2  y + 1 }`); it cannot leave the handler".to_string(),
                        span: stmt.span,
                        warning: false,
                        habit: false,
                        kind: "return_in_lambda",
                    });
                }
                self.walk_expr(value);
            }
            Statement::ExprStmt { expr } => self.walk_expr(expr),
            Statement::IndexSet { name, index, value } => {
                // `rows[0] = v` on a slot is a slot write, not a local:
                // it must not hide a later `rows = …` from the check above
                if !self.index.slots.contains(name) || self.scope.contains(name) {
                    self.scope.insert(name.clone());
                }
                self.walk_expr(index);
                self.walk_expr(value);
            }
            Statement::If { condition, then_body, else_body } => {
                self.int_builtin_as_bool(condition);
                self.walk_expr(condition);
                self.scoped(&[], |w| w.walk_stmts(then_body));
                self.scoped(&[], |w| w.walk_stmts(else_body));
            }
            Statement::For { var, iter, body, .. } => {
                // `for r in rows { r.v = 2 }`: r is a COPY — the write is
                // lost unless r is used afterwards (written back, pushed…)
                if let Some(pos) = body.iter().position(|b| matches!(&b.node, Statement::IndexSet { name, .. } if name == var)) {
                    let mut later: HashSet<String> = HashSet::new();
                    for b in &body[pos + 1..] {
                        if let Statement::IndexSet { name, index, value } = &b.node {
                            if name == var {
                                crate::interpreter::free_names_expr(&index.node, &mut later);
                                crate::interpreter::free_names_expr(&value.node, &mut later);
                                continue;
                            }
                        }
                        crate::interpreter::free_names_stmts(std::slice::from_ref(b), &mut later);
                    }
                    if !later.contains(var.as_str()) {
                        self.issues.push(InterpolationIssue {
                            message: format!(
                                "`{var}` is a copy of the element, so this write is lost when the iteration ends — write it back (`for i in range(0, len(xs)) {{ let {var} = xs[i]  {var}.f = …  xs[i] = {var} }}`, or `rows[i] = {var}` for a List slot)"
                            ),
                            span: body[pos].span,
                            warning: true,
                            habit: true,
                            kind: "write_to_loop_copy",
                        });
                    }
                }
                self.walk_expr(iter);
                let var = var.clone();
                self.scoped(&[var], |w| {
                    // Pre-bind everything the body binds: on iteration 2+
                    // those names exist, so flagging them would be a false
                    // positive for any string evaluated after the binding.
                    bind_stmts(body, &mut w.scope);
                    w.loop_depth += 1;
                    // a loop body has no value: its LAST statement is thrown
                    // away too (`s = s + "<td>{m.k ?? ""}</td>"` split in two
                    // passed check as the body's "last value")
                    w.walk_stmts_discarding(body);
                    w.loop_depth -= 1;
                });
            }
            Statement::While { condition, body, .. } => {
                self.int_builtin_as_bool(condition);
                self.walk_expr(condition);
                self.scoped(&[], |w| {
                    bind_stmts(body, &mut w.scope);
                    w.loop_depth += 1;
                    // a loop body has no value: its LAST statement is thrown
                    // away too (`s = s + "<td>{m.k ?? ""}</td>"` split in two
                    // passed check as the body's "last value")
                    w.walk_stmts_discarding(body);
                    w.loop_depth -= 1;
                });
            }
            Statement::Emit { signal_name, args } => {
                // every listener runs with these arguments: one taking a
                // different count raised at run time (`emit ev(1)` for
                // `on ev(a, b)`) and rolled the emitter back
                for ((cell, h), (lo, n)) in self.index.arity.iter() {
                    if h == signal_name && (args.len() < *lo || args.len() > *n) {
                        self.issues.push(InterpolationIssue {
                            message: format!("`emit {signal_name}(…)` passes {} argument(s), but the listener {cell}.{signal_name} takes {}", args.len(), n),
                            span: stmt.span,
                            warning: false,
                            habit: false,
                            kind: "arity",
                        });
                        break;
                    }
                }
                for a in args {
                    self.walk_expr(a);
                }
            }
            Statement::Require { constraint, else_signal } => {
                self.walk_constraint(&constraint.node);
                // `require regex_match(s, p) else bad`: an Int builtin as the
                // condition passed check, then raised "cannot compare Int and
                // Bool" (the `if` form was already caught)
                self.int_builtin_in_constraint(constraint);
                // `require … else Bad "detail {x}"`: the detail is interpolated
                // when the require fails — an undefined name answered 500
                // instead of the 400 `Bad`
                if let Some((_, detail)) = else_signal.split_once('\u{1f}') {
                    self.scan_string(detail, stmt.span);
                }
            }
            Statement::MethodCall { target, method, args } => {
                // statement position: `xs.push(x)` on a local does nothing
                // (push returns a new list) and `m.set(k, v)` raises
                if matches!(method.as_str(), "set" | "put" | "delete" | "push" | "append")
                    && self.scope.contains(target) && !self.index.slots.contains(target)
                {
                    let fix = match method.as_str() {
                        "push" | "append" => format!("a local list is rebuilt: `{target} = push({target}, x)`"),
                        "delete" => format!("a local map is rebuilt: `{target} = without({target}, k)`"),
                        _ => format!("a local map is written with brackets: `{target}[k] = v`"),
                    };
                    self.issues.push(InterpolationIssue {
                        message: format!("`.{method}()` is a memory-slot method and '{target}' is a local — {fix}"),
                        span: stmt.span,
                        warning: false,
                        habit: false,
                        kind: "slot_method_on_local",
                    });
                }
                for a in args {
                    self.walk_expr(a);
                }
            }
            Statement::Break | Statement::Continue => {
                if self.loop_depth == 0 {
                    self.issues.push(InterpolationIssue {
                        message: format!("`{}` outside a loop — it only means something inside `for` / `while` (to leave a handler, `return`)",
                            if matches!(stmt.node, Statement::Break) { "break" } else { "continue" }),
                        span: stmt.span,
                        warning: false,
                        habit: false,
                        kind: "break_outside_loop",
                    });
                }
            }
        }
    }

    fn walk_constraint(&mut self, c: &Constraint) {
        match c {
            Constraint::Comparison { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Constraint::And(a, b) | Constraint::Or(a, b) => {
                self.walk_constraint(&a.node);
                self.walk_constraint(&b.node);
            }
            Constraint::Not(inner) => self.walk_constraint(&inner.node),
            // Predicate args are not evaluated at runtime — stay silent.
            Constraint::Predicate { .. } | Constraint::Descriptive(_) => {}
        }
    }

    fn int_builtin_in_constraint(&mut self, c: &Spanned<Constraint>) {
        match &c.node {
            Constraint::Predicate { name, .. } if !self.index.handler_map.contains_key(name) => {
                if let Some(b) = crate::interpreter::builtins::registry::BUILTINS.iter().find(|b| b.name == name) {
                    if b.signature.trim_end().ends_with("-> Int") {
                        self.issues.push(InterpolationIssue {
                            message: format!("{}() answers an Int (1 or 0), not a Bool — compare it: `require {}(…) == 1 else …`", name, name),
                            span: c.span,
                            warning: false,
                            habit: false,
                            kind: "int_as_bool",
                        });
                    }
                }
            }
            // a bare call is parsed as `call == true`
            Constraint::Comparison { left, op: CmpOp::Eq | CmpOp::Ne, right } if matches!(right.node, Expr::Literal(Literal::Bool(_))) => {
                self.int_builtin_as_bool(left);
            }
            Constraint::And(a, b) | Constraint::Or(a, b) => { self.int_builtin_in_constraint(a); self.int_builtin_in_constraint(b); }
            Constraint::Not(inner) => self.int_builtin_in_constraint(inner),
            _ => {}
        }
    }

    /// `if !regex_match(s, p)`: a builtin that answers Int 1/0 used as a
    /// Bool raises "expected Bool, got Int" at run time — say it here.
    fn int_builtin_as_bool(&mut self, e: &Spanned<Expr>) {
        let mut parts = vec![e];
        while let Some(x) = parts.pop() {
            match &x.node {
                Expr::BinaryOp { left, op: BinOp::And | BinOp::Or, right } => { parts.push(left); parts.push(right); }
                Expr::FnCall { name, .. } if !self.index.handler_map.contains_key(name) => {
                    if let Some(b) = crate::interpreter::builtins::registry::BUILTINS.iter().find(|b| b.name == name) {
                        if b.signature.trim_end().ends_with("-> Int") {
                            self.issues.push(InterpolationIssue {
                                message: format!("{}() answers an Int (1 or 0), not a Bool — compare it: `{}(…) == 1`", name, name),
                                span: x.span,
                                warning: false,
                                habit: false,
                                kind: "int_as_bool",
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn walk_expr(&mut self, expr: &Spanned<Expr>) {
        let span = expr.span;
        match &expr.node {
            Expr::Literal(Literal::String(s)) => self.scan_string(s, span),
            Expr::Literal(_) => {}
            // A bare identifier read. Same conservative scope as the
            // interpolation scan: add-only, so a name bound anywhere
            // earlier in the handler is known. What is left is a name
            // bound NOWHERE — a typo or an incomplete rename, which would
            // otherwise only fail at runtime, on the path that reads it.
            Expr::Ident(name) => {
                // a test rule naming a slot two cells declare: it silently
                // read one of them (a false green test)
                if self.in_test && !self.scope.contains(name) {
                    if let Some(owners) = self.index.slot_owners.get(name) {
                        if owners.len() > 1 {
                            self.issues.push(InterpolationIssue {
                                message: format!("`{name}` is a slot of several cells ({}) — in a test it is ambiguous: read it through a handler of the cell you mean", owners.join(", ")),
                                span,
                                warning: false,
                                habit: false,
                                kind: "ambiguous_slot",
                            });
                            return;
                        }
                    }
                }
                // another cell's slot by its bare name: it resolved to that
                // cell's storage (an imported library read and rewrote the
                // importer's `api_keys`), unseen by verify
                if !self.in_test && self.index.slots.contains(name) && !self.cell_slots.contains(name) && !self.scope.contains(name) {
                    self.issues.push(InterpolationIssue {
                        message: format!("`{name}` is a memory slot of another cell — a cell's slots are private to it: call a handler of the cell that owns `{name}`"),
                        span,
                        warning: false,
                        habit: false,
                        kind: "foreign_slot",
                    });
                    return;
                }
                // `let f = len`, `|> map(dbl)`: a function is not a value
                // (it raised "undefined variable" at run time)
                let is_fn = self.index.handler_map.contains_key(name) || super::names::builtin_names().contains(name.as_str());
                if is_fn && !self.scope.contains(name) && !self.index.slots.contains(name) && !self.index.variants.contains(name) && !self.index.cells.contains(name)
                    && !matches!(name.as_str(), "true" | "false" | "_" | "self" | "value" | "key" | "size")
                {
                    self.issues.push(InterpolationIssue {
                        message: format!("`{name}` is a function, not a value — pass a lambda (`x => {name}(x)`) or call it (`{name}(…)`)"),
                        span,
                        warning: self.try_depth > 0,
                        habit: false,
                        kind: "function_as_value",
                    });
                    return;
                }
                // `value` / `key` / `size` exist only inside a memory invariant
                // (`require value >= 0` passed check and raised at run time)
                if !self.known(name)
                    && !super::names::builtin_names().contains(name.as_str())
                    && !matches!(name.as_str(), "true" | "false" | "_" | "self")
                {
                    if matches!(name.as_str(), "value" | "key" | "size") {
                        self.issues.push(InterpolationIssue {
                            message: format!("`{name}` exists only inside a memory `invariant` (the value / key being written, the slot's size) — in a handler use the local that holds it"),
                            span,
                            warning: false,
                            habit: false,
                            kind: "undefined_variable",
                        });
                        return;
                    }
                    self.report_undefined_ident(name, span);
                }
            }
            Expr::FieldAccess { target, .. } => self.walk_expr(target),
            Expr::Index { target, index } => { self.walk_expr(target); self.walk_expr(index); }
            Expr::MethodCall { target, method, args } => {
                // `.set/.delete/.push` are slot methods: on a LOCAL map or
                // list they were a runtime error ("not a memory slot")
                if let Expr::Ident(name) = &target.node {
                    if matches!(method.as_str(), "set" | "put" | "delete")
                        && self.scope.contains(name) && !self.index.slots.contains(name)
                    {
                        let fix = match method.as_str() {
                            "push" | "append" => format!("a local list is rebuilt: `{name} = push({name}, x)`"),
                            "delete" => format!("a local map is rebuilt: `{name} = without({name}, k)`"),
                            _ => format!("a local map is written with brackets: `{name}[k] = v`"),
                        };
                        self.issues.push(InterpolationIssue {
                            message: format!("`.{method}()` is a memory-slot method and '{name}' is a local — {fix}"),
                            span: expr.span,
                            warning: false,
                            habit: false,
                            kind: "slot_method_on_local",
                        });
                    }
                }
                // `Ledger.deposit(a, n)` — a call into another cell: the
                // handler must exist there (the runtime resolves it by name)
                if let Expr::Ident(cell) = &target.node {
                    if self.index.cells.contains(cell) && !self.scope.contains(cell) {
                        let defined = self.index.handler_map.get(method)
                            .map(|cs| cs.contains(cell)).unwrap_or(false);
                        if let Some((lo, n)) = self.index.arity.get(&(cell.clone(), method.clone())) {
                            if args.len() < *lo || args.len() > *n {
                                self.issues.push(InterpolationIssue {
                                    message: format!("`{cell}.{method}(…)` passes {} argument(s), but the handler {cell}.{method} takes {}", args.len(), n),
                                    span,
                                    warning: false,
                                    habit: false,
                                    kind: "arity",
                                });
                            }
                        }
                        if !defined {
                            let owners = self.index.handler_map.get(method).cloned().unwrap_or_default();
                            let hint = if !owners.is_empty() {
                                format!(" — '{method}' is a handler of {}", owners.join(", "))
                            } else {
                                let names: Vec<&String> = self.index.handler_map.iter()
                                    .filter(|(_, cs)| cs.contains(cell)).map(|(h, _)| h).collect();
                                match suggest(method, names.iter().copied()) {
                                    Some(s) => format!(" (did you mean '{s}'?)"),
                                    None => String::new(),
                                }
                            };
                            self.issues.push(InterpolationIssue {
                                message: format!("cell '{cell}' has no handler '{method}'{hint}"),
                                span: target.span,
                                warning: false,
                habit: false,
                kind: "unknown_handler",
                            });
                        }
                    }
                }
                self.walk_expr(target);
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::FnCall { name, args } => {
                for (i, a) in args.iter().enumerate() {
                    // `horde(review, …)` / `horde(Reviewer.review, …)` names a handler
                    if i == 0 && (name == "horde" || (name == "vote" && args.len() == 3)) && matches!(&a.node, Expr::Ident(_) | Expr::FieldAccess { .. }) { continue; }
                    self.walk_expr(a);
                }
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::Not(inner) => {
                self.int_builtin_as_bool(inner);
                self.walk_expr(inner);
            }
            Expr::Try(inner) | Expr::TryPropagate(inner) => {
                self.try_depth += 1;
                self.walk_expr(inner);
                self.try_depth -= 1;
            }
            Expr::Pipe { left, right } => {
                // the runtime refuses anything but a call on the right:
                // say so here, with the parenthesised form
                if !matches!(right.node, Expr::FnCall { .. } | Expr::MethodCall { .. } | Expr::Pipe { .. } | Expr::Ident(_)) {
                    self.issues.push(InterpolationIssue {
                        message: "the right side of `|>` must be a call — `x |> f(a)` means f(x, a); to combine the result, parenthesise: `(x |> f()) + 1`".to_string(),
                        span: right.span,
                        warning: false,
                habit: false,
                kind: "pipe_right_side",
                    });
                }
                self.walk_expr(left);
                // `xs |> len`: a bare function name on the right is a call
                if !matches!(&right.node, Expr::Ident(n) if self.index.handler_map.contains_key(n) || super::names::builtin_names().contains(n.as_str())) {
                    self.walk_expr(right);
                }
            }
            Expr::Record { fields, .. } => {
                for (_, v) in fields {
                    self.walk_expr(v);
                }
            }
            Expr::ListLiteral(items) => {
                for item in items {
                    self.walk_expr(item);
                }
            }
            Expr::Lambda { param, body } => {
                let param = param.clone();
                self.scoped(&[param], |w| w.walk_expr(body));
            }
            Expr::LambdaBlock { param, stmts, result } => {
                // `xs |> map(x => { t = t + x  x })`: a lambda gets a COPY of
                // the outer locals — the assignment changes nothing outside
                fn outer_assign(stmts: &[Spanned<Statement>], own: &mut HashSet<String>) -> Option<String> {
                    for st in stmts {
                        match &st.node {
                            Statement::Let { name, .. } => { own.insert(name.clone()); }
                            Statement::Assign { name, .. } if !own.contains(name) => return Some(name.clone()),
                            Statement::If { then_body, else_body, .. } => {
                                if let Some(n) = outer_assign(then_body, &mut own.clone()).or_else(|| outer_assign(else_body, &mut own.clone())) { return Some(n); }
                            }
                            Statement::For { var, body, .. } => { let mut o = own.clone(); o.insert(var.clone()); if let Some(n) = outer_assign(body, &mut o) { return Some(n); } }
                            Statement::While { body, .. } => { if let Some(n) = outer_assign(body, &mut own.clone()) { return Some(n); } }
                            _ => {}
                        }
                    }
                    None
                }
                let mut own: HashSet<String> = HashSet::new();
                own.insert(param.clone());
                if let Some(n) = outer_assign(stmts, &mut own) {
                    self.issues.push(InterpolationIssue {
                        message: format!("`{n} = …` inside a lambda changes a copy — the outer `{n}` is unchanged after it; accumulate with a `for` loop, or `reduce`/`sum` over the list"),
                        span,
                        warning: true,
                        habit: true,
                        kind: "lambda_assign",
                    });
                }
                let param = param.clone();
                self.scoped(&[param], |w| {
                    // a lambda body is not inside the enclosing loop
                    let outer = std::mem::replace(&mut w.loop_depth, 0);
                    w.lambda_depth += 1;
                    w.walk_stmts(stmts);
                    w.walk_expr(result);
                    w.lambda_depth -= 1;
                    w.loop_depth = outer;
                });
            }
            Expr::Match { subject, arms } => {
                if arms.is_empty() {
                    self.issues.push(InterpolationIssue {
                        message: "`match` with no arms always evaluates to `()` — add at least one arm (`_ -> …`)".to_string(),
                        span,
                        warning: false,
                        habit: false,
                        kind: "empty_match",
                    });
                }
                self.walk_expr(subject);
                // an arm's bindings end with the arm (the runtime scopes them)
                for arm in arms {
                    let mut bound: HashSet<String> = HashSet::new();
                    bind_pattern(&arm.pattern, &mut bound);
                    let bound: Vec<String> = bound.into_iter().collect();
                    self.scoped(&bound, |w| {
                        if let Some(g) = &arm.guard {
                            w.walk_expr(g);
                        }
                        w.walk_stmts(&arm.body);
                        w.walk_expr(&arm.result);
                    });
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.walk_expr(condition);
                self.walk_stmts(then_body);
                self.walk_expr(then_result);
                self.walk_stmts(else_body);
                self.walk_expr(else_result);
            }
        }
    }

    // ── String scanning (mirrors interpolate_string) ─────────────────

    fn scan_string(&mut self, s: &str, span: Span) {
        let bytes = s.as_bytes();
        let mut pos = 0;
        while pos < s.len() {
            let byte = bytes[pos];
            // {{ and }} escape literal braces
            if byte == b'{' && bytes.get(pos + 1) == Some(&b'{') {
                pos += 2;
                continue;
            }
            if byte == b'}' && bytes.get(pos + 1) == Some(&b'}') {
                pos += 2;
                continue;
            }
            if byte == b'{' {
                // `"v={x} and {"` — the literal ends INSIDE a `{`: a nested
                // quote split the string. Name that, not the stray word after.
                // (a literal that IS just "{" is a brace, not a split string)
                if !s[pos + 1..].contains('}') && s[pos + 1..].trim().is_empty() && s.trim() != "{" {
                    self.issues.push(InterpolationIssue {
                        message: "string literal ends inside `{…}` — a nested quote inside an interpolation? bind the inner value first: `let inner = \"lit\"` then `\"… {inner}\"` (a literal brace is `{{`)".to_string(),
                        span,
                        warning: true,
                        habit: true,
                        kind: "nested_quote",
                    });
                    return;
                }
                if let Some(end) = crate::interpreter::interp_segment_end(s, pos) {
                    let expr_str = &s[pos + 1..pos + 1 + end];
                    // Skipped as CSS/HTML — runtime advances one byte
                    // and rescans, so nested segments are still found.
                    if expr_str.is_empty() || expr_str.contains(':') || expr_str.contains(';') {
                        pos += 1;
                        continue;
                    }
                    if self.check_segment(expr_str, span) {
                        // Segment evaluates at runtime — skip past it.
                        pos = pos + 1 + end + 1;
                    } else {
                        // Not parseable as an expression: literal text.
                        pos += 1;
                    }
                    continue;
                }
            }
            pos += 1;
        }
    }

    /// Check a single `{...}` segment. Returns true if the runtime
    /// would treat it as an expression (and so skip past it), false if
    /// it renders literally.
    fn check_segment(&mut self, expr_str: &str, span: Span) -> bool {
        // `{4}` / `{2,3}`: a regex quantifier, literal text at run time
        if !expr_str.is_empty() && expr_str.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ') {
            return false;
        }
        // Fast path mirror: a bare word is looked up in env directly.
        if expr_str.chars().all(|c| c.is_alphanumeric() || c == '_') {
            let starts_like_ident = expr_str
                .chars()
                .next()
                .map_or(false, |c| c.is_alphabetic() || c == '_');
            if starts_like_ident && !self.in_test && !self.scope.contains(expr_str)
                && self.index.slots.contains(expr_str) && !self.cell_slots.contains(expr_str) {
                self.issues.push(InterpolationIssue {
                    message: format!("`{expr_str}` is a memory slot of another cell — a cell's slots are private to it: call a handler of the cell that owns `{expr_str}`"),
                    span,
                    warning: false,
                    habit: false,
                    kind: "foreign_slot",
                });
                return true;
            }
            if starts_like_ident && !self.known(expr_str) {
                self.report_undefined_var(expr_str, span);
            } else if starts_like_ident && self.index.cells.contains(expr_str) && !self.scope.contains(expr_str) && !self.index.slots.contains(expr_str) {
                // `"{S}"` — a cell is not a value (`"{Other.ask(1)}"` is fine)
                self.issues.push(InterpolationIssue {
                    message: format!("`{{{expr_str}}}` in a string: `{expr_str}` is a cell, not a value — call one of its handlers (`{{{expr_str}.handler(…)}}`) or write `{{{{{expr_str}}}}}` for the literal text"),
                    span, warning: false, habit: false, kind: "function_as_value",
                });
            } else if starts_like_ident {
                self.check_segment_expr(&Expr::Ident(expr_str.to_string()), &mut HashSet::new(), span);
            }
            return true;
        }

        // Nested string literals (`{m[\"k\"]}`, escaped in the source)
        // are evaluated by the runtime: the parser mirror below judges them.

        // Full path mirror: parse the wrapped segment with the real
        // parser. Failure to parse means it renders literally.
        let wrapped = format!("cell _T {{ on _e() {{ return {} }} }}", expr_str);
        let mut lexer = crate::lexer::Lexer::new(&wrapped);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut parser = crate::parser::Parser::new(tokens);
        let program = match parser.parse_program() {
            Ok(p) => p,
            Err(_) => return false,
        };
        // `{total - fee junk}` parses as TWO statements: the runtime refuses it
        let several = program.cells.first().map_or(false, |cell| cell.node.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.body.len() > 1)));
        if several {
            self.issues.push(InterpolationIssue {
                message: format!("string interpolation `{{{}}}` is not one expression — the part after it would be dropped; bind it with a let first", expr_str),
                span,
                warning: false,
                habit: false,
                kind: "interpolation_trailing",
            });
            return true;
        }
        let expr = program.cells.first().and_then(|cell| {
            cell.node.sections.iter().find_map(|s| {
                if let Section::OnSignal(ref on) = s.node {
                    on.body.first().and_then(|stmt| {
                        if let Statement::Return { ref value } = stmt.node {
                            Some(value.node.clone())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            })
        });
        match expr {
            Some(e) => {
                let mut bound = HashSet::new();
                self.check_segment_expr(&e, &mut bound, span);
                true
            }
            None => false,
        }
    }

    /// Walk a parsed segment expression looking for provably unknown
    /// identifier leaves and call targets. Exotic forms stay silent.
    fn check_segment_expr(&mut self, expr: &Expr, bound: &mut HashSet<String>, span: Span) {
        match expr {
            Expr::Ident(name) => {
                // another cell's slot inside `{…}` (it reached that cell's
                // storage: a write past its invariant, a read of its secret)
                if !self.in_test && !bound.contains(name) && !self.scope.contains(name)
                    && self.index.slots.contains(name) && !self.cell_slots.contains(name) {
                    self.issues.push(InterpolationIssue {
                        message: format!("`{name}` is a memory slot of another cell — a cell's slots are private to it: call a handler of the cell that owns `{name}`"),
                        span,
                        warning: false,
                        habit: false,
                        kind: "foreign_slot",
                    });
                    return;
                }
                if !bound.contains(name) && !self.known(name) {
                    self.report_undefined_var(name, span);
                } else if !bound.contains(name) && !self.scope.contains(name) && !self.index.slots.contains(name) && !self.index.variants.contains(name)
                    && (self.index.handler_map.contains_key(name) || super::names::builtin_names().contains(name.as_str()))
                    && !matches!(name.as_str(), "true" | "false")
                {
                    // `"{len}"`, `"{helper}"`, `"{S}"`: passed check, raised
                    // "undefined variable" at run time
                    self.issues.push(InterpolationIssue {
                        message: format!("`{{{name}}}` in a string: `{name}` is a function or a cell, not a value — call it (`{{{name}(…)}}`) or write `{{{{{name}}}}}` for the literal text"),
                        span, warning: false, habit: false, kind: "function_as_value",
                    });
                }
            }
            Expr::FnCall { name, args } => {
                if !bound.contains(name) && !self.known(name) {
                    self.report_undefined_fn(name, span);
                }
                for a in args {
                    self.check_segment_expr(&a.node, bound, span);
                }
            }
            Expr::FieldAccess { target, .. } => {
                self.check_segment_expr(&target.node, bound, span);
            }
            Expr::Index { target, index } => {
                self.check_segment_expr(&target.node, bound, span);
                self.check_segment_expr(&index.node, bound, span);
            }
            Expr::MethodCall { target, args, .. } => {
                self.check_segment_expr(&target.node, bound, span);
                for a in args {
                    self.check_segment_expr(&a.node, bound, span);
                }
            }
            Expr::BinaryOp { left, right, .. }
            | Expr::CmpOp { left, right, .. }
            | Expr::Pipe { left, right } => {
                self.check_segment_expr(&left.node, bound, span);
                self.check_segment_expr(&right.node, bound, span);
            }
            Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
                self.check_segment_expr(&inner.node, bound, span);
            }
            Expr::ListLiteral(items) => {
                for item in items {
                    self.check_segment_expr(&item.node, bound, span);
                }
            }
            Expr::Lambda { param, body } => {
                bound.insert(param.clone());
                self.check_segment_expr(&body.node, bound, span);
            }
            Expr::LambdaBlock { param, stmts, result } => {
                bound.insert(param.clone());
                bind_stmts(stmts, bound);
                self.check_segment_expr(&result.node, bound, span);
            }
            // Nested string literals, match/if expressions and record
            // literals inside an interpolation segment: too exotic to
            // reason about scope — stay silent.
            Expr::Literal(_) | Expr::Match { .. } | Expr::IfExpr { .. } | Expr::Record { .. } => {}
        }
    }

    fn report_undefined_var(&mut self, name: &str, span: Span) {
        let suggestion = suggest(name, self.scope.iter().chain(self.index.known.iter()))
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.issues.push(InterpolationIssue {
            message: format!(
                "string interpolation references undefined variable '{name}'{suggestion} — \
                 define it before this line, or escape literal braces as '{{{{{name}}}}}'"
            ),
            span,
            warning: self.try_depth > 0 || self.blessed,
                habit: false,
                kind: "undefined_variable",
        });
    }

    fn report_undefined_ident(&mut self, name: &str, span: Span) {
        let foreign = match name {
            "null" | "None" | "nil" | "undefined" | "NULL" => Some("Soma's null is `()`: `if x == () { … }`, `x ?? default`"),
            "const" | "var" | "val" => Some("bindings are `let x = …`; reassign with `x = …`"),
            "function" | "def" | "fn" | "func" | "lambda" => Some("a function is a handler: `on name(x: Int) { … }`; a lambda is `x => expr`"),
            "this" | "self" => Some("there is no `this`: memory slots and handlers of the cell are in scope by name"),
            "True" | "TRUE" => Some("write `true`"),
            "False" | "FALSE" => Some("write `false`"),
            _ => None,
        };
        if let Some(hint) = foreign {
            self.issues.push(InterpolationIssue {
                message: format!("'{name}' does not exist in Soma — {hint}"),
                span,
                warning: self.try_depth > 0,
                habit: false,
                kind: "foreign_idiom",
            });
            return;
        }
        let suggestion = suggest(name, self.scope.iter().chain(self.index.known.iter()))
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.issues.push(InterpolationIssue {
            message: format!(
                "undefined variable '{name}'{suggestion} — no let, parameter, loop variable, \
                 match binding or memory slot with that name is in scope"
            ),
            span,
            warning: self.try_depth > 0,
                habit: false,
                kind: "undefined_variable",
        });
    }

    fn report_undefined_fn(&mut self, name: &str, span: Span) {
        let suggestion = suggest(name, self.scope.iter().chain(self.index.known.iter()))
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.issues.push(InterpolationIssue {
            message: format!(
                "string interpolation calls undefined function '{name}'{suggestion} — \
                 define it before this line, or escape literal braces as '{{{{{name}(...)}}}}'"
            ),
            span,
            warning: self.try_depth > 0 || self.blessed,
                habit: false,
                kind: "undefined_function",
        });
    }
}

/// Collect every name a statement list can bind (lets, assignments,
/// loop variables, lambda params, match-arm patterns), recursively.
/// Used to pre-bind loop bodies and to skip lambda-local names.
fn bind_stmts(stmts: &[Spanned<Statement>], scope: &mut HashSet<String>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                scope.insert(name.clone());
                bind_expr(&value.node, scope);
            }
            Statement::For { var, iter, body, .. } => {
                scope.insert(var.clone());
                bind_expr(&iter.node, scope);
                bind_stmts(body, scope);
            }
            Statement::While { condition, body, .. } => {
                bind_expr(&condition.node, scope);
                bind_stmts(body, scope);
            }
            Statement::If { condition, then_body, else_body } => {
                bind_expr(&condition.node, scope);
                bind_stmts(then_body, scope);
                bind_stmts(else_body, scope);
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                bind_expr(&value.node, scope);
            }
            Statement::ExprStmt { expr } => bind_expr(&expr.node, scope),
            Statement::IndexSet { name, index, value } => { scope.insert(name.clone()); bind_expr(&index.node, scope); bind_expr(&value.node, scope); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                for a in args {
                    bind_expr(&a.node, scope);
                }
            }
            Statement::Require { .. } | Statement::Break | Statement::Continue => {}
        }
    }
}

fn bind_expr(expr: &Expr, scope: &mut HashSet<String>) {
    match expr {
        Expr::Lambda { param, body } => {
            scope.insert(param.clone());
            bind_expr(&body.node, scope);
        }
        Expr::LambdaBlock { param, stmts, result } => {
            scope.insert(param.clone());
            bind_stmts(stmts, scope);
            bind_expr(&result.node, scope);
        }
        Expr::Match { subject, arms } => {
            bind_expr(&subject.node, scope);
            for arm in arms {
                bind_pattern(&arm.pattern, scope);
                bind_stmts(&arm.body, scope);
                bind_expr(&arm.result.node, scope);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            bind_expr(&condition.node, scope);
            bind_stmts(then_body, scope);
            bind_expr(&then_result.node, scope);
            bind_stmts(else_body, scope);
            bind_expr(&else_result.node, scope);
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                bind_expr(&a.node, scope);
            }
        }
        Expr::MethodCall { target, args, .. } => {
            bind_expr(&target.node, scope);
            for a in args {
                bind_expr(&a.node, scope);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            bind_expr(&left.node, scope);
            bind_expr(&right.node, scope);
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            bind_expr(&inner.node, scope);
        }
        Expr::ListLiteral(items) => {
            for item in items {
                bind_expr(&item.node, scope);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                bind_expr(&v.node, scope);
            }
        }
        Expr::FieldAccess { target, .. } => bind_expr(&target.node, scope),
        Expr::Index { target, index } => { bind_expr(&target.node, scope); bind_expr(&index.node, scope); }
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Names bound by a match pattern.
pub(super) fn bind_pattern(pattern: &MatchPattern, scope: &mut HashSet<String>) {
    match pattern {
        MatchPattern::Variable(name) => {
            scope.insert(name.clone());
        }
        MatchPattern::Or(alts) => {
            for alt in alts {
                bind_pattern(alt, scope);
            }
        }
        MatchPattern::MapDestructure(fields) => {
            for (_, sub) in fields {
                bind_pattern(sub, scope);
            }
        }
        MatchPattern::StringPrefix { rest, .. } => {
            scope.insert(rest.clone());
        }
        MatchPattern::Variant { fields, .. } => match fields {
            VariantPatternFields::Unit => {}
            VariantPatternFields::Tuple(pats) => {
                for p in pats {
                    bind_pattern(p, scope);
                }
            }
            VariantPatternFields::Struct { fields, .. } => {
                for (_, p) in fields {
                    bind_pattern(p, scope);
                }
            }
        },
        MatchPattern::Literal(_) | MatchPattern::Wildcard | MatchPattern::Range { .. } => {}
    }
}

fn expr_has_call(e: &Expr) -> bool {
    match e {
        Expr::FnCall { .. } | Expr::MethodCall { .. } | Expr::Pipe { .. } => true,
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => expr_has_call(&left.node) || expr_has_call(&right.node),
        Expr::Not(i) => expr_has_call(&i.node),
        Expr::FieldAccess { target, .. } => expr_has_call(&target.node),
        Expr::ListLiteral(items) => items.iter().any(|i| expr_has_call(&i.node)),
        _ => false,
    }
}


/// One edit (insert, delete, substitute or swap of neighbours) apart.
fn strsim_close(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let n = a.len(); let m = b.len();
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n { d[i][0] = i; }
    for j in 0..=m { d[0][j] = j; }
    for i in 1..=n { for j in 1..=m {
        let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
        d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
        if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] { d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1); }
    } }
    d[n][m] <= 2
}


/// Handlers (cell, name) whose run reaches a `[task]` step boundary — a
/// think() / think_json() / vote(), directly or through a handler they
/// call (same cell, `Cell.h`, an emit's listeners): each of those waits for
/// the model outside the lock.
pub(crate) struct Boundary { handlers: HashSet<(String, String)>, signals: HashSet<String>, bodies: std::collections::HashMap<(String, String), Vec<Spanned<Statement>>> }

fn direct_boundary(e: &Expr) -> bool {
    match e {
        Expr::FnCall { name, .. } => matches!(name.as_str(), "think" | "think_json" | "vote"),
        // "{think(…)}" inside a string
        Expr::Literal(Literal::String(t)) => t.contains('{') && (t.contains("think(") || t.contains("think_json(") || t.contains("vote(")),
        _ => false,
    }
}

impl Boundary {
    fn expr(&self, cell: &str, e: &Expr) -> bool {
        direct_boundary(e) || match e {
            Expr::FnCall { name, args } if name == "delegate" => match (args.first().map(|a| &a.node), args.get(1).map(|a| &a.node)) {
                (Some(Expr::Literal(Literal::String(c))), Some(Expr::Literal(Literal::String(h)))) => self.handlers.contains(&(c.clone(), h.clone())),
                _ => false,
            },
            Expr::FnCall { name, .. } => self.handlers.contains(&(cell.to_string(), name.clone())) || (!name.contains('.') && self.handlers.iter().any(|(_, h)| h == name) && !matches!(name.as_str(), "horde")),
            Expr::MethodCall { target, method, .. } => matches!(&target.node, Expr::Ident(c) if self.handlers.contains(&(c.clone(), method.clone()))),
            _ => false,
        }
    }
    pub(crate) fn stmts(&self, cell: &str, stmts: &[Spanned<Statement>]) -> bool {
        let mut t = false;
        super::literals::for_each_expr(stmts, &mut |e| if self.expr(cell, e) { t = true; });
        super::literals::for_each_stmt_deep(stmts, &mut |st| match st {
            Statement::Emit { signal_name, .. } if self.signals.contains(signal_name) => t = true,
            Statement::MethodCall { target, method, .. } if self.handlers.contains(&(target.clone(), method.clone())) => t = true,
            _ => {}
        });
        t
    }
    fn in_expr(&self, cell: &str, e: &Expr) -> bool {
        let mut t = self.expr(cell, e);
        super::literals::for_each_in_expr(e, &mut |x| if self.expr(cell, x) { t = true; });
        let mut stmts: Vec<Spanned<Statement>> = Vec::new();
        super::literals::for_each_stmt_in_expr(e, &mut |st| stmts.push(Spanned::new(st.clone(), crate::ast::Span::new(0, 0))));
        t || self.stmts(cell, &stmts)
    }
}

fn step_boundaries(program: &Program) -> Boundary {
    let cells = super::names::collect_cells(program);
    let mut b = Boundary { handlers: HashSet::new(), signals: HashSet::new(), bodies: Default::default() };
    for c in &cells { for sec in &c.sections { if let Section::OnSignal(on) = &sec.node { b.bodies.insert((c.name.clone(), on.signal_name.clone()), on.body.clone()); } } }
    loop {
        let mut grew = false;
        for c in &cells {
            for sec in &c.sections {
                let Section::OnSignal(on) = &sec.node else { continue };
                let key = (c.name.clone(), on.signal_name.clone());
                if b.handlers.contains(&key) { continue; }
                if b.stmts(&c.name, &on.body) {
                    b.handlers.insert(key);
                    b.signals.insert(on.signal_name.clone());
                    grew = true;
                }
            }
        }
        if !grew { break; }
    }
    b
}

#[allow(clippy::too_many_arguments)]
fn task_lints(label: &str, body: &[Spanned<Statement>], cell: &str, cell_slots: &HashSet<String>, boundary: &Boundary, span: crate::ast::Span, native: bool, issues: &mut Vec<InterpolationIssue>) {
    let has_think = |stmts: &[Spanned<Statement>]| boundary.stmts(cell, stmts);
    let slot_writes = |stmts: &[Spanned<Statement>]| -> Vec<String> {
        let mut w: Vec<String> = Vec::new();
        super::literals::for_each_stmt_deep(stmts, &mut |st| match st {
            Statement::MethodCall { target, method, .. } if cell_slots.contains(target) && matches!(method.as_str(), "set" | "push" | "delete" | "remove" | "put") => w.push(target.clone()),
            Statement::IndexSet { name, .. } if cell_slots.contains(name) => w.push(name.clone()),
            _ => {}
        });
        super::literals::for_each_expr(stmts, &mut |e| if let Expr::MethodCall { target, method, .. } = e {
            if let Expr::Ident(t) = &target.node { if cell_slots.contains(t) && matches!(method.as_str(), "set" | "push" | "delete" | "remove" | "put") { w.push(t.clone()); } }
        });
        w
    };
    if native {
        issues.push(InterpolationIssue { message: format!("handler {} is both [task] and [native] — a native handler cannot call think(); drop one", label), span, warning: false, habit: false, kind: "task_native" });
    }
    // a stale read: read before a step boundary, written at or after it —
    // unless read again after it (the fresh value is what is written)
    let reads_of = |stmts: &[Spanned<Statement>], with_helpers: bool| -> Vec<String> {
        let mut read: Vec<String> = Vec::new();
        let mut scan = |stmts: &[Spanned<Statement>], read: &mut Vec<String>| super::literals::for_each_expr(stmts, &mut |e| match e {
            Expr::MethodCall { target, method, .. } if matches!(method.as_str(), "get" | "has" | "len" | "size" | "keys" | "values") => { if let Expr::Ident(t) = &target.node { if cell_slots.contains(t) { read.push(t.clone()); } } }
            Expr::FieldAccess { target, field } if matches!(field.as_str(), "len" | "size" | "count") => { if let Expr::Ident(t) = &target.node { if cell_slots.contains(t) { read.push(t.clone()); } } }
            Expr::Index { target, .. } => { if let Expr::Ident(t) = &target.node { if cell_slots.contains(t) { read.push(t.clone()); } } }
            Expr::FnCall { name, args } if matches!(name.as_str(), "len" | "keys" | "values") => { if let Some(Expr::Ident(t)) = args.first().map(|a| &a.node) { if cell_slots.contains(t) { read.push(t.clone()); } } }
            _ => {}
        });
        scan(stmts, &mut read);
        if with_helpers {
            // …and in this cell's helpers it calls (`let b = _get(k)`, `_chk()`)
            let mut helpers: Vec<&Vec<Spanned<Statement>>> = Vec::new();
            super::literals::for_each_expr(stmts, &mut |e| if let Expr::FnCall { name, .. } = e { if let Some(b) = boundary.bodies.get(&(cell.to_string(), name.clone())) { helpers.push(b); } });
            for b in helpers { scan(b, &mut read); }
        }
        read
    };
    let writes_of = |stmts: &[Spanned<Statement>]| -> Vec<String> {
        let mut w = slot_writes(stmts);
        let mut helpers: Vec<&Vec<Spanned<Statement>>> = Vec::new();
        super::literals::for_each_expr(stmts, &mut |e| if let Expr::FnCall { name, .. } = e { if let Some(b) = boundary.bodies.get(&(cell.to_string(), name.clone())) { helpers.push(b); } });
        for b in helpers { w.extend(slot_writes(b)); }
        w
    };
    if let Some(i) = body.iter().position(|st| has_think(std::slice::from_ref(st))) {
        let read = reads_of(&body[..=i], true);
        let written = writes_of(&body[i..]);
        let reread = reads_of(&body[i + 1..], false);
        let mut stale: Vec<String> = read.into_iter().filter(|r| written.contains(r) && !reread.contains(r)).collect();
        stale.sort(); stale.dedup();
        for sl in stale {
            issues.push(InterpolationIssue {
                message: format!("[task] {} reads '{}' before a think() / vote() (or a handler that makes one) and writes it after: other requests may write '{}' while the model runs — read (and re-check) it after", label, sl, sl),
                span, warning: true, habit: true, kind: "task_stale_read",
            });
        }
    } else {
        issues.push(InterpolationIssue {
            message: format!("[task] on {} makes no think() / vote() (nor calls a handler that does) — it runs as one atomic unit like a plain handler", label),
            span, warning: true, habit: true, kind: "task_without_think",
        });
    }
    // a `try` holding writes AND a boundary: the writes before it commit
    // there and are not undone if the try fails
    super::literals::for_each_expr(body, &mut |e| if let Expr::Try(inner) = e {
        let mut stmts: Vec<Spanned<Statement>> = Vec::new();
        super::literals::for_each_stmt_in_expr(&inner.node, &mut |st| stmts.push(Spanned::new(st.clone(), span)));
        // …a helper of this cell called in the try writes too
        let mut helper_writes = false;
        super::literals::for_each_in_expr(&inner.node, &mut |x| if let Expr::FnCall { name, .. } = x {
            if let Some(b) = boundary.bodies.get(&(cell.to_string(), name.clone())) { if !slot_writes(b).is_empty() { helper_writes = true; } }
        });
        // a transition() or an emit commits with the step too
        let mut effects = false;
        super::literals::for_each_in_expr(&inner.node, &mut |x| if matches!(x, Expr::FnCall { name, .. } if name == "transition") { effects = true; });
        super::literals::for_each_stmt_in_expr(&inner.node, &mut |st| if matches!(st, Statement::Emit { .. }) { effects = true; });
        if boundary.in_expr(cell, &inner.node) && (!slot_writes(&stmts).is_empty() || helper_writes || effects) {
            issues.push(InterpolationIssue {
                message: format!("[task] {}: this try writes a slot and reaches a think() / vote() — the writes made before it commit there and are NOT undone if the try fails; write after it", label),
                span, warning: true, habit: true, kind: "task_try_write",
            });
        }
    });
}
