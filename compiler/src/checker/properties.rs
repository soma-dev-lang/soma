use crate::ast::*;
use crate::registry::Registry;
use super::{CheckError, CheckWarning};

/// Checks memory properties against the registry.
/// All rules come from `cell property` definitions — nothing is hardcoded.
pub struct PropertyChecker<'a> {
    pub registry: &'a Registry,
    pub errors: Vec<CheckError>,
    pub warnings: Vec<CheckWarning>,
}

impl<'a> PropertyChecker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            registry,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn check_slot(&mut self, slot: &MemorySlot, span: Span) {
        let prop_names: Vec<String> = slot
            .properties
            .iter()
            .map(|p| p.node.name().to_string())
            .collect();

        // 1. Warn about unknown properties
        for prop in &slot.properties {
            let name = prop.node.name();
            if !self.registry.is_known_property(name) {
                self.warnings.push(CheckWarning::UnknownProperty {
                    slot: slot.name.clone(),
                    property: name.to_string(),
                    span: prop.span,
                });
            }
        }

        // 2. Check contradictions (from registry)
        for (i, prop_a) in prop_names.iter().enumerate() {
            let contradictions = self.registry.contradictions_for(prop_a);
            for prop_b in &prop_names[i + 1..] {
                if contradictions.contains(prop_b) {
                    self.errors.push(CheckError::PropertyContradiction {
                        slot: slot.name.clone(),
                        a: prop_a.clone(),
                        b: prop_b.clone(),
                        span,
                    });
                }
            }
        }

        // 3. Check mutex groups (at most one from each group)
        let mut seen_groups: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for name in &prop_names {
            if let Some(def) = self.registry.properties.get(name) {
                if let Some(ref group) = def.mutex_group {
                    if let Some(existing) = seen_groups.get(group) {
                        // Only report if not already caught by contradictions
                        if !self.registry.contradictions_for(name).contains(existing) {
                            self.errors.push(CheckError::PropertyContradiction {
                                slot: slot.name.clone(),
                                a: existing.clone(),
                                b: name.clone(),
                                span,
                            });
                        }
                    } else {
                        seen_groups.insert(group.clone(), name.clone());
                    }
                }
            }
        }

        // 4. Check implications (warn and suggest)
        for name in &prop_names {
            let implied = self.registry.implications_for(name);
            for imp in &implied {
                if !prop_names.contains(imp) {
                    // Check if it contradicts something already present
                    let imp_contradictions = self.registry.contradictions_for(imp);
                    let has_conflict = prop_names.iter().any(|p| imp_contradictions.contains(p));

                    if has_conflict {
                        self.errors.push(CheckError::InvalidPropertyCombination {
                            slot: slot.name.clone(),
                            reason: format!(
                                "'{}' implies '{}', but '{}' contradicts other properties on this slot",
                                name, imp, imp
                            ),
                            span,
                        });
                    } else {
                        self.warnings.push(CheckWarning::PropertyImplication {
                            slot: slot.name.clone(),
                            flag: name.clone(),
                            implied: imp.clone(),
                            span,
                        });
                    }
                }
            }
        }

        // 5. Check requirements (must coexist)
        for name in &prop_names {
            let required = self.registry.requirements_for(name);
            for req in &required {
                if !prop_names.contains(req) {
                    self.errors.push(CheckError::InvalidPropertyCombination {
                        slot: slot.name.clone(),
                        reason: format!("'{}' requires '{}' but it is not present", name, req),
                        span,
                    });
                }
            }
        }

        // 6. Check ttl < retain (semantic check on parameter values)
        self.check_ttl_vs_retain(slot, span);
    }

    /// Special semantic check: ttl must not be shorter than retain
    fn check_ttl_vs_retain(&mut self, slot: &MemorySlot, span: Span) {
        let ttl_ms = self.find_duration_param(slot, "ttl");
        let retain_ms = self.find_duration_param(slot, "retain");

        if let (Some(ttl), Some(retain)) = (ttl_ms, retain_ms) {
            if ttl < retain {
                self.errors.push(CheckError::InvalidPropertyCombination {
                    slot: slot.name.clone(),
                    reason: format!(
                        "ttl ({}) is shorter than retain ({}); data would expire before retention period ends",
                        format_ms(ttl),
                        format_ms(retain),
                    ),
                    span,
                });
            }
        }
    }

    fn find_duration_param(&self, slot: &MemorySlot, prop_name: &str) -> Option<f64> {
        for prop in &slot.properties {
            if let MemoryProperty::Param(ref p) = prop.node {
                if p.name == prop_name {
                    if let Some(first) = p.values.first() {
                        return duration_to_ms(&first.node);
                    }
                }
            }
        }
        None
    }
}

fn duration_to_ms(lit: &Literal) -> Option<f64> {
    match lit {
        Literal::Duration(d) => {
            let multiplier = match d.unit {
                DurationUnit::Milliseconds => 1.0,
                DurationUnit::Seconds => 1_000.0,
                DurationUnit::Minutes => 60_000.0,
                DurationUnit::Hours => 3_600_000.0,
                DurationUnit::Days => 86_400_000.0,
                DurationUnit::Years => 365.25 * 86_400_000.0,
            };
            Some(d.value * multiplier)
        }
        _ => None,
    }
}

