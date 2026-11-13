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
    // (the path: skeleton.lock is found from the program — `fix` said "All
    // checks passed" on a program `check` refused)
    chk.source = Some((file_str.clone(), source.clone()));
    chk.check(&program);
    chk.errors
}

/// The first lex / parse error of `source`, if any (without exiting).
fn parse_error(source: &str) -> Option<String> {
    let mut lex = crate::lexer::Lexer::new(source);
    match lex.tokenize() {
        Err(e) => Some(e.to_string()),
        Ok(tokens) => crate::parser::Parser::new(tokens).parse_program().err().map(|e| e.to_string()),
    }
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

/// The offset of `word` in `source` from `from` on, as a whole word (`log`
/// is not inside `catalog`, `cell App` not inside `cell AppStore`).
fn find_word(source: &str, from: usize, word: &str) -> Option<usize> {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    // not inside a comment or a string (`// Helper is used by cell App`
    // put App's handler into Helper): searched in a copy where they are
    // blanked, byte for byte
    let masked = mask_comments_and_strings(source);
    let source = masked.as_str();
    let mut at = from;
    while let Some(i) = source[at..].find(word) {
        let abs = at + i;
        let before_ok = source[..abs].chars().next_back().map_or(true, |c| !ident(c));
        let after_ok = source[abs + word.len()..].chars().next().map_or(true, |c| !ident(c));
        if before_ok && after_ok { return Some(abs); }
        at = abs + word.len();
    }
    None
}

/// `source` with every comment and string literal replaced by spaces (ASCII
/// spaces for ASCII bytes, the same byte length overall).
fn mask_comments_and_strings(source: &str) -> String {
    let b = source.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    let blank = |out: &mut Vec<u8>, a: usize, z: usize| for k in a..z.min(out.len()) { if out[k] < 0x80 && out[k] != b'\n' { out[k] = b' '; } else if out[k] >= 0x80 { out[k] = b' '; } };
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            let end = source[i..].find('\n').map(|e| i + e).unwrap_or(b.len());
            blank(&mut out, i, end); i = end;
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            let end = source[i + 2..].find("*/").map(|e| i + 2 + e + 2).unwrap_or(b.len());
            blank(&mut out, i, end); i = end;
        } else if source[i..].starts_with("\"\"\"") {
            // a triple-quoted string may hold `"` and braces
            let end = source[i + 3..].find("\"\"\"").map(|e| i + 3 + e).unwrap_or(b.len());
            blank(&mut out, i + 3, end); i = end + 3;
        } else if b[i] == b'"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' { if b[j] == b'\\' { j += 1; } j += 1; }
            blank(&mut out, i + 1, j); i = j + 1;
        } else { i += 1; }
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

/// The declaration `cell Name` — or `cell agent Name`, `cell <kind> Name`
/// (the literal `cell Name` never matched those: a missing handler of an
/// agent could not be added) — outside comments and strings: the byte
/// offsets of `cell` and of the end of the name.
fn find_cell_decl(source: &str, cell_name: &str) -> Option<(usize, usize)> {
    let masked = mask_comments_and_strings(source);
    let re = regex::Regex::new(&format!(r"\bcell\s+(?:[a-z_]+\s+)?{}\b", regex::escape(cell_name))).ok()?;
    re.find(&masked).map(|m| (m.start(), m.end()))
}

