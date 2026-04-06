use std::path::PathBuf;

use crate::ast::{self, TypeExpr};
use crate::checker::{self, CheckError};
use crate::registry::Registry;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

/// Format a TypeExpr back to source syntax (e.g. "String", "Map<String, Int>").
fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Simple(name) => name.clone(),
        TypeExpr::Generic { name, args } => {
            let inner: Vec<String> = args.iter().map(|a| format_type(&a.node)).collect();
            format!("{}<{}>", name, inner.join(", "))
        }
        TypeExpr::CellRef { cell, member } => format!("{}.{}", cell, member),
    }
}

/// Format a parameter list for an `on` handler (e.g. "id: String, name: String").
fn format_params(params: &[ast::Param]) -> String {
    params.iter()
        .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A single applied fix, for reporting.
struct AppliedFix {
    description: String,
}

/// Run the checker and collect errors from the source at `path`.
fn run_checker(path: &PathBuf, registry: &mut Registry) -> Vec<CheckError> {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);

    let mut chk = checker::Checker::new(registry);
    chk.check(&program);
    chk.errors
}

/// Parse the program and return the AST (for reading face declarations).
fn parse_program(path: &PathBuf) -> ast::Program {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    program
}

/// Find the face signal declaration for `signal_name` in the given cell.
fn find_face_signal<'a>(cell: &'a ast::CellDef, signal_name: &str) -> Option<&'a ast::SignalDecl> {
    for section in &cell.sections {
        if let ast::Section::Face(ref face) = section.node {
            for decl in &face.declarations {
                if let ast::FaceDecl::Signal(ref sig) = decl.node {
                    if sig.name == signal_name {
                        return Some(sig);
                    }
                }
            }
        }
    }
    None
}

/// Find the cell definition by name in the program.
fn find_cell<'a>(program: &'a ast::Program, cell_name: &str) -> Option<&'a ast::CellDef> {
    program.cells.iter()
        .find(|c| c.node.name == cell_name)
        .map(|c| &c.node)
}

/// Find the byte offset of the last `}` that closes a cell block in the source.
/// We search for `cell <name>` and then find its matching closing brace.
fn find_cell_closing_brace(source: &str, cell_name: &str) -> Option<usize> {
    // Look for `cell <name>` pattern
    let pattern = format!("cell {}", cell_name);
    let cell_start = source.find(&pattern)?;

    // Find the opening `{` after the cell name
    let after_name = cell_start + pattern.len();
    let open_brace = source[after_name..].find('{')? + after_name;

    // Walk forward counting braces to find the matching close
    let mut depth = 0;
    let mut in_string = false;
    let mut prev_char = '\0';
    for (i, ch) in source[open_brace..].char_indices() {
        match ch {
            '"' if prev_char != '\\' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(open_brace + i);
                }
            }
            _ => {}
        }
        prev_char = ch;
    }
    None
}

/// Find the second occurrence of `signal <name>(` in the source and return
/// the byte range of that entire line (including trailing newline).
fn find_duplicate_signal_line(source: &str, signal_name: &str) -> Option<(usize, usize)> {
    let pattern = format!("signal {}(", signal_name);
    let first = source.find(&pattern)?;
    let rest = &source[first + 1..];
    let second_offset = rest.find(&pattern)?;
    let second_abs = first + 1 + second_offset;

    // Find the start of this line
    let line_start = source[..second_abs].rfind('\n').map(|i| i + 1).unwrap_or(second_abs);
    // Find the end of this line
    let line_end = source[second_abs..].find('\n')
        .map(|i| second_abs + i + 1)
        .unwrap_or(source.len());

    Some((line_start, line_end))
}

/// Compute the indentation used inside a cell (look at existing `on` handlers or sections).
fn detect_indent(source: &str, cell_name: &str) -> String {
    let pattern = format!("cell {}", cell_name);
    if let Some(cell_start) = source.find(&pattern) {
        // Look for an `on ` line inside the cell to detect its indent
        for line in source[cell_start..].lines().skip(1) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("on ") || trimmed.starts_with("face ")
                || trimmed.starts_with("memory ") || trimmed.starts_with("state ")
            {
                let indent_len = line.len() - trimmed.len();
                return line[..indent_len].to_string();
            }
        }
    }
    "    ".to_string()
}

