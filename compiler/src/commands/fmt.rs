//! `soma fmt` (3.0): the canonical layout of a .cell file. No options.
//!
//! What it settles: indentation (4 spaces per open `{` `[` `(`), trailing
//! whitespace, tabs, runs of blank lines (one at most, none after an opening
//! brace or before a closing one), the final newline. What it leaves alone:
//! line breaks (a newline separates statements in Soma), the spacing inside
//! a line, comments, and every character of a multi-line string.
//!
//! The formatted text must lex to exactly the token sequence of the
//! original — same tokens, same line breaks between them — or nothing is
//! written.

use std::path::{Path, PathBuf};

use crate::lexer;

#[derive(Clone, Copy, PartialEq)]
enum State { Code, LineComment, BlockComment, Str, RawStr }

/// The canonical layout of `source`.
pub fn format_source(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut out: Vec<String> = Vec::new();
    // lines that start inside a string or block comment: content, not layout
    let mut content: Vec<bool> = Vec::new();
    let mut state = State::Code;
    let mut depth: usize = 0;
    let mut after_signal = false;
    // the previous line is a frame clause that ends with `,`: this one continues it
    let mut clause_open = false;
    for raw in source.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let starts_inside = matches!(state, State::Str | State::RawStr | State::BlockComment);
        // scan the line: where it leaves us, and how the depth moves
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        let depth_before = depth;
        // closers that lead the line (before any other code) dedent it
        let mut leading_closers = 0usize;
        let mut seen_code = false;
        if state == State::LineComment { state = State::Code; }
        while i < chars.len() {
            let c = chars[i];
            match state {
                State::Code => {
                    if c == '/' && chars.get(i + 1) == Some(&'/') { state = State::LineComment; break; }
                    if c == '/' && chars.get(i + 1) == Some(&'*') { state = State::BlockComment; i += 2; seen_code = true; continue; }
                    if c == '"' {
                        if chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"') { state = State::RawStr; i += 3; } else { state = State::Str; i += 1; }
                        seen_code = true;
                        continue;
                    }
                    match c {
                        '{' | '[' | '(' => { depth += 1; seen_code = true; }
                        '}' | ']' | ')' => {
                            if !seen_code && !starts_inside { leading_closers += 1; }
                            depth = depth.saturating_sub(1);
                        }
                        c if c.is_whitespace() => {}
                        _ => seen_code = true,
                    }
                    i += 1;
                }
                State::LineComment => break,
                State::BlockComment => {
                    if c == '*' && chars.get(i + 1) == Some(&'/') { state = State::Code; i += 2; } else { i += 1; }
                }
                State::Str => {
                    if c == '\\' { i += 2; continue; }
                    if c == '"' { state = State::Code; }
                    i += 1;
                }
                State::RawStr => {
                    if c == '"' && chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"') { state = State::Code; i += 3; } else { i += 1; }
                }
            }
        }
        if state == State::LineComment { state = State::Code; }
        if starts_inside {
            // inside a string or a block comment: every character is content
            out.push(raw.to_string());
            content.push(true);
            continue;
        }
        let text = raw.trim();
        if text.is_empty() {
            out.push(String::new());
            content.push(false);
            continue;
        }
        // a face contract clause hangs under its signal:
        //     signal debit(amount: Int) -> Int
        //         requires amount > 0
        //         writes account  calls balance,
        //             Audit.record
        let first = text.split_whitespace().next().unwrap_or("");
        let continued = after_signal && clause_open;
        let clause = continued || (after_signal && matches!(first, "requires" | "ensures" | "reads" | "writes" | "calls" | "moves" | "emits"));
        if !text.starts_with("//") {
            after_signal = clause || matches!(first, "signal" | "tool");
            clause_open = clause && text.ends_with(',');
        }
        let indent = depth_before.saturating_sub(leading_closers) + usize::from(clause) + usize::from(continued);
        // a line that ends inside a multi-line string keeps its trailing spaces
        let body = if matches!(state, State::Str | State::RawStr) { raw.trim_start() } else { text };
        out.push(format!("{}{}", "    ".repeat(indent), body));
        content.push(false);
    }
    // blank lines: one at most; none after an opener, before a closer, at either end
    let mut lines: Vec<String> = Vec::new();
    for (idx, line) in out.iter().enumerate() {
        if line.is_empty() && !content[idx] {
            let prev_opens = lines.last().map_or(true, |p: &String| p.is_empty() || p.trim_end().ends_with(['{', '[', '(']));
            let next_closes = out[idx + 1..].iter().find(|l| !l.is_empty()).map_or(true, |n| n.trim_start().starts_with(['}', ']', ')']));
            if prev_opens || next_closes { continue; }
        }
        lines.push(line.clone());
    }
    while lines.last().map_or(false, |l| l.is_empty()) { lines.pop(); }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Tokens and the line breaks between them: what the layout may not change.
