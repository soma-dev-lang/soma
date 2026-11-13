pub mod storage;
pub mod cluster;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;

use crate::ast::*;
use crate::interpreter::{self, Value};
use storage::{StorageBackend, resolve_backend};

/// A live cell instance with its state and handlers
#[derive(Clone)]
pub struct CellInstance {
    pub name: String,
    /// Qualified path (`Sys.Worker` for an interior cell): the prefix of
    /// its slot files and of its runtime tables
    pub path: String,
    pub def: CellDef,
    /// Memory slots backed by storage
    pub memory: HashMap<String, Arc<dyn StorageBackend>>,
    /// Child cell instances, in declaration order (deterministic dispatch
    /// and display — HashMap order made `dump_state` and main-cell selection
    /// vary between runs)
    pub children: IndexMap<String, CellInstance>,
}

/// The Soma runtime: instantiates cells, wires signals, executes handlers
pub struct Runtime {
    /// Top-level cell instances, in declaration order
    pub cells: IndexMap<String, CellInstance>,
    /// Signal log for debugging
    pub signal_log: Vec<String>,
    /// All cell definitions (for the interpreter)
    pub program: Program,
    /// State-machine instances of the cells run here (`__sm_<Cell>_<m>`),
    /// kept across handler runs: every run builds a fresh interpreter
    machine_storage: HashMap<String, Arc<dyn StorageBackend>>,
    /// The runtime tables of each interior cell, by its qualified path:
    /// machine instances, next_id() counters, remember() memory — keyed as
    /// the interpreter looks them up (`__sm_Worker_flow`, `Worker.__counters`)
    /// but stored under the qualified name (`Sys.Worker__sm_flow`). Under
    /// the bare name they were shared with every same-named cell, top-level
    /// or in another program of the directory.
    scope_tables: HashMap<String, HashMap<String, Arc<dyn StorageBackend>>>,
    /// names of the program's top-level cells
    top_names: HashSet<String>,
}

fn exec_error_text(e: interpreter::ExecError) -> String {
    match e {
        interpreter::ExecError::Runtime(e) => e.to_string(),
        interpreter::ExecError::Return(_) => "`return` outside of a handler".to_string(),
        interpreter::ExecError::Break => "break outside of loop".to_string(),
        interpreter::ExecError::Continue => "continue outside of loop".to_string(),
    }
}

impl Runtime {
    /// Create a runtime from a parsed program
    pub fn new(program: Program) -> Self {
        Self::with_registry(program, None)
    }

    /// …with the registry the other commands resolve slot back-ends with:
    /// a `[persistent]` slot of a cell is then the same store under `soma
    /// run app.cell` as under `soma run app.cell Cell.h`, `serve` and `test`
    /// (it was a JSON file here and a sqlite table there — two stores of
    /// one slot, neither seeing the other's writes)
    pub fn with_registry(program: Program, registry: Option<&crate::registry::Registry>) -> Self {
        let mut cells = IndexMap::new();

        for cell_spanned in &program.cells {
            let cell = &cell_spanned.node;
            if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
                continue;
            }
            let instance = Self::instantiate_cell(cell, "", registry);
            cells.insert(cell.name.clone(), instance);
        }
        let top_names = program.cells.iter().map(|c| c.node.name.clone()).collect();

