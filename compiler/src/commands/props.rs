use crate::registry::Registry;

pub fn cmd_props(registry: &Registry) {
    println!("Registered properties ({} total):\n", registry.properties.len());

    let mut names: Vec<&String> = registry.properties.keys().collect();
    names.sort();

    for name in names {
        let def = &registry.properties[name];
        println!("  {} {}", if def.has_params { "●" } else { "○" }, name);

        if !def.promises.is_empty() {
            for p in &def.promises {
                println!("    promise: {}", p);
            }
        }
        if !def.contradicts.is_empty() {
            let mut c: Vec<&String> = def.contradicts.iter().collect();
            c.sort();
            println!("    contradicts: [{}]", c.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if !def.implies.is_empty() {
            let mut i: Vec<&String> = def.implies.iter().collect();
            i.sort();
            println!("    implies: [{}]", i.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if !def.requires.is_empty() {
            let mut r: Vec<&String> = def.requires.iter().collect();
            r.sort();
            println!("    requires: [{}]", r.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if let Some(ref group) = def.mutex_group {
            println!("    mutex_group: {}", group);
        }
        println!();
    }

    if !registry.mutex_groups.is_empty() {
        println!("Mutex groups:");
        for (group, members) in &registry.mutex_groups {
            println!("  {} → [{}]", group, members.join(", "));
        }
    }

    if !registry.backends.is_empty() {
        println!("\nBackends ({} total):", registry.backends.len());
        for backend in &registry.backends {
            println!("  {} (native: {})", backend.name,
                backend.native_impl.as_deref().unwrap_or("?"));
            for m in &backend.matches {
                println!("    matches [{}]", m.join(", "));
            }
        }
    }

    if !registry.builtins.is_empty() {
        println!("\nBuiltins ({} total):", registry.builtins.len());
        let mut names: Vec<&String> = registry.builtins.keys().collect();
        names.sort();
        for name in names {
            let def = &registry.builtins[name];
            println!("  {} (native: {})", name,
                def.native_impl.as_deref().unwrap_or("?"));
        }
    }
}
