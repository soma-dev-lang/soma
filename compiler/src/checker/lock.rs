//! skeleton.lock — the structure a program is held to
//! (SKELETON_DESIGN.md, "Freezing").
//!
//! `soma skeleton app.cell --freeze` writes, beside the program, what its
//! skeletons are: for each one a hash and its text. From then on every
//! command that loads the program refuses it when a skeleton differs from
//! the lock — changed, added or removed — and says what changed.
//!
//! What a skeleton's entry covers is everything that can write one of its
//! slots or move one of its machines:
//!   - the tokens of its declaration (so comments and layout are free):
//!     memory, invariants, machines, the face and every frame;
//!   - the signals of OTHER cells that move one of its machines (a plain
//!     cell added after the freeze could move the program's only machine);
//!   - interior skeletons are entries like the others.
//! One more entry holds the `[verify…]` tables of soma.toml: the theorems
//! the design is held to are part of it.
//!
//! The lock is found by directory and applied by skeleton NAME: a copy of
//! the program under another file name is held to the same lock. A lock
//! that cannot be read is an error, never "no lock".
//!
//! Changing the design is still possible: `--amend` rewrites the lock and
//! prints the difference. The point is that it is a separate act with its
//! own name: writing flesh cannot change the structure in passing, and a
//! harness can grant one without the other.

use crate::ast::*;
use std::path::{Path, PathBuf};

pub const LOCK_FILE: &str = "skeleton.lock";
/// The entry that holds soma.toml's `[verify…]` tables.
pub const VERIFY_ENTRY: &str = "soma.toml [verify]";
/// In an entry's text: `//~ signal <digest of its comment>`.
const NOTE_MARK: &str = "//~ ";

/// The signals whose comment is not the one the lock recorded, as
/// `Cell.signal` — for a warning: a comment is the brief a body is written
/// from, and it is no part of the frozen structure.
pub fn notes_changed(program: &Program, file: &Path, main_source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let now = fingerprint(program, file, main_source);
    for lock_path in lock_paths(program, file) {
        let Ok(Some(lock)) = read(&lock_path) else { continue };
        for old in &lock.entries {
            let Some(new) = now.iter().find(|e| e.name == old.name) else { continue };
            let notes = |t: &str| -> Vec<(String, String)> { t.lines().filter_map(|l| l.trim().strip_prefix(NOTE_MARK))
                .filter_map(|l| l.split_once(' ').map(|(n, d)| (n.to_string(), d.to_string()))).collect() };
            let (was, is) = (notes(&old.text), notes(&new.text));
            for (name, digest) in &was {
                if is.iter().any(|(n, d)| n == name && d != digest) {
                    let who = format!("{}.{}", old.name, name);
                    if !out.contains(&who) { out.push(who); }
                }
            }
        }
    }
    out
}

pub struct Entry {
    pub name: String,
    /// The declaration, the types it names, and who moves its machines from outside.
    pub hash: String,
    /// The declaration and the types it names, alone: what a program that
    /// merely USES the skeleton is held to (who else moves its machine is
    /// the business of the lock of the program those cells are in).
    pub core: String,
    pub text: String,
}

pub struct Lock {
    /// The program the lock was written for (a file name).
    pub file: String,
    pub entries: Vec<Entry>,
}

pub fn path_for(program: &Path) -> PathBuf {
    program.parent().unwrap_or(Path::new("")).join(LOCK_FILE)
}