fn format_ms(ms: f64) -> String {
    if ms < 1_000.0 {
        format!("{}ms", ms)
    } else if ms < 60_000.0 {
        format!("{}s", ms / 1_000.0)
    } else if ms < 3_600_000.0 {
        format!("{}min", ms / 60_000.0)
    } else if ms < 86_400_000.0 {
        format!("{}h", ms / 3_600_000.0)
    } else {
        format!("{}d", ms / 86_400_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> Registry {
        let mut reg = Registry::new();
        reg.load_source(
            r#"
            cell property persistent {
                face { promise "durable" }
                rules {
                    contradicts [ephemeral]
                    mutex_group durability
                }
            }
            cell property ephemeral {
                face { promise "in-memory" }
                rules {
                    contradicts [persistent, retain]
                    mutex_group durability
                }
            }
            cell property consistent {
                face { promise "linearizable" }
                rules {
                    contradicts [eventual, local]
                    mutex_group consistency
                }
            }
            cell property eventual {
                face { promise "eventually consistent" }
                rules {
                    contradicts [consistent, local]
                    mutex_group consistency
                }
            }
            cell property local {
                face { promise "per-instance" }
                rules {
                    contradicts [consistent, eventual]
                    mutex_group consistency
                }
            }
            cell property immutable {
                face { promise "append-only" }
                rules {
                    implies [consistent]
                    contradicts [evict]
                }
            }
            cell property evict {
                face { given policy: String }
                rules {
                    contradicts [immutable]
                }
            }
            cell property retain {
                face { given duration: Int }
                rules {
                    implies [persistent]
                    contradicts [ephemeral]
                }
            }
            cell property encrypted {
                face { promise "encrypted at rest" }
                rules {}
            }
            "#,
            "test_stdlib",
        )
        .unwrap();
        reg
    }

    fn make_slot(name: &str, props: Vec<MemoryProperty>) -> MemorySlot {
        MemorySlot {
            name: name.to_string(),
            ty: Spanned::new(TypeExpr::Simple("Int".to_string()), Span::new(0, 0)),
            properties: props
                .into_iter()
                .map(|p| Spanned::new(p, Span::new(0, 0)))
                .collect(),
        }
    }

    fn flag(name: &str) -> MemoryProperty {
        MemoryProperty::Flag(name.to_string())
    }

    fn param(name: &str, val: Literal) -> MemoryProperty {
        MemoryProperty::Param(PropertyParam {
            name: name.to_string(),
            values: vec![Spanned::new(val, Span::new(0, 0))],
        })
    }

    #[test]
    fn test_persistent_ephemeral_contradiction() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot("data", vec![flag("persistent"), flag("ephemeral")]);
        checker.check_slot(&slot, Span::new(0, 0));
        assert_eq!(checker.errors.len(), 1);
        assert!(matches!(checker.errors[0], CheckError::PropertyContradiction { .. }));
    }

    #[test]
    fn test_consistent_eventual_contradiction() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot("data", vec![flag("consistent"), flag("eventual")]);
        checker.check_slot(&slot, Span::new(0, 0));
        assert_eq!(checker.errors.len(), 1);
    }

    #[test]
    fn test_immutable_implies_consistent() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot("log", vec![flag("persistent"), flag("immutable")]);
        checker.check_slot(&slot, Span::new(0, 0));
        assert_eq!(checker.errors.len(), 0);
        assert!(checker.warnings.iter().any(|w| matches!(w, CheckWarning::PropertyImplication { .. })));
    }

    #[test]
    fn test_immutable_evict_contradiction() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot(
            "log",
            vec![
                flag("immutable"),
                param("evict", Literal::String("lru".to_string())),
            ],
        );
        checker.check_slot(&slot, Span::new(0, 0));
        assert!(checker.errors.len() >= 1);
    }

    #[test]
    fn test_ephemeral_retain_contradiction() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot(
            "data",
            vec![
                flag("ephemeral"),
                param(
                    "retain",
                    Literal::Duration(Duration {
                        value: 7.0,
                        unit: DurationUnit::Years,
                    }),
                ),
            ],
        );
        checker.check_slot(&slot, Span::new(0, 0));
        assert!(checker.errors.len() >= 1);
    }

    #[test]
    fn test_ttl_shorter_than_retain() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot(
            "data",
            vec![
                flag("persistent"),
                param(
                    "ttl",
                    Literal::Duration(Duration {
                        value: 30.0,
                        unit: DurationUnit::Days,
                    }),
                ),
                param(
                    "retain",
                    Literal::Duration(Duration {
                        value: 7.0,
                        unit: DurationUnit::Years,
                    }),
                ),
            ],
        );
        checker.check_slot(&slot, Span::new(0, 0));
        assert!(checker.errors.len() >= 1);
    }

    #[test]
    fn test_valid_slot() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot(
            "data",
            vec![flag("persistent"), flag("consistent"), flag("encrypted")],
        );
        checker.check_slot(&slot, Span::new(0, 0));
        assert_eq!(checker.errors.len(), 0);
    }

    #[test]
    fn test_custom_property_unknown_warning() {
        let reg = make_registry();
        let mut checker = PropertyChecker::new(&reg);
        let slot = make_slot("data", vec![flag("persistent"), flag("my_custom_thing")]);
        checker.check_slot(&slot, Span::new(0, 0));
        assert_eq!(checker.errors.len(), 0);
        assert!(checker.warnings.iter().any(|w| matches!(w, CheckWarning::UnknownProperty { .. })));
    }
}