/// Find the byte offset of the last `}` that closes a cell block in the source.
/// We search for `cell <name>` and then find its matching closing brace.
fn find_cell_closing_brace(source: &str, cell_name: &str) -> Option<usize> {
    let (_, after_name) = find_cell_decl(source, cell_name)?;

    // Find the opening `{` after the cell name — counting braces outside
    // comments and strings (`// closes early: }` ended the cell there)
    let masked = mask_comments_and_strings(source);
    let source = masked.as_str();
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
fn find_duplicate_signal_line(source: &str, from: usize, signal_name: &str) -> Option<(usize, usize)> {
    // in the error's cell, outside comments and strings (a comment naming
    // the signal counted as its first declaration)
    let masked = mask_comments_and_strings(source);
    let pattern = format!("signal {}(", signal_name);
    let first = from + masked[from..].find(&pattern)?;
    let rest = &masked[first + 1..];
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
    if let Some((cell_start, _)) = find_cell_decl(source, cell_name) {
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

/// Syntax habits the parser names with a fix-it, applied to the text before
/// anything else can run: `;` between statements, `=>` in match arms, `-> T`
/// on a handler. Each round re-lexes / re-parses; stops at the first error
/// it cannot mend. Returns the descriptions of what changed.
/// Positions from the lexer / parser count CHARACTERS: slicing the text
/// needs the byte offset (`"日本語";` panicked, `"éé;;"` edited inside the
/// string).
fn char_to_byte(src: &str, c: usize) -> usize {
    src.char_indices().nth(c).map(|(b, _)| b).unwrap_or(src.len())
}

fn syntax_fixes(source: &mut String, skipped: &mut Vec<String>) -> Vec<String> {
    let mut done: Vec<String> = Vec::new();
    for _ in 0..40 {
        let mut lex = crate::lexer::Lexer::new(source);
        let tokens = match lex.tokenize() {
            Ok(t) => t,
            Err(e) => {
                let Some(cpos) = super::lex_error_position(&e) else { break };
                let pos = char_to_byte(source, cpos);
                // the character there may be several bytes (`print(“hi”)`
                // panicked slicing inside `“`): step over it whole
                let next = pos + source[pos..].chars().next().map_or(0, char::len_utf8);
                // …unless what follows would continue the expression: `let d =
                // a; -b` became `let d = a   -b`, which is a - b
                let continues = source[next..].trim_start_matches([' ', '\t'])
                    .starts_with(['-', '+', '*', '/', '%', '(', '[', '.', '|', '&', '=', '<', '>', '?', '!']);
                if matches!(e, crate::lexer::LexError::UnexpectedChar { .. }) && source[pos..].starts_with(';') && !continues {
                    // `a = 1; b = 2` — two spaces separate statements on one line
                    let (line, _) = crate::interpreter::span_to_location(source, cpos);
                    source.replace_range(pos..pos + 1, "  ");
                    done.push(format!("line {}: removed ';' (one statement per line, or two spaces between statements)", line));
                    continue;
                }
                break;
            }
        };
        let mut parser = crate::parser::Parser::new(tokens);
        match parser.parse_program() {
            Ok(_) => break,
            Err(e) => {
                let msg = e.to_string();
                let Some(cspan) = e.span() else { break };
                // a position in an imported file: not this text
                if cspan.start > source.chars().count() { break; }
                let (line, _) = crate::interpreter::span_to_location(source, cspan.start);
                let span = crate::ast::Span::new(char_to_byte(source, cspan.start), char_to_byte(source, cspan.end));
                if msg.starts_with("match arms use '->'") && source[span.start..].starts_with("=>") {
                    source.replace_range(span.start..span.start + 2, "->");
                    done.push(format!("line {}: match arm `=>` → `->`", line));
                    continue;
                }
                if msg.starts_with("handlers do not declare return types") && source[span.start..].starts_with("->") {
                    // the return type MOVES to the face signal (it used to be
                    // dropped: the contract was lost); when that cannot be
                    // done safely the text is left as it is, reported
                    match move_return_type_to_face(source, span.start) {
                        Ok(desc) => {
                            done.push(format!("line {}: {}", line, desc));
                            continue;
                        }
                        Err(why) => {
                            skipped.push(format!("line {}: the handler declares a return type, and {} — move `-> T` onto its `signal` inside face {{ }} by hand", line, why));
                        }
                    }
                }
                break;
            }
        }
    }
    done
}

/// `on f(x: Int) -> Map<String, Int> {` (a parse error at `->`, byte
/// `arrow`): the type moves onto the face signal — added to `signal f(x:
/// Int)` in the cell's face, or a new `signal` line, or a new face — and
/// the handler line keeps everything else (`[native]`, the brace). Err: why
/// it was not done (nothing changed).
fn move_return_type_to_face(source: &mut String, arrow: usize) -> Result<String, String> {
    let line_start = source[..arrow].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[arrow..].find('\n').map_or(source.len(), |i| arrow + i);
    // the type runs to the annotation or the brace, on the SAME line
    // (searching further found the NEXT cell's `{`)
    let rel = source[arrow..line_end].find(['[', '{']).ok_or("its type does not end on the same line")?;
    let ty = source[arrow + 2..arrow + rel].trim().to_string();
    if ty.is_empty() { return Err("its type is empty".to_string()); }
    // `on f(…)` right before the arrow (a one-line `cell P { on run() -> Int { … } }` too)
    let head_re = regex::Regex::new(r"(?:^|[\s{])on\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)\)\s*$").unwrap();
    let caps = head_re.captures(&source[line_start..arrow]).ok_or("its parameters do not fit on one line")?;
    let indent: String = source[line_start..arrow].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let name = caps[1].to_string();
    let params = caps[2].trim().to_string();
    // split at the commas OUTSIDE brackets: `m: Map<String, List<Int>>`
    // is one parameter (its inner comma made a "parameter" without a type)
    let mut parts: Vec<String> = vec![String::new()];
    let mut depth = 0i64;
    for ch in params.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => { parts.push(String::new()); continue; }
            _ => {}
        }
        parts.last_mut().unwrap().push(ch);
    }
    if !params.is_empty() && parts.iter().any(|p| !p.contains(':')) {
        return Err("a parameter has no type (a face signal needs them)".to_string());
    }
    let masked = mask_comments_and_strings(source);
    // the matching `}` of the `{` at `open`
    let close_of = |open: usize| -> Option<usize> {
        let mut depth = 0i64;
        for (i, b) in masked.as_bytes()[open..].iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => { depth -= 1; if depth == 0 { return Some(open + i); } }
                _ => {}
            }
        }
        None
    };
    // the innermost cell whose braces hold the handler
    let cell_re = regex::Regex::new(r"\bcell\s+(?:[a-z_]+\s+)?[A-Za-z_][A-Za-z0-9_]*[^{]*\{").unwrap();
    let (cell_open, cell_close) = cell_re.find_iter(&masked[..arrow]).map(|m| m.end() - 1)
        .filter_map(|o| close_of(o).map(|c| (o, c)))
        .filter(|(_, c)| *c > arrow)
        .last().ok_or("no enclosing cell was found")?;
    // its OWN face: at depth 1 inside the cell
    let face_re = regex::Regex::new(r"\bface\s*\{").unwrap();
    let depth_at = |pos: usize| -> i64 {
        masked.as_bytes()[cell_open..pos].iter().fold(0i64, |d, b| match b { b'{' => d + 1, b'}' => d - 1, _ => d })
    };
    let face = face_re.find_iter(&masked[cell_open..cell_close]).map(|m| (cell_open + m.start(), cell_open + m.end() - 1))
        .find(|(start, _)| depth_at(*start) == 1);
    let unit = if indent.contains('\t') { "\t" } else { "    " };
    // edits as (position, bytes removed, text inserted), applied last first
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut handler_start = arrow;
    while handler_start > line_start && source.as_bytes()[handler_start - 1] == b' ' { handler_start -= 1; }
    edits.push((handler_start, arrow + rel - handler_start, " ".to_string()));
    let desc;
    match face {
        Some((_, face_open)) => {
            let face_close = close_of(face_open).ok_or("its face is not closed")?;
            let sig_re = regex::Regex::new(&format!(r"\bsignal\s+{}\s*\(", regex::escape(&name))).unwrap();
            if let Some(m) = sig_re.find(&masked[face_open..face_close]) {
                let paren = face_open + m.end() - 1;
                let mut depth = 0i64;
                let mut close_paren = None;
                for (i, b) in masked.as_bytes()[paren..face_close].iter().enumerate() {
                    match b { b'(' => depth += 1, b')' => { depth -= 1; if depth == 0 { close_paren = Some(paren + i); break; } } _ => {} }
                }
                let close_paren = close_paren.ok_or("its face signal is not closed")?;
                let sig_line_end = source[close_paren..].find('\n').map_or(source.len(), |i| close_paren + i);
                let after = masked[close_paren + 1..sig_line_end].trim_start();
                if let Some(declared) = after.strip_prefix("->") {
                    let declared = declared.split('[').next().unwrap_or("").trim();
                    if declared.split_whitespace().collect::<String>() != ty.split_whitespace().collect::<String>() {
                        return Err(format!("its face signal already returns `{}`, not `{}`", declared, ty));
                    }
                    desc = format!("removed `-> {}` from `on {}` (its face signal already declares it)", ty, name);
                } else {
                    edits.push((close_paren + 1, 0, format!(" -> {}", ty)));
                    desc = format!("moved the return type to the face: `signal {}(…) -> {}` (`on {}` no longer declares it)", name, ty, name);
                }
            } else {
                // a new signal line, just before the face's closing brace
                let close_line = source[..face_close].rfind('\n').map_or(0, |i| i + 1);
                let face_indent: String = source[close_line..face_close].chars().take_while(|c| c.is_whitespace()).collect();
                let at = if source[close_line..face_close].trim().is_empty() { close_line } else { face_close };
                let lead = if at == face_close { "\n".to_string() } else { String::new() };
                edits.push((at, 0, format!("{}{}{}signal {}({}) -> {}\n{}", lead, face_indent, unit, name, params, ty, if at == face_close { face_indent.clone() } else { String::new() })));
                desc = format!("moved the return type to the face: added `signal {}({}) -> {}`", name, params, ty);
            }
        }
        None => {
            // a new face, first thing in the cell
            edits.push((cell_open + 1, 0, format!("\n{i}face {{\n{i}{u}signal {n}({p}) -> {t}\n{i}}}", i = indent, u = unit, n = name, p = params, t = ty)));
            desc = format!("moved the return type to a new face: `signal {}({}) -> {}`", name, params, ty);
        }
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    for (at, len, text) in edits {
        source.replace_range(at..at + len, &text);
    }
    Ok(desc)
}