fn sha(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// The text a span covers (spans count characters), in the main file or
/// in the imported file it belongs to.
fn span_text(span: Span, main: &str) -> Option<String> {
    let (text, start, end): (std::borrow::Cow<str>, usize, usize) = match crate::interpreter::resolve_import_pos(span.start) {
        Some((_, text, local)) => (std::borrow::Cow::Owned(text.to_string()), local, local + (span.end - span.start)),
        None => (std::borrow::Cow::Borrowed(main), span.start, span.end),
    };
    if start > end { return None; }
    let out: String = text.chars().skip(start).take(end - start).collect();
    if out.chars().count() == end - start { Some(out) } else { None }
}

/// The tokens of a piece of source, as text: layout and comments gone.
fn tokens_of(source: &str) -> String {
    match crate::lexer::Lexer::new(source).tokenize() {
        Ok(tokens) => tokens.iter().map(|t| format!("{:?}\u{1f}", t.token)).collect(),
        Err(_) => source.to_string(),
    }
}

fn skeleton_cells(program: &Program) -> Vec<&Spanned<CellDef>> {
    fn rec<'a>(cells: &'a [Spanned<CellDef>], out: &mut Vec<&'a Spanned<CellDef>>) {
        for c in cells {
            if c.node.skeleton { out.push(c); }
            for s in &c.node.sections {
                if let Section::Interior(i) = &s.node { rec(&i.cells, out); }
            }
        }
    }
    let mut out = Vec::new();
    rec(&program.cells, &mut out);
    out
}

/// The `verify` table of the soma.toml beside the program, as TOML reads
/// it (a dotted key, an inline table, another layout are the same table; a
/// `[verify…]` line inside a string is not one), in a canonical text.
fn verify_tables(program_path: &Path) -> Option<String> {
    // no manifest: no theorem, and the default edition
    let Ok(text) = std::fs::read_to_string(crate::project::manifest(program_path)) else {
        return Some("package.edition = \"2026\"\n".to_string());
    };
    let doc: toml::Value = match text.parse() {
        Ok(v) => v,
        // a manifest that does not parse: what it says is not known — the
        // raw text stands for it, so that it cannot match a frozen table
        Err(_) => return Some(format!("(soma.toml does not parse)\n{}", text)),
    };
    fn render(v: &toml::Value, path: &str, out: &mut Vec<String>) {
        match v {
            toml::Value::Table(t) => {
                let mut keys: Vec<&String> = t.keys().collect();
                keys.sort();
                for k in keys { render(&t[k], &format!("{}.{}", path, k), out); }
            }
            other => out.push(format!("{} = {}", path, other)),
        }
    }
    let mut lines = Vec::new();
    if let Some(table) = doc.get("verify") { render(table, "verify", &mut lines); }
    // the edition decides what the same source means (what `get_status`
    // answers for a fresh id): it is frozen with the theorems
    let edition = doc.get("package").and_then(|p| p.get("edition")).and_then(|e| e.as_str()).unwrap_or("2026");
    lines.push(format!("package.edition = \"{}\"", edition));
    Some(lines.join("\n") + "\n")
}