pub fn cmd_fix(path: &PathBuf, json: bool, registry: &mut Registry) {
    // 1. Run checker to find errors
    let errors = run_checker(path, registry);

    if errors.is_empty() {
        if json {
            println!("{}", serde_json::json!({
                "fixes": [],
                "fix_count": 0,
                "passed": true,
            }));
        } else {
            println!("  \u{2713} All checks passed — nothing to fix.");
        }
        return;
    }

    // 2. Parse program for AST info (face declarations, etc.)
    let program = parse_program(path);
    let mut source = read_source(path);
    let mut fixes: Vec<AppliedFix> = Vec::new();

    // Process errors in reverse span order so earlier fixes don't shift later offsets.
    // Collect fixable actions first, then apply.
    struct FixAction {
        kind: FixKind,
    }
    enum FixKind {
        InsertHandler { cell_name: String, signal_name: String, params: String },
        RemoveContradictoryProperty { slot_name: String, property_to_remove: String },
        RemoveDuplicateSignal { signal_name: String },
    }

    let mut actions: Vec<FixAction> = Vec::new();

    for error in &errors {
        match error {
            CheckError::MissingHandler { cell, signal, .. } => {
                // Look up the face declaration to get exact params
                if let Some(cell_def) = find_cell(&program, cell) {
                    if let Some(sig_decl) = find_face_signal(cell_def, signal) {
                        let params = format_params(&sig_decl.params);
                        actions.push(FixAction {
                            kind: FixKind::InsertHandler {
                                cell_name: cell.clone(),
                                signal_name: signal.clone(),
                                params,
                            },
                        });
                    }
                }
            }
            CheckError::PropertyContradiction { slot, a, b, .. } => {
                // Remove the "less safe" property. If one is ephemeral, remove it.
                // Otherwise remove b (the second one listed).
                let to_remove = if a == "ephemeral" {
                    a.clone()
                } else if b == "ephemeral" {
                    b.clone()
                } else {
                    // Default: remove the second property
                    b.clone()
                };
                actions.push(FixAction {
                    kind: FixKind::RemoveContradictoryProperty {
                        slot_name: slot.clone(),
                        property_to_remove: to_remove,
                    },
                });
            }
            CheckError::DuplicateSignal { name, .. } => {
                actions.push(FixAction {
                    kind: FixKind::RemoveDuplicateSignal {
                        signal_name: name.clone(),
                    },
                });
            }
            CheckError::ParamCountMismatch { cell, signal, expected, actual, .. } => {
                // Too risky to auto-fix — just report
                if !json {
                    println!("  \u{2717} skipped: handler '{}' in '{}' has {} params, face expects {} (manual fix required)",
                        signal, cell, actual, expected);
                }
            }
            _ => {
                // Not auto-fixable
            }
        }
    }

    // 3. Deduplicate: don't insert the same handler twice
    {
        let mut seen_handlers: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        actions.retain(|a| {
            if let FixKind::InsertHandler { cell_name, signal_name, .. } = &a.kind {
                seen_handlers.insert((cell_name.clone(), signal_name.clone()))
            } else {
                true
            }
        });
    }

    // 4. Apply fixes
    for action in &actions {
        match &action.kind {
            FixKind::InsertHandler { cell_name, signal_name, params } => {
                if let Some(close_pos) = find_cell_closing_brace(&source, cell_name) {
                    let indent = detect_indent(&source, cell_name);
                    let handler = format!(
                        "\n{indent}on {signal_name}({params}) {{\n{indent}    return map(\"status\", \"ok\")\n{indent}}}\n",
                        indent = indent,
                        signal_name = signal_name,
                        params = params,
                    );
                    source.insert_str(close_pos, &handler);
                    fixes.push(AppliedFix {
                        description: format!("added handler: on {}({}) {{ ... }}", signal_name, params),
                    });
                }
            }
            FixKind::RemoveContradictoryProperty { slot_name, property_to_remove } => {
                // Find pattern like `[persistent, ephemeral]` or `[ephemeral, persistent]`
                // and remove the offending property from the bracket list.
                // Strategy: find the slot line and remove the property from its bracket list.
                let removed = remove_property_from_source(&mut source, slot_name, property_to_remove);
                if removed {
                    fixes.push(AppliedFix {
                        description: format!("removed contradictory property: {} on slot '{}'", property_to_remove, slot_name),
                    });
                }
            }
            FixKind::RemoveDuplicateSignal { signal_name } => {
                if let Some((start, end)) = find_duplicate_signal_line(&source, signal_name) {
                    source.replace_range(start..end, "");
                    fixes.push(AppliedFix {
                        description: format!("removed duplicate signal declaration: signal {}(...)", signal_name),
                    });
                }
            }
        }
    }

    if fixes.is_empty() {
        if json {
            println!("{}", serde_json::json!({
                "fixes": [],
                "fix_count": 0,
                "passed": false,
                "note": "errors found but none are auto-fixable",
            }));
        } else {
            println!("  \u{2717} No auto-fixable errors found.");
        }
        return;
    }

    // 4. Write the fixed file back
    std::fs::write(path, &source).unwrap_or_else(|e| {
        eprintln!("error: cannot write '{}': {}", path.display(), e);
        std::process::exit(1);
    });

    // 5. Print what was fixed
    if !json {
        for fix in &fixes {
            println!("  \u{2713} {}", fix.description);
        }
        println!("  \u{2713} {} fix(es) applied, re-checking...", fixes.len());
    }

    // 6. Re-run checker to verify
    let recheck_errors = run_checker(path, registry);

    if json {
        let fix_list: Vec<serde_json::Value> = fixes.iter()
            .map(|f| serde_json::json!({ "description": f.description }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "fixes": fix_list,
            "fix_count": fixes.len(),
            "passed": recheck_errors.is_empty(),
            "remaining_errors": recheck_errors.len(),
        })).unwrap());
    } else {
        if recheck_errors.is_empty() {
            println!("  \u{2713} All checks passed.");
        } else {
            println!("  \u{2717} {} error(s) remain after fixes.", recheck_errors.len());
        }
    }
}

