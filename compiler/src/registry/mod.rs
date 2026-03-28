use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::fs;

use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// A registered property definition, loaded from a `cell property` definition.
#[derive(Debug, Clone)]
pub struct PropertyDef {
    pub name: String,
    /// Properties this one contradicts (cannot coexist)
    pub contradicts: HashSet<String>,
    /// Properties this one implies (auto-added)
    pub implies: HashSet<String>,
    /// Properties this one requires (must coexist)
    pub requires: HashSet<String>,
    /// Mutex group — at most one property from each group
    pub mutex_group: Option<String>,
    /// Whether this property accepts parameters
    pub has_params: bool,
    /// Descriptive promises from the face
    pub promises: Vec<String>,
}

/// A registered checker definition, loaded from a `cell checker` definition.
#[derive(Debug, Clone)]
pub struct CheckerDef {
    pub name: String,
    pub promises: Vec<String>,
    pub check_body: Vec<Spanned<Statement>>,
}

/// A registered backend definition, loaded from `cell backend`.
#[derive(Debug, Clone)]
pub struct BackendDef {
    pub name: String,
    /// Properties this backend matches (e.g., [persistent] or [ephemeral])
    pub matches: Vec<Vec<String>>,
    /// Native implementation identifier
    pub native_impl: Option<String>,
    pub promises: Vec<String>,
}

/// A registered builtin function, loaded from `cell builtin`.
#[derive(Debug, Clone)]
pub struct BuiltinDef {
    pub name: String,
    pub native_impl: Option<String>,
    pub promises: Vec<String>,
}