pub fn cmd_fix(path: &PathBuf, json: bool, registry: &mut Registry) {
    let mut fixes: Vec<AppliedFix> = Vec::new();
    // what a fix-it named but could not apply safely (left unchanged)
    let mut skipped: Vec<String> = Vec::new();
    // 0. Syntax fix-its first: nothing else runs on a file that does not parse
    {
        let mut src = read_source(path);
        let done = syntax_fixes(&mut src, &mut skipped);
        if !done.is_empty() {
            std::fs::write(path, &src).unwrap_or_else(|e| {
                eprintln!("error: cannot write '{}': {}", path.display(), e);
                std::process::exit(1);
            });
            if !json {
                for d in &done { println!("  \u{2713} {}", d); }
            }
            fixes.extend(done.into_iter().map(|description| AppliedFix { description }));
        }
        if !json {
            for s in &skipped { println!("  \u{2717} skipped: {} (manual fix required)", s); }
        }
        // still no parse: the checker cannot run. --json reports what WAS
        // written (the fixes were applied, and the report said nothing)
        if json {
            if let Some(err) = parse_error(&src) {
                let mut doc = super::fix_json(serde_json::json!(fixes.iter().map(|f| serde_json::json!({"description": f.description})).collect::<Vec<_>>()), false, 1, serde_json::json!(skipped));
                doc["parse_error"] = serde_json::json!(err);
                doc["error"] = serde_json::json!(err);
                println!("{}", serde_json::to_string_pretty(&doc).unwrap());
                std::process::exit(1);
            }
        }
    }
    // 1. Run checker to find errors
    let errors = run_checker(path, registry);

    if errors.is_empty() {
        if json {
            println!("{}", super::fix_json(serde_json::json!(fixes.iter().map(|f| serde_json::json!({"description": f.description})).collect::<Vec<_>>()), true, 0, serde_json::json!(skipped)));
        } else {
            if fixes.is_empty() { println!("  \u{2713} All checks passed — nothing to fix."); }
            else { println!("  \u{2713} {} fix(es) applied. All checks passed.", fixes.len()); }
        }
        return;
    }

    // 2. Parse program for AST info (face declarations, etc.)
    let program = parse_program(path);
    let mut source = read_source(path);
    let syntax_fix_count = fixes.len();

    // Process errors in reverse span order so earlier fixes don't shift later offsets.
    // Collect fixable actions first, then apply.
    struct FixAction {
        kind: FixKind,
    }
    enum FixKind {
        InsertHandler { cell_name: String, signal_name: String, params: String, ret: String },
        RemoveContradictoryProperty { slot_name: String, property_to_remove: String, cell: Option<String> },
        RemoveDuplicateSignal { signal_name: String, cell: Option<String> },
    }

    let mut actions: Vec<FixAction> = Vec::new();

    // `null` / `None` / `True` … : the checker names the Soma spelling; the
    // identifier's span is exact, so the rewrite is mechanical (applied
    // last-to-first so offsets stay valid)
    let mut idiom_edits: Vec<(usize, usize, &str, String)> = Vec::new();
    for error in &errors {
        if let CheckError::Static { kind: "foreign_idiom", message, span } = error {
            if span.start >= source.chars().count() { continue; }
            let span = crate::ast::Span::new(char_to_byte(&source, span.start), char_to_byte(&source, span.end));
            let word = source[span.start..span.end.min(source.len())].to_string();
            let to = match word.as_str() {
                "null" | "None" | "nil" | "undefined" | "NULL" => "()",
                "True" | "TRUE" => "true",
                "False" | "FALSE" => "false",
                _ => continue,
            };
            if message.starts_with(&format!("'{}'", word)) {
                idiom_edits.push((span.start, span.end, to, word));
            }
        }
    }
    idiom_edits.sort_by(|a, b| b.0.cmp(&a.0));
    idiom_edits.dedup_by(|a, b| a.0 == b.0);
    for (start, end, to, word) in idiom_edits {
        source.replace_range(start..end, to);
        fixes.push(AppliedFix { description: format!("`{}` → `{}`", word, to) });
    }

    for error in &errors {
        match error {
            CheckError::MissingHandler { cell, signal, .. } => {
                // Look up the face declaration to get exact params
                if let Some(cell_def) = find_cell(&program, cell) {
                    if let Some(sig_decl) = find_face_signal(cell_def, signal) {
                        let params = format_params(&sig_decl.params);
                        // a stub of the declared return type, so the fixed
                        // program passes the face-contract check
                        let ret = match sig_decl.return_type.as_ref().map(|t| &t.node) {
                            Some(ast::TypeExpr::Simple(t)) | Some(ast::TypeExpr::Generic { name: t, .. }) => match t.as_str() {
                                "Int" => "0".to_string(),
                                "Float" => "0.0".to_string(),
                                "Bool" => "false".to_string(),
                                "String" => "\"\"".to_string(),
                                "List" => "[]".to_string(),
                                // an empty map fits every `Map<K, V>` (a
                                // "status": "ok" entry broke `Map<String, Int>`)
                                "Map" => "map()".to_string(),
                                "Any" => "map(\"status\", \"ok\")".to_string(),
                                // a sum type: one of its bare variants, else a
                                // stub that says it is one (a Map broke `-> Opt`)
                                other => unit_variant_of(&program, other)
                                    .unwrap_or_else(|| format!("fail(\"not_implemented\", \"{} is a stub written by soma fix\")", signal)),
                            },
                            _ => "map(\"status\", \"ok\")".to_string(),
                        };
                        actions.push(FixAction {
                            kind: FixKind::InsertHandler {
                                cell_name: cell.clone(),
                                signal_name: signal.clone(),
                                params,
                                ret,
                            },
                        });
                    }
                }
            }
            CheckError::PropertyContradiction { slot, a, b, span } => {
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
                        // the cell of the declaration the error points at (a
                        // slot of that name in another cell was edited; a
                        // character offset went stale after earlier edits)
                        cell: cell_at(&program, span.start),
                    },
                });
            }
            CheckError::DuplicateSignal { name, span, .. } => {
                // two `on` handlers: which one is meant is the author's call
                // (the fix deleted a FACE line and left both handlers)
                let is_handler = program.cells.iter().flat_map(|c| c.node.sections.iter())
                    .any(|s| matches!(s.node, ast::Section::OnSignal(_)) && s.span.start == span.start);
                if is_handler {
                    if !json {
                        println!("  \u{2717} skipped: two handlers 'on {}' in one cell — keep one by hand (manual fix required)", name);
                    }
                } else {
                    actions.push(FixAction {
                        kind: FixKind::RemoveDuplicateSignal {
                            signal_name: name.clone(),
                            cell: cell_at(&program, span.start),
                        },
                    });
                }
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
            FixKind::InsertHandler { cell_name, signal_name, params, ret } => {
                if let Some(close_pos) = find_cell_closing_brace(&source, cell_name) {
                    let indent = detect_indent(&source, cell_name);
                    let handler = format!(
                        "\n{indent}on {signal_name}({params}) {{\n{indent}    // TODO: implement\n{indent}    return {ret}\n{indent}}}\n",
                        indent = indent,
                        signal_name = signal_name,
                        params = params,
                        ret = ret,
                    );
                    source.insert_str(close_pos, &handler);
                    fixes.push(AppliedFix {
                        description: format!("added handler: on {}({}) {{ ... }}", signal_name, params),
                    });
                }
            }
            FixKind::RemoveContradictoryProperty { slot_name, property_to_remove, cell } => {
                // Find pattern like `[persistent, ephemeral]` or `[ephemeral, persistent]`
                // and remove the offending property from the bracket list.
                // Strategy: find the slot line and remove the property from its bracket list.
                // searched from the start of its cell, in the text as it is now
                let from_byte = cell.as_ref().and_then(|c| find_cell_decl(&source, c)).map_or(0, |d| d.0);
                let removed = remove_property_from_source(&mut source, slot_name, property_to_remove, from_byte);
                if removed {
                    fixes.push(AppliedFix {
                        description: format!("removed contradictory property: {} on slot '{}'", property_to_remove, slot_name),
                    });
                }
            }
            FixKind::RemoveDuplicateSignal { signal_name, cell } => {
                let from = cell.as_ref().and_then(|c| find_cell_decl(&source, c)).map_or(0, |d| d.0);
                if let Some((start, end)) = find_duplicate_signal_line(&source, from, signal_name) {
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
            let mut doc = super::fix_json(serde_json::json!([]), false, errors.len(), serde_json::json!(skipped));
            doc["note"] = serde_json::json!("errors found but none are auto-fixable");
            println!("{}", doc);
        } else {
            println!("  \u{2717} No auto-fixable errors found. `soma check` refuses the program for:");
            for e in &errors { println!("    error: {}", e.to_string().lines().next().unwrap_or("")); }
        }
        // errors remain: a failing exit, as `soma check` gives
        std::process::exit(1);
    }

    // 4. Write the fixed file back
    std::fs::write(path, &source).unwrap_or_else(|e| {
        eprintln!("error: cannot write '{}': {}", path.display(), e);
        std::process::exit(1);
    });

    // 5. Print what was fixed
    if !json {
        for fix in &fixes[syntax_fix_count..] {
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
        println!("{}", serde_json::to_string_pretty(&super::fix_json(serde_json::json!(fix_list), recheck_errors.is_empty(), recheck_errors.len(), serde_json::json!(skipped))).unwrap());
    } else {
        if recheck_errors.is_empty() {
            println!("  \u{2713} All checks passed.");
        } else {
            println!("  \u{2717} {} error(s) remain after fixes.", recheck_errors.len());
        }
    }
    if !recheck_errors.is_empty() { std::process::exit(1); }
}

/// Remove a specific property from a memory slot's property list in the source.
/// E.g., given slot "items" with `[persistent, ephemeral]`, remove "ephemeral"
/// to produce `[persistent]`.
fn remove_property_from_source(source: &mut String, slot_name: &str, prop: &str, from: usize) -> bool {
    // Find the slot declaration line. Look for the slot name preceded by `[...]`
    // Typical patterns:
    //   items: Map<String, Item> = map() [persistent, ephemeral]
    //   items: List<String> [ephemeral, persistent]
    // We need to find a `[` ... `]` block on the same line as the slot name,
    // where the bracket contains the property.

    // Search for slot name followed eventually by a bracket list containing the property
    let mut search_from = from;
    while let Some(abs_pos) = find_word(source, search_from, slot_name) {
        // the declaration `name:` — not the name inside another word or a use
        if !source[abs_pos + slot_name.len()..].trim_start_matches([' ', '\t']).starts_with(':') {
            search_from = abs_pos + slot_name.len();
            continue;
        }
        // Find the end of this line
        let line_end = source[abs_pos..].find('\n').map(|i| abs_pos + i).unwrap_or(source.len());
        let line = &source[abs_pos..line_end];

        // Find a bracket list in this line
        if let Some(bracket_start) = line.find('[') {
            if let Some(bracket_end) = line[bracket_start..].find(']') {
                let bracket_abs_start = abs_pos + bracket_start;
                let bracket_abs_end = abs_pos + bracket_start + bracket_end + 1;
                let bracket_content = &source[bracket_abs_start + 1..bracket_abs_end - 1];

                if bracket_content.split(',').any(|p| p.trim() == prop) {
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

/// `soma fix --native-idiv`: rewrite every `a / b` on two Ints inside a
/// [native] handler to `idiv(a, b)`. Native `/` used to truncate; it is now
/// 3.5 like everywhere else, so code that relied on the integer quotient
/// must say so. Innermost divisions first, one pass per nesting level.
pub fn cmd_fix_native_idiv(path: &PathBuf, json: bool) {
    let mut total = 0usize;
    for _pass in 0..16 {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {}: {}", path.display(), e);
                std::process::exit(1);
            }
        };
        let program = parse_program(path);
        let mut sites = crate::checker::native::int_division_sites(&program);
        if sites.is_empty() {
            break;
        }
        // smallest first, drop anything overlapping an already chosen site
        sites.sort_by_key(|(l, r)| r.end - l.start);
        // spans count CHARACTERS (the lexer walks a Vec<char>); text
        // surgery needs byte offsets
        let mut byte_at: Vec<usize> = source.char_indices().map(|(b, _)| b).collect();
        byte_at.push(source.len());
        let at = |c: usize| byte_at.get(c).copied().unwrap_or(source.len());
        let mut chosen: Vec<(usize, usize, String)> = Vec::new();
        for (l, r) in sites {
            let Some((start, end, text)) = idiv_rewrite(&source, at(l.start), at(l.end), at(r.start), at(r.end)) else { continue };
            if chosen.iter().any(|(s, e, _)| start < *e && *s < end) {
                continue;
            }
            chosen.push((start, end, text));
        }
        if chosen.is_empty() {
            break;
        }
        chosen.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
        let mut out = source.clone();
        for (start, end, text) in &chosen {
            out.replace_range(*start..*end, text);
        }
        total += chosen.len();
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("error: cannot write {}: {}", path.display(), e);
            std::process::exit(1);
        }
    }
    if json {
        println!("{}", serde_json::json!({
            "file": path.display().to_string(),
            "fixes": if total > 0 { vec![serde_json::json!({"description": format!("{} division(s) rewritten to idiv()", total)})] } else { vec![] },
            "fix_count": total,
            "rewritten": total,
            "passed": true,
        }));
    } else {
        println!("{}: {} division(s) rewritten to idiv()", path.display(), total);
    }
}

/// Replacement for one division given its operand spans. Spans cover the
/// bare operands and ignore parentheses at their edges: for
/// `((2 * n + 1) * m) / (n + 2)` the left span is `2 * n + 1) * m` and the
/// right one `n + 2`. Each operand is first re-balanced — a `)` with no
/// partner pulls in the `(` before the span, a `(` left open pulls in the
/// `)` after it. Whatever parentheses then remain between the operands and
/// the `/` are wrappers: each is matched outward and dropped.
fn idiv_rewrite(src: &str, l_start: usize, l_end: usize, r_start: usize, r_end: usize) -> Option<(usize, usize, String)> {
    if !(l_start <= l_end && l_end <= r_start && r_start <= r_end && r_end <= src.len()) {
        return None;
    }
    let bytes = src.as_bytes();
    // (missing `(` before, missing `)` after) for a piece of text
    let imbalance = |text: &str| -> (usize, usize) {
        let (mut depth, mut min) = (0i64, 0i64);
        for c in text.chars() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    min = min.min(depth);
                }
                _ => {}
            }
        }
        ((-min) as usize, (depth - min) as usize)
    };
    // step left over `n` opening parens (whitespace allowed)
    let eat_opens = |from: usize, n: usize| -> Option<usize> {
        let mut i = from;
        for _ in 0..n {
            loop {
                if i == 0 {
                    return None;
                }
                i -= 1;
                if bytes[i] == b'(' {
                    break;
                }
                if !bytes[i].is_ascii_whitespace() {
                    return None;
                }
            }
        }
        Some(i)
    };
    // step right over `n` closing parens (whitespace allowed)
    let eat_closes = |from: usize, n: usize| -> Option<usize> {
        let mut i = from;
        for _ in 0..n {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b')' {
                return None;
            }
            i += 1;
        }
        Some(i)
    };

    // re-balance each operand
    let (l_need_open, l_need_close) = imbalance(src.get(l_start..l_end)?);
    let l_text_start = eat_opens(l_start, l_need_open)?;
    let l_text_end = eat_closes(l_end, l_need_close)?;
    let (r_need_open, r_need_close) = imbalance(src.get(r_start..r_end)?);
    let r_text_start = eat_opens(r_start, r_need_open)?;
    let r_text_end = eat_closes(r_end, r_need_close)?;
    if l_text_end > r_text_start {
        return None;
    }

    // what is left between them: whitespace, wrapper parens, one `/`
    let between = src.get(l_text_end..r_text_start)?;
    if between.matches('/').count() != 1
        || between.chars().any(|c| !(c.is_whitespace() || c == '(' || c == ')' || c == '/'))
    {
        return None;
    }
    let slash = between.find('/')?;
    let (before, after) = (&between[..slash], &between[slash + 1..]);
    if before.contains('(') || after.contains(')') {
        return None;
    }
    let start = eat_opens(l_text_start, before.matches(')').count())?;
    let end = eat_closes(r_text_end, after.matches('(').count())?;

    let left = strip_outer_parens(src.get(l_text_start..l_text_end)?.trim());
    let right = strip_outer_parens(src.get(r_text_start..r_text_end)?.trim());
    // last line of defense: never write unbalanced text
    if imbalance(left) != (0, 0) || imbalance(right) != (0, 0) {
        return None;
    }
    Some((start, end, format!("idiv({}, {})", left, right)))
}