        Self {
            cells,
            signal_log: Vec::new(),
            program,
            machine_storage: HashMap::new(),
            scope_tables: HashMap::new(),
            top_names,
        }
    }

    /// Instantiate a cell: create storage backends for memory slots,
    /// recurse into interior children
    fn instantiate_cell(def: &CellDef, parent_path: &str, registry: Option<&crate::registry::Registry>) -> CellInstance {
        let cell_path = if parent_path.is_empty() {
            def.name.clone()
        } else {
            format!("{}.{}", parent_path, def.name)
        };

        let mut memory = HashMap::new();
        let mut children = IndexMap::new();

        for section in &def.sections {
            match &section.node {
                Section::Memory(mem) => {
                    for slot in &mem.slots {
                        let props: Vec<String> = slot.node.properties.iter()
                            .map(|p| p.node.name().to_string())
                            .collect();
                        let backend = match registry {
                            Some(r) => storage::resolve_backend_from_registry(&cell_path, &slot.node.name, &props, r),
                            None => resolve_backend(&cell_path, &slot.node.name, &props),
                        };
                        memory.insert(slot.node.name.clone(), backend);
                    }
                }
                Section::Interior(interior) => {
                    for child in &interior.cells {
                        if matches!(child.node.kind, CellKind::Cell | CellKind::Agent) {
                            let child_instance = Self::instantiate_cell(&child.node, &cell_path, registry);
                            children.insert(child.node.name.clone(), child_instance);
                        }
                    }
                }
                _ => {}
            }
        }

        CellInstance {
            name: def.name.clone(),
            path: cell_path,
            def: def.clone(),
            memory,
            children,
        }
    }

    /// The runtime tables of the interior cell `child` (created on first
    /// use, then kept for the whole run): persistent under `soma run`, in
    /// memory otherwise. Rows a legacy bare-named table held are copied
    /// once into the new qualified table — unless a top-level cell of the
    /// program has that name (the bare table is then its own).
    fn child_tables(&mut self, child: &CellInstance) -> HashMap<String, Arc<dyn StorageBackend>> {
        if let Some(t) = self.scope_tables.get(&child.path) {
            return t.clone();
        }
        let persistent = interpreter::PERSIST_MACHINES.load(std::sync::atomic::Ordering::Relaxed)
            || child.memory.values().any(|b| matches!(b.backend_name(), "sqlite" | "file"));
        let mut wanted: Vec<(String, String, &'static str)> = vec![
            (format!("{}.__counters", child.name), "_counters".to_string(), "next_id() counter"),
            (format!("{}.__agent_memory", child.name), "_agent_memory".to_string(), "remember() memory"),
            (format!("{}.__waits", child.name), "_waits".to_string(), "stored waits"),
        ];
        for s in &child.def.sections {
            if let Section::State(sm) = &s.node {
                wanted.push((format!("__sm_{}_{}", child.name, sm.name), format!("_sm_{}", sm.name), "state-machine instances"));
            }
        }
        let mut out: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        for (key, slot, what) in wanted {
            let backend: Arc<dyn StorageBackend> = if !persistent {
                Arc::new(storage::MemoryBackend::new())
            } else if self.top_names.contains(&child.name) {
                Arc::new(storage::SqliteBackend::new(&child.path, &slot))
            } else {
                let legacy = format!("{}_{}", child.name, slot);
                let (b, copied) = storage::sqlite_adopting(&child.path, &slot, &legacy);
                if copied > 0 {
                    eprintln!("note: interior cell {} keeps its {} in table \"{}_{}\" from now on — {} row(s) copied from the shared table \"{}\" (left as it was)",
                        child.path, what, child.path, slot, copied, legacy);
                }
                Arc::new(b)
            };
            out.insert(key, backend);
        }
        self.scope_tables.insert(child.path.clone(), out.clone());
        out
    }

    /// An interpreter for the interior scope of `parent_cell`: its children
    /// registered (handlers, machines, invariants, in declaration order),
    /// every sibling's slots and runtime tables injected, the parent's slots
    /// too, and `signal` dispatch confined to the scope.
    fn scope_interpreter(&mut self, parent_cell: &str) -> Result<interpreter::Interpreter, String> {
        let parent = self.find_cell(parent_cell)
            .ok_or_else(|| format!("cell '{}' not found", parent_cell))?
            .clone();
        let mut interp = interpreter::Interpreter::new(&self.program);

        // interior cell definitions — with their state machines and memory
        // invariants (register_cell), as `soma verify` proves them
        for section in &parent.def.sections {
            if let Section::Interior(ref interior) = section.node {
                for child_def in &interior.cells {
                    interp.register_interior(child_def.node.clone(), &parent.name);
                }
            }
        }

        // every sibling's slots, under `Cell.slot`: a handler's `signal`
        // runs the siblings' handlers in the interpreter, which found no
        // storage for them ("undefined variable: vals")
        for (name, sibling) in &parent.children {
            let keyed: HashMap<String, Arc<dyn StorageBackend>> = sibling.memory.iter()
                .map(|(slot, b)| (format!("{}.{}", name, slot), b.clone()))
                .collect();
            interp.set_storage_raw(&keyed);
            let tables = self.child_tables(sibling);
            interp.set_storage_raw(&tables);
        }

        // every top-level cell's slots, under `Cell.slot`: an event a top-level
        // cell raises reaches the top-level cells too (`Interpreter::hears`),
        // and their handlers found no storage ("undefined variable: got")
        let tops: Vec<(String, HashMap<String, Arc<dyn StorageBackend>>)> = self.cells.iter()
            .filter(|(n, _)| n.as_str() != parent_cell)
            .map(|(n, c)| (n.clone(), c.memory.clone())).collect();
        for (name, memory) in tops {
            let keyed: HashMap<String, Arc<dyn StorageBackend>> = memory.iter().map(|(slot, b)| (format!("{}.{}", name, slot), b.clone())).collect();
            interp.set_storage_raw(&keyed);
        }
        // the parent's storage
        interp.set_storage(parent_cell, &parent.memory);

        // machine instances survive from one handler run to the next
        interp.set_storage_raw(&self.machine_storage);
        interp.ensure_state_machine_storage();
        for (k, b) in interp.storage.iter() {
            if k.starts_with("__sm_") && !self.machine_storage.contains_key(k) {
                self.machine_storage.insert(k.clone(), b.clone());
            }
        }

        // who hears an event is the interpreter's one rule (`listeners_of`):
        // an interior cell's, its siblings and its parent; a top-level cell's,
        // the top-level cells and its own interior
        Ok(interp)
    }

    /// Emit a signal into the system. Finds handlers among siblings and executes them.
    pub fn emit_signal(
        &mut self,
        parent_cell: &str,
        signal_name: &str,
        args: Vec<Value>,
    ) -> Result<Vec<Value>, String> {
        let mut results = Vec::new();

        // Find the parent cell
        let parent = self.find_cell(parent_cell)
            .ok_or_else(|| format!("cell '{}' not found", parent_cell))?;

        // Find children that handle this signal (via `on` sections)
        let handlers: Vec<(String, OnSection)> = parent.children.values()
            .flat_map(|child| {
                child.def.sections.iter().filter_map(|s| {
                    if let Section::OnSignal(ref on) = s.node {
                        if on.signal_name == signal_name {
                            return Some((child.name.clone(), on.clone()));
                        }
                    }
                    None
                })
            })
            .collect();

        // Also check children's face for `await` declarations
        let awaiters: Vec<String> = parent.children.values()
            .filter(|child| {
                child.def.sections.iter().any(|s| {
                    if let Section::Face(ref face) = s.node {
                        face.declarations.iter().any(|d| {
                            if let FaceDecl::Await(ref aw) = d.node {
                                aw.name == signal_name
                            } else {
                                false
                            }
                        })
                    } else {
                        false
                    }
                })
            })
            .map(|c| c.name.clone())
            .collect();

        self.signal_log.push(format!(
            "signal {} -> {} handler(s), {} awaiter(s)",
            signal_name, handlers.len(), awaiters.len()
        ));

        // Execute each handler
        for (child_name, handler) in &handlers {
            let result = self.execute_handler(parent_cell, child_name, handler, args.clone())?;
            results.push(result);
        }

        Ok(results)
    }

    /// Execute a signal handler on a child cell
    fn execute_handler(
        &mut self,
        parent_cell: &str,
        child_name: &str,
        handler: &OnSection,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let mut interp = self.scope_interpreter(parent_cell)?;

        // the running child's own slots, also under their bare names
        if let Some(child_instance) = self.find_cell(parent_cell).and_then(|p| p.children.get(child_name)) {
            interp.set_storage(child_name, &child_instance.memory);
        }

        let result = interp.call_signal(child_name, &handler.signal_name, args)
            .map_err(|e| format!("{}", e))?;

        // The interpreter already ran every sibling handler of the signals
        // this handler emitted (in order, synchronously); dispatching them
        // again here ran each handler twice
        let emitted = interp.take_emitted_signals();
        for (sig_name, _) in emitted {
            self.signal_log.push(format!("signal {} (emitted by {}.{})", sig_name, child_name, handler.signal_name));
        }

        Ok(result)
    }

    /// Run one handler of the child `child_name` of `parent_cell` (`soma run
    /// app.cell Child.handler …`), its siblings registered as for a signal.
    pub fn call_child_handler(&mut self, parent_cell: &str, child_name: &str, handler: &str, args: Vec<Value>) -> Result<Value, String> {
        let parent = self.find_cell(parent_cell)
            .ok_or_else(|| format!("cell '{}' not found", parent_cell))?;
        let on = parent.children.get(child_name)
            .and_then(|c| c.def.sections.iter().find_map(|s| match &s.node {
                Section::OnSignal(on) if on.signal_name == handler => Some(on.clone()),
                _ => None,
            }))
            .ok_or_else(|| format!("cell '{}' has no handler '{}'", child_name, handler))?;
        self.execute_handler(parent_cell, child_name, &on, args)
    }

    /// Run a top-level cell's runtime section, in order: `let`s and other
    /// statements run in the interpreter (in one environment, so a later
    /// `signal go(k)` reads the `let k`), and the arguments of a `signal`
    /// are evaluated there — they were read as literals only (`[1, 2]`,
    /// `1 + 2` arrived as (), a name as its own text).
    pub fn run_cell(&mut self, cell_name: &str) -> Result<(), String> {
        let cell = self.find_cell(cell_name)
            .ok_or_else(|| format!("cell '{}' not found", cell_name))?;

        // Find the runtime section
        let runtime_entries: Vec<Spanned<RuntimeEntry>> = cell.def.sections.iter()
            .filter_map(|s| {
                if let Section::Runtime(ref rt) = s.node {
                    Some(rt.entries.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        if runtime_entries.is_empty() {
            return Ok(());
        }

        let mut interp = self.scope_interpreter(cell_name)?;
        let mut env = interpreter::Env::default();

        for entry in &runtime_entries {
            match &entry.node {
                RuntimeEntry::Emit { signal_name, args } => {
                    let std_env: HashMap<String, Value> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    let mut values = Vec::with_capacity(args.len());
                    for a in args {
                        let v = interp.eval_expr_with_env(&a.node, &std_env, cell_name, "runtime")
                            .map_err(|e| format!("runtime section, `signal {}`: {}", signal_name, exec_error_text(e)))?;
                        values.push(v);
                    }
                    self.emit_signal(cell_name, signal_name, values)?;
                }
                RuntimeEntry::Start { cell_name: child } => {
                    // every interior cell is live from the start
                    self.signal_log.push(format!("start {}: already running (interior cells run from the start)", child));
                }
                RuntimeEntry::Connect { from_cell, signal, to_cell } => {
                    // a declaration: a `signal` reaches every sibling that
                    // handles it, wired or not (check validates the names)
                    self.signal_log.push(format!(
                        "connect {}.{} -> {} (declared; signals reach every sibling handling them)",
                        from_cell, signal, to_cell
                    ));
                }
                RuntimeEntry::Stmt(stmt) => {
                    let body = [Spanned { node: stmt.clone(), span: entry.span }];
                    interp.exec_every(&body, &mut env, cell_name)
                        .map_err(|e| format!("runtime section: {}", e))?;
                }
            }
        }

        Ok(())
    }

    /// The start-up audit (`Interpreter::audit_stored_data`) over the data of
    /// the interior cells: their machine instances, their slots against the
    /// invariants of the current program. The interpreter's own audit sees
    /// top-level cells only; an interior cell's stored state, in a state its
    /// machine no longer declares or past an invariant added since, was read
    /// back without a word.
    pub fn audit_interior(&mut self) -> Vec<String> {
        let parents: Vec<String> = self.cells.iter().filter(|(_, c)| !c.children.is_empty()).map(|(n, _)| n.clone()).collect();
        let mut out = Vec::new();
        for parent in parents {
            let Ok(mut interp) = self.scope_interpreter(&parent) else { continue };
            let children: Vec<String> = self.cells.get(&parent).map(|c| c.children.keys().cloned().collect()).unwrap_or_default();
            // (only what is the interior's: the top-level cells have their own audit)
            interp.state_machines.retain(|(c, _), _| children.contains(c));
            interp.invariants.retain(|k, _| children.iter().any(|c| k.starts_with(&format!("{}.", c))));
            let keep: Vec<String> = interp.storage.keys().filter(|k| children.iter().any(|c| k.starts_with(&format!("{}.", c)) || k.starts_with(&format!("__sm_{}_", c)))).cloned().collect();
            interp.storage.retain(|k, _| keep.contains(k));
            for line in interp.audit_stored_data() { out.push(format!("(interior of {}) {}", parent, line)); }
        }
        out
    }

    fn find_cell(&self, name: &str) -> Option<&CellInstance> {
        // Try top-level
        if let Some(cell) = self.cells.get(name) {
            return Some(cell);
        }
        // Try as child of any top-level cell
        for top in self.cells.values() {
            if let Some(child) = top.children.get(name) {
                return Some(child);
            }
        }
        None
    }

    /// Print the runtime state for debugging
    pub fn dump_state(&self) {
        for (name, cell) in &self.cells {
            println!("cell {} {{", name);
            for (slot_name, backend) in &cell.memory {
                println!("  {} [{}]: {} entries", slot_name, backend.backend_name(), backend.len());
            }
            for (child_name, child) in &cell.children {
                println!("  child {} {{", child_name);
                for (slot_name, backend) in &child.memory {
                    println!("    {} [{}]: {} entries", slot_name, backend.backend_name(), backend.len());
                }
                println!("  }}");
            }
            println!("}}");
        }
    }
}

mod file_storage;