/// The registry: all known properties, types, checkers, backends, and builtins.
/// Loaded from stdlib + user cell files.
pub struct Registry {
    pub properties: HashMap<String, PropertyDef>,
    pub mutex_groups: HashMap<String, Vec<String>>,
    pub checkers: Vec<CheckerDef>,
    pub backends: Vec<BackendDef>,
    pub builtins: HashMap<String, BuiltinDef>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            properties: HashMap::new(),
            mutex_groups: HashMap::new(),
            checkers: Vec::new(),
            backends: Vec::new(),
            builtins: HashMap::new(),
        }
    }

    /// Load all .cell files from a directory and register meta-cells
    pub fn load_dir(&mut self, dir: &Path) -> Result<(), String> {
        if !dir.exists() {
            return Ok(());
        }

        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("cannot read directory '{}': {}", dir.display(), e))?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "cell")
            })
            .collect();

        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let source = fs::read_to_string(entry.path())
                .map_err(|e| format!("cannot read '{}': {}", entry.path().display(), e))?;
            self.load_source(&source, &entry.path().display().to_string())?;
        }

        Ok(())
    }

    /// Load meta-cells from source code
    pub fn load_source(&mut self, source: &str, filename: &str) -> Result<(), String> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer
            .tokenize()
            .map_err(|e| format!("{}: {}", filename, e))?;
        let mut parser = Parser::new(tokens);
        let program = parser
            .parse_program()
            .map_err(|e| format!("{}: {}", filename, e))?;

        for cell in &program.cells {
            self.register_cell(&cell.node)?;
        }

        Ok(())
    }

    /// Register a single cell definition (only meta-cells are registered)
    fn register_cell(&mut self, cell: &CellDef) -> Result<(), String> {
        match cell.kind {
            CellKind::Property => self.register_property(cell),
            CellKind::Checker => self.register_checker(cell),
            CellKind::Backend => self.register_backend(cell),
            CellKind::Builtin => self.register_builtin(cell),
            CellKind::Type | CellKind::Test => Ok(()),
            CellKind::Cell => Ok(()),
        }
    }

    fn register_property(&mut self, cell: &CellDef) -> Result<(), String> {
        let mut def = PropertyDef {
            name: cell.name.clone(),
            contradicts: HashSet::new(),
            implies: HashSet::new(),
            requires: HashSet::new(),
            mutex_group: None,
            has_params: false,
            promises: Vec::new(),
        };

        for section in &cell.sections {
            match &section.node {
                Section::Face(face) => {
                    for decl in &face.declarations {
                        match &decl.node {
                            FaceDecl::Promise(p) => {
                                if let Constraint::Descriptive(s) = &p.constraint.node {
                                    def.promises.push(s.clone());
                                }
                            }
                            FaceDecl::Given(_) => {
                                def.has_params = true;
                            }
                            _ => {}
                        }
                    }
                }
                Section::Rules(rules) => {
                    for rule in &rules.rules {
                        match &rule.node {
                            Rule::Contradicts(names) => {
                                def.contradicts.extend(names.iter().cloned());
                            }
                            Rule::Implies(names) => {
                                def.implies.extend(names.iter().cloned());
                            }
                            Rule::Requires(names) => {
                                def.requires.extend(names.iter().cloned());
                            }
                            Rule::MutexGroup(group) => {
                                def.mutex_group = Some(group.clone());
                            }
                            Rule::Check(_) | Rule::Matches(_) | Rule::Native(_) | Rule::Assert(_) => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Register in mutex group
        if let Some(ref group) = def.mutex_group {
            self.mutex_groups
                .entry(group.clone())
                .or_default()
                .push(def.name.clone());
        }

        // Make contradicts bidirectional: if A contradicts B, B contradicts A
        let name = def.name.clone();
        let contradicts: Vec<String> = def.contradicts.iter().cloned().collect();
        self.properties.insert(name.clone(), def);

        for other_name in &contradicts {
            if let Some(other_def) = self.properties.get_mut(other_name) {
                other_def.contradicts.insert(name.clone());
            }
        }

        Ok(())
    }

    fn register_checker(&mut self, cell: &CellDef) -> Result<(), String> {
        let mut def = CheckerDef {
            name: cell.name.clone(),
            promises: Vec::new(),
            check_body: Vec::new(),
        };

        for section in &cell.sections {
            match &section.node {
                Section::Face(face) => {
                    for decl in &face.declarations {
                        if let FaceDecl::Promise(p) = &decl.node {
                            if let Constraint::Descriptive(s) = &p.constraint.node {
                                def.promises.push(s.clone());
                            }
                        }
                    }
                }
                Section::Rules(rules) => {
                    for rule in &rules.rules {
                        if let Rule::Check(body) = &rule.node {
                            def.check_body = body.clone();
                        }
                    }
                }
                _ => {}
            }
        }

        self.checkers.push(def);
        Ok(())
    }

    fn register_backend(&mut self, cell: &CellDef) -> Result<(), String> {
        let mut def = BackendDef {
            name: cell.name.clone(),
            matches: Vec::new(),
            native_impl: None,
            promises: Vec::new(),
        };

        for section in &cell.sections {
            match &section.node {
                Section::Face(face) => {
                    for decl in &face.declarations {
                        if let FaceDecl::Promise(p) = &decl.node {
                            if let Constraint::Descriptive(s) = &p.constraint.node {
                                def.promises.push(s.clone());
                            }
                        }
                    }
                }
                Section::Rules(rules) => {
                    for rule in &rules.rules {
                        match &rule.node {
                            Rule::Matches(props) => {
                                def.matches.push(props.clone());
                            }
                            Rule::Native(name) => {
                                def.native_impl = Some(name.clone());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        self.backends.push(def);
        Ok(())
    }

    fn register_builtin(&mut self, cell: &CellDef) -> Result<(), String> {
        let mut def = BuiltinDef {
            name: cell.name.clone(),
            native_impl: None,
            promises: Vec::new(),
        };

        for section in &cell.sections {
            match &section.node {
                Section::Face(face) => {
                    for decl in &face.declarations {
                        if let FaceDecl::Promise(p) = &decl.node {
                            if let Constraint::Descriptive(s) = &p.constraint.node {
                                def.promises.push(s.clone());
                            }
                        }
                    }
                }
                Section::Rules(rules) => {
                    for rule in &rules.rules {
                        if let Rule::Native(name) = &rule.node {
                            def.native_impl = Some(name.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        self.builtins.insert(def.name.clone(), def);
        Ok(())
    }

    /// Resolve which backend to use for a set of memory properties.
    /// Checks all registered backends and finds the best match.
    pub fn resolve_backend(&self, slot_properties: &[String]) -> Option<&BackendDef> {
        let mut best_match: Option<(&BackendDef, usize)> = None;

        for backend in &self.backends {
            for match_set in &backend.matches {
                // Check if ALL properties in the match set are present in the slot
                let all_match = match_set.iter().all(|p| slot_properties.contains(p));
                if all_match && match_set.len() > best_match.map(|(_, s)| s).unwrap_or(0) {
                    best_match = Some((backend, match_set.len()));
                }
            }
        }

        best_match.map(|(b, _)| b)
    }

    /// Check if a property name is known
    pub fn is_known_property(&self, name: &str) -> bool {
        self.properties.contains_key(name)
    }

    /// Get all properties that contradict the given one
    pub fn contradictions_for(&self, name: &str) -> HashSet<String> {
        let mut result = HashSet::new();

        // Direct contradictions from the property def
        if let Some(def) = self.properties.get(name) {
            result.extend(def.contradicts.iter().cloned());
        }

        // Mutex group contradictions
        if let Some(def) = self.properties.get(name) {
            if let Some(ref group) = def.mutex_group {
                if let Some(members) = self.mutex_groups.get(group) {
                    for member in members {
                        if member != name {
                            result.insert(member.clone());
                        }
                    }
                }
            }
        }

        result
    }

    /// Get all properties implied by the given one
    pub fn implications_for(&self, name: &str) -> HashSet<String> {
        self.properties
            .get(name)
            .map(|d| d.implies.clone())
            .unwrap_or_default()
    }

    /// Get all properties required by the given one
    pub fn requirements_for(&self, name: &str) -> HashSet<String> {
        self.properties
            .get(name)
            .map(|d| d.requires.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_property_from_source() {
        let mut reg = Registry::new();
        reg.load_source(
            r#"
            cell property persistent {
                face {
                    promise "data survives cell restart"
                }
                rules {
                    contradicts [ephemeral]
                    mutex_group durability
                }
            }

            cell property ephemeral {
                face {
                    promise "data lives only in memory"
                }
                rules {
                    contradicts [persistent, retain]
                    mutex_group durability
                }
            }
            "#,
            "test",
        )
        .unwrap();

        assert!(reg.is_known_property("persistent"));
        assert!(reg.is_known_property("ephemeral"));

        // persistent contradicts ephemeral
        let contras = reg.contradictions_for("persistent");
        assert!(contras.contains("ephemeral"));

        // ephemeral contradicts persistent (bidirectional)
        let contras = reg.contradictions_for("ephemeral");
        assert!(contras.contains("persistent"));

        // Mutex group: durability contains both
        assert_eq!(reg.mutex_groups["durability"].len(), 2);
    }

    #[test]
    fn test_load_property_with_implications() {
        let mut reg = Registry::new();
        reg.load_source(
            r#"
            cell property immutable {
                face {
                    promise "entries never change after write"
                }
                rules {
                    implies [consistent]
                    contradicts [evict]
                }
            }
            "#,
            "test",
        )
        .unwrap();

        let implies = reg.implications_for("immutable");
        assert!(implies.contains("consistent"));

        let contras = reg.contradictions_for("immutable");
        assert!(contras.contains("evict"));
    }

    #[test]
    fn test_load_checker_from_source() {
        let mut reg = Registry::new();
        reg.load_source(
            r#"
            cell checker auth_required {
                face {
                    promise "every network cell validates auth"
                }
                rules {
                    check {
                        require has_auth else MissingAuth
                    }
                }
            }
            "#,
            "test",
        )
        .unwrap();

        assert_eq!(reg.checkers.len(), 1);
        assert_eq!(reg.checkers[0].name, "auth_required");
        assert_eq!(reg.checkers[0].promises.len(), 1);
    }

    #[test]
    fn test_unknown_property() {
        let reg = Registry::new();
        assert!(!reg.is_known_property("nonexistent"));
    }
}