/// `(lo + hi)` → `lo + hi` when ONE pair of parentheses wraps the whole
/// text (an argument of idiv needs none); `(a) + (b)` is left alone.
fn strip_outer_parens(text: &str) -> &str {
    let t = text.trim();
    if !(t.starts_with('(') && t.ends_with(')')) {
        return t;
    }
    let mut depth = 0i64;
    for (i, c) in t.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && i + 1 < t.len() {
                    return t; // the first paren closes before the end
                }
            }
            _ => {}
        }
    }
    strip_outer_parens(&t[1..t.len() - 1])
}

/// A variant without fields of the sum type `ty`, if it declares one.
fn unit_variant_of(program: &ast::Program, ty: &str) -> Option<String> {
    program.cells.iter().filter(|c| c.node.name == ty).flat_map(|c| c.node.sections.iter()).find_map(|s| match &s.node {
        ast::Section::Variants(vs) => vs.variants.iter().find(|v| matches!(v.node.fields, ast::VariantFields::Unit)).map(|v| v.node.name.clone()),
        _ => None,
    })
}

/// The name of the cell whose source span holds `pos` (a character offset).
fn cell_at(program: &ast::Program, pos: usize) -> Option<String> {
    program.cells.iter().find(|c| c.span.start <= pos && pos <= c.span.end).map(|c| c.node.name.clone())
}