/// The skeletons of the program (interior ones included) and the verify
/// tables, as the lock records them.
pub fn fingerprint(program: &Program, program_path: &Path, main_source: &str) -> Vec<Entry> {
    let skeletons = super::frame::build(program);
    let mut out = Vec::new();
    for cell in skeleton_cells(program) {
        let Some(sk) = skeletons.iter().find(|sk| sk.cell.name == cell.node.name && sk.cell.skeleton) else { continue };
        let mut text = crate::commands::skeleton::render_frozen(sk);
        // who else moves its machines
        let mut movers: Vec<String> = Vec::new();
        for other in skeletons.iter().filter(|o| o.cell.name != cell.node.name) {
            let frames = other.bones.iter().map(|b| (format!("{}.{}", other.cell.name, b.name), &b.frame))
                .chain(other.schedules.iter().map(|s| (format!("{}: {}", other.cell.name, s.label), &s.frame)));
            for (who, f) in frames {
                let edges: Vec<String> = f.moves.iter()
                    .filter(|(label, _, _)| label.split_once('.').map_or(false, |(c, _)| c == cell.node.name))
                    .map(|(label, from, to)| format!("{}: {} -> {}", label, from, to)).collect();
                if !edges.is_empty() { movers.push(format!("    // moved from outside by {} — {}\n", who, edges.join(", "))); }
            }
        }
        movers.sort();
        if !movers.is_empty() {
            let close = text.trim_end().rfind('}').unwrap_or(text.len());
            text = format!("{}{}{}", &text[..close], movers.concat(), &text[close..]);
        }
        // the tokens of the declaration; its text when the source is out of reach
        let source = span_text(cell.span, main_source);
        let declared = source.as_deref().map(tokens_of).unwrap_or_else(|| text.clone());
        // the types it names (a signature, a slot, a typed machine): the
        // brief presents them as part of the contract
        let mut types: Vec<String> = crate::commands::skeleton::types_closure(program, &[source.clone().unwrap_or_else(|| text.clone())])
            .into_iter().map(|t| format!("    // type {}\n", t)).collect();
        types.sort();
        if !types.is_empty() {
            let close = text.trim_end().rfind('}').unwrap_or(text.len());
            text = format!("{}{}{}", &text[..close], types.concat(), &text[close..]);
        }
        let hash = sha(&[&declared, &movers.concat(), &types.concat()]);
        let core = sha(&[&declared, &types.concat()]);
        // the comment of each signal — its brief — by a digest: not part of
        // the structure (a retouched sentence is no amendment), but a change
        // of it is said (`notes_changed`)
        let mut notes: Vec<String> = Vec::new();
        for section in &cell.node.sections {
            let Section::Face(face) = &section.node else { continue };
            for d in &face.declarations {
                let name = match &d.node { FaceDecl::Signal(sig) => &sig.name, FaceDecl::Tool(t) => &t.name, _ => continue };
                let words = crate::commands::skeleton::intent_of(d.span, main_source, true).join(" ");
                let words: String = words.split_whitespace().collect::<Vec<_>>().join(" ");
                notes.push(format!("    {}{} {}\n", NOTE_MARK, name, &sha(&[&words])[..16]));
            }
        }
        if !notes.is_empty() {
            let close = text.trim_end().rfind('}').unwrap_or(text.len());
            text = format!("{}{}{}", &text[..close], notes.concat(), &text[close..]);
        }
        out.push(Entry { name: cell.node.name.clone(), hash, core, text });
    }
    if !out.is_empty() {
        if let Some(tables) = verify_tables(program_path) {
            out.push(Entry { name: VERIFY_ENTRY.to_string(), hash: sha(&[&tables]), core: sha(&[&tables]), text: tables });
        }
    }
    out
}

pub fn render(file: &str, entries: &[Entry]) -> String {
    let mut body = format!("file {file}\n");
    for e in entries {
        body.push_str(&format!("\n== {} sha256:{} core:{}\n{}", e.name, e.hash, e.core, e.text));
        if !e.text.ends_with('\n') { body.push('\n'); }
    }
    format!("# skeleton.lock — the structure `soma check` holds {file} to.\n# Written by `soma skeleton {file} --freeze`; changed on purpose with `--amend`. Do not edit.\n{body}\nchecksum sha256:{}\n", sha(&[body.trim_end()]))
}

/// The lock at `path`: `Ok(None)` when there is none, `Err` when there is
/// one that cannot be trusted (unreadable, cut short, edited by hand).
pub fn read(path: &Path) -> Result<Option<Lock>, String> {
    if !path.exists() { return Ok(None); }
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot be read ({e})"))?;
    let body: String = text.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n");
    let Some((body, sum)) = body.rsplit_once("\nchecksum sha256:") else {
        return Err("has no checksum line: it was cut short or edited by hand".to_string());
    };
    if sum.trim() != sha(&[body.trim_end()]) {
        return Err("does not match its checksum: it was edited by hand or damaged".to_string());
    }
    let mut file = String::new();
    let mut entries: Vec<Entry> = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("== ") {
            let (name, hashes) = rest.rsplit_once(" sha256:").ok_or_else(|| format!("has a malformed entry line: `{line}`"))?;
            let (hash, core) = hashes.trim().split_once(" core:").ok_or_else(|| format!("has an entry line of an older format: `{line}`"))?;
            entries.push(Entry { name: name.to_string(), hash: hash.to_string(), core: core.to_string(), text: String::new() });
        } else if let Some(e) = entries.last_mut() {
            e.text.push_str(line);
            e.text.push('\n');
        } else if let Some(f) = line.strip_prefix("file ") {
            file = f.trim().to_string();
        }
    }
    if file.is_empty() { return Err("names no program (`file …` line missing)".to_string()); }
    for e in entries.iter_mut() { e.text = format!("{}\n", e.text.trim_end()); }
    Ok(Some(Lock { file, entries }))
}

