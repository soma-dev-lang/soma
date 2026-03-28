pub mod storage;

use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::*;
use crate::interpreter::{self, Value};
use storage::{StorageBackend, resolve_backend};

/// A live cell instance with its state and handlers
#[derive(Clone)]
pub struct CellInstance {
    pub name: String,
    pub def: CellDef,
    /// Memory slots backed by storage
    pub memory: HashMap<String, Arc<dyn StorageBackend>>,
    /// Child cell instances
    pub children: HashMap<String, CellInstance>,
}

/// The Soma runtime: instantiates cells, wires signals, executes handlers
pub struct Runtime {
    /// Top-level cell instances
    pub cells: HashMap<String, CellInstance>,
    /// Signal log for debugging
    pub signal_log: Vec<String>,
    /// All cell definitions (for the interpreter)
    pub program: Program,
}

impl Runtime {
    /// Create a runtime from a parsed program
    pub fn new(program: Program) -> Self {
        let mut cells = HashMap::new();

        for cell_spanned in &program.cells {
            let cell = &cell_spanned.node;
            if cell.kind != CellKind::Cell {
                continue;
            }
            let instance = Self::instantiate_cell(cell, "");
            cells.insert(cell.name.clone(), instance);
        }

        Self {
            cells,
            signal_log: Vec::new(),
            program,
        }
    }

    /// Instantiate a cell: create storage backends for memory slots,
    /// recurse into interior children
    fn instantiate_cell(def: &CellDef, parent_path: &str) -> CellInstance {
        let cell_path = if parent_path.is_empty() {
            def.name.clone()
        } else {
            format!("{}.{}", parent_path, def.name)
        };

        let mut memory = HashMap::new();
        let mut children = HashMap::new();

        for section in &def.sections {
            match &section.node {
                Section::Memory(mem) => {
                    for slot in &mem.slots {
                        let props: Vec<String> = slot.node.properties.iter()
                            .map(|p| p.node.name().to_string())
                            .collect();
                        let backend = resolve_backend(&cell_path, &slot.node.name, &props);
                        memory.insert(slot.node.name.clone(), backend);
                    }
                }
                Section::Interior(interior) => {
                    for child in &interior.cells {
                        if child.node.kind == CellKind::Cell {
                            let child_instance = Self::instantiate_cell(&child.node, &cell_path);
                            children.insert(child.node.name.clone(), child_instance);
                        }
                    }
                }
                _ => {}
            }
        }

        CellInstance {
            name: def.name.clone(),
            def: def.clone(),
            memory,
            children,
        }
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
        // Build a program with all cells flattened so the interpreter can find them
        let mut interp = interpreter::Interpreter::new(&self.program);

        // Also inject interior cells and their storage
        let parent = self.find_cell(parent_cell)
            .ok_or_else(|| format!("cell '{}' not found", parent_cell))?;

        // Add interior cell definitions to interpreter
        for section in &parent.def.sections {
            if let Section::Interior(ref interior) = section.node {
                for child_def in &interior.cells {
                    interp.register_cell(child_def.node.clone());
                }
            }
        }

        // Inject storage backends for the child cell being executed
        if let Some(child_instance) = parent.children.get(child_name) {
            interp.set_storage(child_name, &child_instance.memory);
        }

        // Also inject parent's storage
        interp.set_storage(parent_cell, &parent.memory);

        let result = interp.call_signal(child_name, &handler.signal_name, args)
            .map_err(|e| format!("{}", e))?;

        // Check if the handler emitted signals (from the signal log)
        // For now, collect emitted signals and dispatch them
        let emitted = interp.take_emitted_signals();
        for (sig_name, sig_args) in emitted {
            self.emit_signal(parent_cell, &sig_name, sig_args)?;
        }

        Ok(result)
    }

    /// Run a top-level cell's runtime section
    pub fn run_cell(&mut self, cell_name: &str) -> Result<(), String> {
        let cell = self.find_cell(cell_name)
            .ok_or_else(|| format!("cell '{}' not found", cell_name))?;

        // Find the runtime section
        let runtime_entries: Vec<RuntimeEntry> = cell.def.sections.iter()
            .filter_map(|s| {
                if let Section::Runtime(ref rt) = s.node {
                    Some(rt.entries.iter().map(|e| e.node.clone()).collect::<Vec<_>>())
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        // Execute runtime entries
        for entry in &runtime_entries {
            match entry {
                RuntimeEntry::Emit { signal_name, args } => {
                    // Evaluate args (for now, just convert literals)
                    let values: Vec<Value> = args.iter()
                        .map(|a| expr_to_value(&a.node))
                        .collect();
                    self.emit_signal(cell_name, signal_name, values)?;
                }
                RuntimeEntry::Start { cell_name: child } => {
                    self.signal_log.push(format!("starting child: {}", child));
                }
                RuntimeEntry::Connect { from_cell, signal, to_cell } => {
                    self.signal_log.push(format!(
                        "wired: {}.{} -> {}",
                        from_cell, signal, to_cell
                    ));
                }
                RuntimeEntry::Stmt(_) => {}
            }
        }

        Ok(())
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

/// Convert an AST expression to a runtime value (for simple literal args)
fn expr_to_value(expr: &Expr) -> Value {
    match expr {
        Expr::Literal(Literal::Int(n)) => Value::Int(*n),
        Expr::Literal(Literal::Float(n)) => Value::Float(*n),
        Expr::Literal(Literal::String(s)) => Value::String(s.clone()),
        Expr::Literal(Literal::Bool(b)) => Value::Bool(*b),
        Expr::Ident(name) => Value::String(name.clone()),
        _ => Value::Unit,
    }
}