// ── `soma fix --edition 2027` (3.0) ─────────────────────────────────────

/// The program of ONE file (no imports: edits apply to this text only).
fn parse_text(source: &str) -> Option<ast::Program> {
    let tokens = crate::lexer::Lexer::new(source).tokenize().ok()?;
    crate::parser::Parser::new(tokens).parse_program().ok()
}

/// Every handler-like body of a program, interior cells included.
fn bodies(program: &ast::Program) -> Vec<&[ast::Spanned<ast::Statement>]> {
    fn of_cell<'a>(cell: &'a ast::CellDef, out: &mut Vec<&'a [ast::Spanned<ast::Statement>]>) {
        for s in &cell.sections {
            match &s.node {
                ast::Section::OnSignal(h) => out.push(&h.body),
                ast::Section::Every(e) | ast::Section::After(e) => out.push(&e.body),
                ast::Section::Interior(i) => for c in &i.cells { of_cell(&c.node, out); },
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for c in &program.cells { of_cell(&c.node, &mut out); }
    out
}

/// The AST without positions: two texts with the same one are the same program.
fn shape_of(program: &ast::Program) -> String {
    let text = format!("{:?}", program.cells);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Span {") {
        out.push_str(&rest[..i]);
        rest = match rest[i..].find('}') { Some(j) => &rest[i + j + 1..], None => "" };
    }
    out.push_str(rest);
    out
}

/// (char position, text to insert, chars to remove) edits that move a file
/// to edition 2027 without changing what it does:
///  - `a ?? b + c` reads `a ?? (b + c)`: the parentheses are written;
///  - `xs + ys` joining two lists of non-numbers becomes `xs ++ ys`.
fn edition_2027_edits(source: &str, program: &ast::Program) -> (Vec<(usize, String, usize)>, usize, usize) {
    use ast::{BinOp, Expr};
    let chars: Vec<char> = source.chars().collect();
    let mut edits: Vec<(usize, String, usize)> = Vec::new();
    let (mut parens, mut joins) = (0usize, 0usize);
    for body in bodies(program) {
        checker::literals::for_each_call(body, &mut |name, args, _| {
            if name != "_coalesce" || args.len() != 2 { return; }
            let r = &args[1];
            let (rl, rr) = match &r.node {
                Expr::BinaryOp { left, op, right } if !matches!(op, BinOp::And | BinOp::Or) => (left, right),
                Expr::CmpOp { left, right, .. } => (left, right),
                _ => return,
            };
            // already parenthesised: its span covers the parentheses
            if r.span.start != rl.span.start || r.span.end != rr.span.end { return; }
            if edits.iter().any(|(at, _, _)| *at == r.span.start || *at == r.span.end) { return; }
            edits.push((r.span.start, "(".to_string(), 0));
            edits.push((r.span.end, ")".to_string(), 0));
            parens += 1;
        });
    }
    let types = checker::types::Program2::new(program, true);
    for cell in &program.cells {
        for issue in types.check_cell(&cell.node) {
            let Some((l, r)) = issue.join else { continue };
            // the one `+` between the operands (closing / opening parentheses aside)
            let between: Vec<usize> = (l.end..r.start.min(chars.len())).filter(|i| chars[*i] == '+').collect();
            let clean = (l.end..r.start.min(chars.len())).all(|i| chars[i] == '+' || chars[i].is_whitespace() || chars[i] == '(' || chars[i] == ')');
            if let ([at], true) = (between.as_slice(), clean) {
                if !edits.iter().any(|(p, _, _)| p == at) {
                    edits.push((*at, "++".to_string(), 1));
                    joins += 1;
                }
            }
        }
    }
    (edits, parens, joins)
}

/// soma.toml beside the program names the edition.
fn write_edition(dir: &std::path::Path, edition: &str) -> Result<bool, String> {
    let path = dir.join("soma.toml");
    let line = format!("edition = \"{}\"", edition);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            std::fs::write(&path, format!("[package]\n{line}\n")).map_err(|e| format!("cannot write '{}': {}", path.display(), e))?;
            return Ok(true);
        }
    };
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let mut in_package = false;
    let (mut package_at, mut edition_at) = (None, None);
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with('[') { in_package = t == "[package]"; if in_package { package_at = Some(i); } continue; }
        if in_package && t.split('=').next().map_or(false, |k| k.trim() == "edition") { edition_at = Some(i); }
    }
    match (edition_at, package_at) {
        (Some(i), _) => { if lines[i].trim() == line { return Ok(false); } lines[i] = line; }
        (None, Some(i)) => lines.insert(i + 1, line),
        (None, None) => { lines.insert(0, String::new()); lines.insert(0, line); lines.insert(0, "[package]".to_string()); }
    }
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("cannot write '{}': {}", path.display(), e))?;
    Ok(true)
}