/// The signal (or slot, state…) a line of an entry's text is about.
fn subject(line: &str) -> String {
    let t = line.trim();
    for kw in ["signal ", "tool "] {
        if let Some(rest) = t.strip_prefix(kw) {
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            return format!("{}{}", kw, name);
        }
    }
    String::new()
}

/// Lines of `a` that `b` lacks, counting repeats (two signals may carry
/// the same clause line).
fn minus<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<&'a str> {
    let mut rest: Vec<&str> = b.to_vec();
    a.iter().copied().filter(|l| match rest.iter().position(|x| x == l) {
        Some(i) => { rest.remove(i); false }
        None => true,
    }).collect()
}

/// What differs between a lock and the program, one block per entry:
/// lines of the lock that are gone (`-`) and lines that are new (`+`),
/// each signal named. `whole` — the program is the one the lock was
/// written for: entries missing on either side count too.
pub fn compare(lock: &Lock, now: &[Entry], whole: bool) -> Vec<String> {
    let mut out = Vec::new();
    let what = |name: &str| if name == VERIFY_ENTRY { "the `[verify]` tables of soma.toml (or its edition) differ".to_string() } else { format!("skeleton `{}` differs", name) };
    for old in &lock.entries {
        match now.iter().find(|e| e.name == old.name) {
            None if whole && old.name == VERIFY_ENTRY => out.push(format!("the `[verify]` tables and the edition of soma.toml are in {} and no longer in soma.toml", LOCK_FILE)),
            None if whole => out.push(format!("skeleton `{}` is in {} and no longer in the program", old.name, LOCK_FILE)),
            None => {}
            // (a program that is not the lock's own: outside movers are not its lock's to judge)
            Some(new) if (whole && new.hash != old.hash) || (!whole && new.core != old.core) => {
                let outside = |l: &&str| (whole || !l.trim_start().starts_with("// moved from outside by")) && !l.trim_start().starts_with(NOTE_MARK);
                let old_lines: Vec<&str> = old.text.lines().filter(outside).collect();
                let new_lines: Vec<&str> = new.text.lines().filter(outside).collect();
                let mut block = format!("{} from {}:", what(&old.name), LOCK_FILE);
                let gone = minus(&old_lines, &new_lines);
                let added = minus(&new_lines, &old_lines);
                for l in &gone { block.push_str(&format!("\n    - {}", l.trim())); }
                for l in &added { block.push_str(&format!("\n    + {}", l.trim())); }
                let mut signals: Vec<String> = gone.iter().chain(added.iter()).map(|l| subject(l)).filter(|s| !s.is_empty()).collect();
                signals.dedup();
                if !signals.is_empty() && old.name != VERIFY_ENTRY {
                    block.push_str(&format!("\n    (in `{}`: {})", old.name, signals.join(", ")));
                }
                if gone.is_empty() && added.is_empty() {
                    block.push_str("\n    (in a part this summary does not print: the order of its lines, a schedule's body, a `scale` or `runtime` section, a tool's description)");
                }
                out.push(block);
            }
            Some(_) => {}
        }
    }
    if whole {
        for new in now.iter().filter(|e| !lock.entries.iter().any(|o| o.name == e.name)) {
            out.push(if new.name == VERIFY_ENTRY {
                format!("soma.toml has `[verify]` tables and {} has none: they were added after the freeze", LOCK_FILE)
            } else {
                format!("skeleton `{}` is not in {}: it was added after the freeze", new.name, LOCK_FILE)
            });
        }
    }
    out
}