/// Remove a specific property from a memory slot's property list in the source.
/// E.g., given slot "items" with `[persistent, ephemeral]`, remove "ephemeral"
/// to produce `[persistent]`.
fn remove_property_from_source(source: &mut String, slot_name: &str, prop: &str) -> bool {
    // Find the slot declaration line. Look for the slot name preceded by `[...]`
    // Typical patterns:
    //   items: Map<String, Item> = map() [persistent, ephemeral]
    //   items: List<String> [ephemeral, persistent]
    // We need to find a `[` ... `]` block on the same line as the slot name,
    // where the bracket contains the property.

    // Search for slot name followed eventually by a bracket list containing the property
    let mut search_from = 0;
    while let Some(slot_pos) = source[search_from..].find(slot_name) {
        let abs_pos = search_from + slot_pos;
        // Find the end of this line
        let line_end = source[abs_pos..].find('\n').map(|i| abs_pos + i).unwrap_or(source.len());
        let line = &source[abs_pos..line_end];

        // Find a bracket list in this line
        if let Some(bracket_start) = line.find('[') {
            if let Some(bracket_end) = line[bracket_start..].find(']') {
                let bracket_abs_start = abs_pos + bracket_start;
                let bracket_abs_end = abs_pos + bracket_start + bracket_end + 1;
                let bracket_content = &source[bracket_abs_start + 1..bracket_abs_end - 1];

                if bracket_content.contains(prop) {
                    // Parse the properties
                    let props: Vec<&str> = bracket_content.split(',')
                        .map(|s| s.trim())
                        .collect();
                    let remaining: Vec<&str> = props.iter()
                        .copied()
                        .filter(|p| *p != prop)
                        .collect();

                    let new_bracket = if remaining.is_empty() {
                        String::new() // Remove brackets entirely
                    } else {
                        format!("[{}]", remaining.join(", "))
                    };

                    // Also remove leading space before bracket if we're replacing
                    let space_start = if bracket_abs_start > 0
                        && source.as_bytes()[bracket_abs_start - 1] == b' '
                        && new_bracket.is_empty()
                    {
                        bracket_abs_start - 1
                    } else {
                        bracket_abs_start
                    };

                    source.replace_range(space_start..bracket_abs_end, &if new_bracket.is_empty() {
                        String::new()
                    } else {
                        if space_start < bracket_abs_start { format!(" {}", new_bracket) } else { new_bracket }
                    });
                    return true;
                }
            }
        }
        search_from = abs_pos + slot_name.len();
    }
    false
}