fn shape(source: &str) -> Option<Vec<(lexer::Token, bool)>> {
    let tokens = lexer::Lexer::new(source).tokenize().ok()?;
    Some(tokens.into_iter().map(|t| (t.token, t.newline_before)).collect())
}

/// Format, refusing a result that is not the same program. `None`: the file
/// does not lex (it is left alone; `soma check` says why).
pub fn format_checked(source: &str) -> Option<Result<String, String>> {
    let before = shape(source)?;
    let formatted = format_source(source);
    // a blank line inside a multi-line string is content the blank-line rule
    // must not touch: compare tokens, values included
    let same = shape(&formatted).map_or(false, |after| after == before);
    if !same {
        return Some(Err("the formatted text would not be the same program (a multi-line string or comment the formatter does not lay out) — left unchanged".to_string()));
    }
    Some(Ok(formatted))
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(rd) = std::fs::read_dir(path) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() {
                if name.starts_with('.') || name == "node_modules" || name == "target" { continue; }
                collect(&p, out);
            } else if name.ends_with(".cell") {
                out.push(p);
            }
        }
    } else {
        out.push(path.to_path_buf());
    }
}

/// `soma fmt [--check] <paths…>`: rewrite in place, or with `--check` list
/// the files that are not canonical and exit 1.
pub fn cmd_fmt(paths: &[PathBuf], check: bool, json: bool) {
    let mut files = Vec::new();
    for p in paths { collect(p, &mut files); }
    if files.is_empty() {
        eprintln!("error: no .cell file to format — `soma fmt app.cell` or `soma fmt .`");
        std::process::exit(1);
    }
    let (mut changed, mut skipped, mut failed) = (Vec::new(), Vec::new(), false);
    for f in &files {
        let source = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => { eprintln!("error: cannot read '{}': {}", f.display(), e); failed = true; continue; }
        };
        match format_checked(&source) {
            None => skipped.push((f.display().to_string(), "does not lex — run `soma check`".to_string())),
            Some(Err(why)) => skipped.push((f.display().to_string(), why)),
            Some(Ok(text)) => {
                if text == source { continue; }
                changed.push(f.display().to_string());
                if !check {
                    if let Err(e) = std::fs::write(f, text) {
                        eprintln!("error: cannot write '{}': {}", f.display(), e);
                        failed = true;
                    }
                }
            }
        }
    }
    if json {
        println!("{}", serde_json::json!({
            "files": files.len(),
            "changed": changed,
            "skipped": skipped.iter().map(|(f, why)| serde_json::json!({"file": f, "reason": why})).collect::<Vec<_>>(),
            "check": check,
            "ok": !failed && !(check && !changed.is_empty()),
        }));
    } else {
        for f in &changed {
            println!("{} {}", if check { "would format" } else { "formatted" }, f);
        }
        for (f, why) in &skipped {
            eprintln!("skipped {f}: {why}");
        }
        let unchanged = files.len() - changed.len() - skipped.len();
        println!("{} file(s): {} {}, {} already canonical{}", files.len(), changed.len(),
            if check { "to format" } else { "formatted" }, unchanged,
            if skipped.is_empty() { String::new() } else { format!(", {} skipped", skipped.len()) });
    }
    if failed || (check && !changed.is_empty()) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_by_open_brackets_and_is_idempotent() {
        let src = "cell A {\n  on f(x: Int) {\n\t\tlet m = map(\n\"a\", 1,\n  \"b\", 2\n)\n      if x > 1 {\n return m\n    }   \n\n\n\n    return [\n1,\n2\n]\n  }\n\n}\n\n\n";
        let want = "cell A {\n    on f(x: Int) {\n        let m = map(\n            \"a\", 1,\n            \"b\", 2\n        )\n        if x > 1 {\n            return m\n        }\n\n        return [\n            1,\n            2\n        ]\n    }\n}\n";
        let got = format_source(src);
        assert_eq!(got, want);
        assert_eq!(format_source(&got), got);
        assert!(matches!(format_checked(src), Some(Ok(_))));
    }

    #[test]
    fn strings_and_comments_are_content() {
        let src = "cell A {\n  // {{{ not code\n  on f() {\n    let s = \"a { b\"   \n    let t = \"\"\"\n  raw {\n\n   text\"\"\"\n    /* [ block\n       comment */\n    return s\n  }\n}\n";
        let got = format_source(src);
        assert!(got.contains("\n  raw {\n\n   text\"\"\"\n"), "{got}");
        assert!(got.contains("\n       comment */\n"), "{got}");
        assert!(got.contains("\n        return s\n    }\n}\n"), "{got}");
        assert_eq!(format_source(&got), got);
        assert!(matches!(format_checked(src), Some(Ok(_))), "token sequence changed");
    }
}