/// Where a lock that holds this program may be: beside the entry file and
/// beside every file that declares one of its skeletons, and up from each
/// (the lock of a project is beside its `app.cell`, its skeletons may be in
/// `lib/`). Real paths: a directory of links is the directory it links to.
fn lock_paths(program: &Program, file: &Path) -> Vec<PathBuf> {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut files: Vec<PathBuf> = vec![real(file)];
    for cell in skeleton_cells(program) {
        if let Some((f, _, _)) = crate::interpreter::resolve_import_pos(cell.span.start) { files.push(real(Path::new(&f))); }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for f in files {
        let mut dir = f.parent().map(|d| d.to_path_buf());
        // up to the root of the file system (a bound only against a loop)
        for _ in 0..256 {
            let Some(d) = dir else { break };
            let lock = d.join(LOCK_FILE);
            if lock.exists() && !out.contains(&lock) { out.push(lock); }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    out
}

/// The errors a command reports for a program that a lock holds.
pub fn check(program: &Program, file: &Path, main_source: &str) -> Vec<String> {
    let real_file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let file_name = real_file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut out = Vec::new();
    let mut now: Option<Vec<Entry>> = None;
    for lock_path in lock_paths(program, file) {
        let lock = match read(&lock_path) {
            Ok(None) => continue,
            Ok(Some(lock)) => lock,
            Err(why) => {
                out.push(format!(
                    "{} {} — a lock that cannot be read protects nothing, so nothing runs: restore it (version control), or write a new one on purpose with `soma skeleton {} --amend`",
                    lock_path.display(), why, file.display()));
                continue;
            }
        };
        let entries = now.get_or_insert_with(|| fingerprint(program, file, main_source));
        // any program is held to a lock for the skeletons it names; the
        // program the lock was written for, for all of them — and for the
        // `[verify]` tables, which are those of ITS directory
        let whole = lock.file == file_name && lock_path.parent() == real_file.parent();
        // (another entry file of the same directory reads the same soma.toml)
        let same_dir = lock_path.parent() == real_file.parent();
        let mine: Vec<&Entry> = entries.iter().filter(|e| same_dir || e.name != VERIFY_ENTRY).collect();
        let held = Lock { file: lock.file.clone(), entries: lock.entries.into_iter().filter(|e| same_dir || e.name != VERIFY_ENTRY).collect() };
        let owned: Vec<Entry> = mine.into_iter().map(|e| Entry { name: e.name.clone(), hash: e.hash.clone(), core: e.core.clone(), text: e.text.clone() }).collect();
        let mut diffs = compare(&held, &owned, whole);
        if !whole && held.entries.iter().any(|e| e.name == VERIFY_ENTRY) && !owned.iter().any(|e| e.name == VERIFY_ENTRY) {
            diffs.push(format!("the `[verify]` tables and the edition of soma.toml are in {} and no longer in soma.toml", LOCK_FILE));
        }
        out.extend(diffs.into_iter()
            .map(|d| format!("{d}\n  the skeleton is frozen: restore it, or accept the change on purpose with `soma skeleton {} --amend`", lock_path.with_file_name(&held.file).display())));
    }
    out
}

/// For the commands that load a program without running the whole checker
/// (`replay`, `deploy`, `serve --no-check`): the lock, the flesh and the
/// frames still hold. Prints what is wrong; false when something is.
pub fn structure_holds(program: &Program, file: &Path, main_source: &str) -> bool {
    let mut errors: Vec<String> = check(program, file, main_source);
    errors.extend(super::flesh::check(program).into_iter().map(|(m, _)| m));
    if !skeleton_cells(program).is_empty() {
        errors.extend(super::conform::check(program).0.into_iter().map(|f| f.message));
    }
    for e in &errors { eprintln!("error: {}", e); }
    errors.is_empty()
}