/// `soma fix --edition 2027 app.cell`: the mechanical part of moving a
/// program to the edition (what each rewrite does is unchanged), the
/// edition written to soma.toml, then what is left for a person — a value
/// that may be absent, a return that contradicts its face — as check
/// reports it.
pub fn cmd_fix_edition(path: &PathBuf, edition: &str, json: bool, registry: &mut Registry) {
    let fail = |msg: String| -> ! {
        if json { println!("{}", serde_json::json!({"fixes": [], "fix_count": 0, "passed": false, "remaining_errors": 1, "skipped": [], "note": null, "parse_error": null, "error": msg})); }
        else { eprintln!("error: {}", msg); }
        std::process::exit(1)
    };
    if edition != "2027" {
        fail(format!("`soma fix --edition` migrates to \"2027\" (got \"{}\"); the editions are {}", edition,
            crate::pkg::manifest::EDITIONS.iter().map(|e| format!("\"{e}\"")).collect::<Vec<_>>().join(", ")));
    }
    let (mut parens, mut joins) = (0usize, 0usize);
    for _pass in 0..8 {
        let source = std::fs::read_to_string(path).unwrap_or_else(|e| fail(format!("cannot read '{}': {}", path.display(), e)));
        let Some(program) = parse_text(&source) else { fail(format!("{} does not parse — `soma check` it first", path.display())) };
        let (mut edits, p, j) = edition_2027_edits(&source, &program);
        if edits.is_empty() { break; }
        // spans count characters; apply from the end of the text
        edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let mut chars: Vec<char> = source.chars().collect();
        for (at, text, remove) in &edits {
            if *at > chars.len() { continue; }
            let end = (*at + *remove).min(chars.len());
            chars.splice(*at..end, text.chars());
        }
        let rewritten: String = chars.into_iter().collect();
        // the rewrite must be the same program: parentheses are not in the
        // AST at all, and `++` is concat() of the same operands
        let Some(after) = parse_text(&rewritten) else { fail("internal: the rewritten text does not parse — nothing was written".to_string()) };
        if j == 0 && shape_of(&after) != shape_of(&program) {
            fail("internal: the rewritten text is not the same program — nothing was written".to_string());
        }
        std::fs::write(path, rewritten).unwrap_or_else(|e| fail(format!("cannot write '{}': {}", path.display(), e)));
        parens += p;
        joins += j;
    }
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let wrote = write_edition(&dir, edition).unwrap_or_else(|e| fail(e));

    // what is left, under the edition just written
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);
    let manifest = crate::pkg::manifest::Manifest::load(&dir.join("soma.toml")).ok();
    let mut chk = checker::Checker::new(registry);
    chk.manifest = manifest.as_ref();
    chk.source = Some((file_str.clone(), source.clone()));
    chk.check(&program);

    let mut fixes: Vec<String> = Vec::new();
    if parens > 0 { fixes.push(format!("{parens} `a ?? b op c` written with its parentheses: `a ?? (b op c)`")); }
    if joins > 0 { fixes.push(format!("{joins} list join(s) `xs + ys` rewritten `xs ++ ys`")); }
    if wrote { fixes.push(format!("soma.toml: edition = \"{edition}\"")); }
    if json {
        let mut doc: serde_json::Value = serde_json::from_str(&chk.report_json()).unwrap_or(serde_json::json!({}));
        doc["fixes"] = serde_json::json!(fixes.iter().map(|d| serde_json::json!({"description": d})).collect::<Vec<_>>());
        doc["fix_count"] = serde_json::json!(fixes.len());
        doc["remaining_errors"] = serde_json::json!(chk.errors.len());
        doc["edition"] = serde_json::json!(edition);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    } else {
        for f in &fixes { println!("  \u{2713} {}", f); }
        if fixes.is_empty() { println!("  nothing to rewrite"); }
        // the one change of 2027 no rewrite can make: it is a run-time answer
        let mut status_reads = 0usize;
        for cell in crate::checker::names::collect_cells(&program) {
            for section in &cell.sections {
                let body = match &section.node {
                    ast::Section::OnSignal(on) => &on.body,
                    ast::Section::Every(e) | ast::Section::After(e) => &e.body,
                    _ => continue,
                };
                crate::checker::literals::for_each_call(body, &mut |name, _, _| if name == "get_status" { status_reads += 1; });
            }
        }
        if edition >= "2027" && status_reads > 0 {
            println!("  ! {status_reads} call(s) of get_status(id): in edition 2027 an id that was never transitioned answers () — no longer the initial state. Read each one: `get_status(id) == \"<initial>\"` is false for a new id (test `get_status(id) == ()`, or transition first)");
        }
        if chk.has_errors() {
            println!("\nleft for you under edition {edition} ({} error(s)):", chk.errors.len());
        }
        print!("{}", chk.report());
    }
    if chk.has_errors() { std::process::exit(1); }
}
