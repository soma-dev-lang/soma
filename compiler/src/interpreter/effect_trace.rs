//! Effect trace — what handlers actually did, for checking the frames.
//!
//! With `SOMA_EFFECT_TRACE=<file>` set, the interpreter appends one line per
//! effect: `stack \t kind \t what`, `stack` being the handlers running
//! (`Cell.h>Cell.g`, outermost first; empty outside any handler).
//!
//!   call     Cell.handler     a handler is entered
//!   read     slot             a memory slot is read
//!   write    slot             a memory slot is written
//!   builtin  name             a builtin with an effect is called
//!   move     state            transition() toward that state
//!   edge     from>to          …and the edge it is about to take
//!   mread    -                a state machine is read (get_status …)
//!   emit     signal
//!
//! `tools/frame_oracle.py` runs the test cells of every program with the
//! trace on and checks that each observed effect is inside the frame
//! `soma skeleton` infers for the signal that made it: the static frames
//! are sound exactly when nothing observed falls outside them.
//!
//! Off (the variable unset), the cost is one atomic load per hook.

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

static STATE: AtomicU8 = AtomicU8::new(0); // 0 unknown, 1 off, 2 on
static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

thread_local! {
    static STACK: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static QUIET: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// While it lives, nothing is logged: the runtime is evaluating the
/// STRUCTURE (an invariant, a guard, an edge's effect block) — what those
/// read is the skeleton's doing, not the handler's.
pub struct Quiet(bool);

pub fn quiet() -> Quiet {
    if !on() { return Quiet(false); }
    QUIET.with(|q| q.set(q.get() + 1));
    Quiet(true)
}

impl Drop for Quiet {
    fn drop(&mut self) {
        if self.0 { QUIET.with(|q| q.set(q.get().saturating_sub(1))); }
    }
}

pub fn on() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let file = std::env::var("SOMA_EFFECT_TRACE").ok().filter(|p| !p.is_empty())
                .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
            let on = file.is_some();
            *FILE.lock().unwrap_or_else(|e| e.into_inner()) = file;
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

pub fn log(kind: &str, what: &str) {
    if !on() || QUIET.with(|q| q.get()) > 0 { return; }
    let stack = STACK.with(|s| s.borrow().join(">"));
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(f, "{}\t{}\t{}", stack, kind, what);
    }
}

/// Logged even while quiet: the edge a transition is about to take
/// (`from>to`) — the source is only known inside the transition.
pub fn log_edge(from: &str, to: &str) {
    if !on() { return; }
    let stack = STACK.with(|s| s.borrow().join(">"));
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = writeln!(f, "{}\tedge\t{}>{}", stack, from, to);
    }
}

/// A handler is entered: logged as a call of the one running, then pushed.
pub fn enter(cell: &str, handler: &str) {
    if !on() { return; }
    let name = format!("{}.{}", cell, handler);
    log("call", &name);
    STACK.with(|s| s.borrow_mut().push(name));
}

pub fn leave() {
    if !on() { return; }
    STACK.with(|s| { s.borrow_mut().pop(); });
}
