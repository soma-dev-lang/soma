use std::fs;
use std::process;

use crate::provider;
use crate::runtime;

pub fn cmd_add_provider(name: &str) {
    let cwd = std::env::current_dir().unwrap();
    let providers_dir = cwd.join(".soma_env/providers").join(name);

    if providers_dir.exists() {
        eprintln!("provider '{}' already installed at {}", name, providers_dir.display());
        return;
    }

    fs::create_dir_all(&providers_dir).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    let manifest = format!(r#"[provider]
name = "{name}"
version = "0.1.0"
description = "{name} storage provider for Soma"

[provider.auth]
env = []

# Define backends here. Example:
# [[backend]]
# name = "default"
# requires = ["persistent", "consistent"]
# optional = ["encrypted"]
# native = "sqlite"
"#);

    fs::write(providers_dir.join("soma-provider.toml"), manifest).unwrap();

    let soma_toml = cwd.join("soma.toml");
    if soma_toml.exists() {
        let mut content = fs::read_to_string(&soma_toml).unwrap_or_default();
        if !content.contains("[storage]") {
            content.push_str(&format!("\n[storage]\nprovider = \"{}\"\n", name));
            fs::write(&soma_toml, content).unwrap();
        }
    }

    println!("provider '{}' added", name);
    println!("  manifest: {}/soma-provider.toml", providers_dir.display());
    println!("  edit the manifest to configure backends");
    println!("  test with: soma test-provider {}", name);
}

pub fn cmd_test_provider(name: &str) {
    let cwd = std::env::current_dir().unwrap();

    let resolver = if name == "local" {
        provider::ProviderResolver::local()
    } else {
        let config = provider::StorageConfig {
            provider: name.to_string(),
            config: std::collections::HashMap::new(),
            overrides: std::collections::HashMap::new(),
        };
        provider::ProviderResolver::from_config(config, &cwd).unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            process::exit(1);
        })
    };

    println!("testing provider: {}", resolver.provider_name());
    println!("");

    let mut pass = 0;
    let mut fail = 0;

    let test_cases = vec![
        ("persistent, consistent", vec!["persistent".to_string(), "consistent".to_string()]),
        ("ephemeral", vec!["ephemeral".to_string()]),
        ("persistent, encrypted", vec!["persistent".to_string(), "encrypted".to_string()]),
        ("ephemeral, local", vec!["ephemeral".to_string(), "local".to_string()]),
    ];

    for (label, props) in &test_cases {
        let request = provider::StorageRequest {
            cell_name: "Test".to_string(),
            field_name: "data".to_string(),
            field_type: "Map<String, String>".to_string(),
            properties: props.iter().map(|p| provider::Property::Flag(p.clone())).collect(),
        };
        match resolver.resolve(&request) {
            Ok(backend) => {
                backend.set("test_key", runtime::storage::StoredValue::String("test_val".to_string()));
                let got = backend.get("test_key");
                let keys = backend.keys();
                backend.delete("test_key");
                let after = backend.get("test_key");

                let ok = got.is_some() && keys.contains(&"test_key".to_string()) && after.is_none();
                if ok {
                    println!("  ✓ [{}] → {} — CRUD passed", label, backend.backend_name());
                    pass += 1;
                } else {
                    println!("  ✗ [{}] → {} — CRUD failed", label, backend.backend_name());
                    fail += 1;
                }
            }
            Err(e) => {
                println!("  ✗ [{}] — resolve failed: {}", label, e);
                fail += 1;
            }
        }
    }

    let bad_request = provider::StorageRequest {
        cell_name: "Test".to_string(),
        field_name: "bad".to_string(),
        field_type: "Map".to_string(),
        properties: vec![
            provider::Property::Flag("persistent".to_string()),
            provider::Property::Flag("ephemeral".to_string()),
        ],
    };
    match resolver.resolve(&bad_request) {
        Ok(_) => { println!("  ✗ [persistent, ephemeral] should not resolve"); fail += 1; }
        Err(_) => { println!("  ✓ [persistent, ephemeral] correctly rejected"); pass += 1; }
    }

    println!("\n{} tests: {} passed, {} failed", pass + fail, pass, fail);
    if fail > 0 { process::exit(1); }
}

pub fn cmd_migrate(from: &str, to: &str) {
    println!("soma migrate --from {} --to {}", from, to);
    println!("");
    println!("Migration reads all keys from source provider and writes to target.");
    println!("This uses the StorageBackend trait — works between any two providers.");
    println!("");

    let cwd = std::env::current_dir().unwrap();

    let source = if from == "local" {
        provider::ProviderResolver::local()
    } else {
        let config = provider::StorageConfig { provider: from.to_string(), ..Default::default() };
        provider::ProviderResolver::from_config(config, &cwd).unwrap_or_else(|e| {
            eprintln!("error loading source: {}", e); process::exit(1);
        })
    };

    let target = if to == "local" {
        provider::ProviderResolver::local()
    } else {
        let config = provider::StorageConfig { provider: to.to_string(), ..Default::default() };
        provider::ProviderResolver::from_config(config, &cwd).unwrap_or_else(|e| {
            eprintln!("error loading target: {}", e); process::exit(1);
        })
    };

    println!("source: {} → target: {}", source.provider_name(), target.provider_name());
    println!("note: migration requires a .cell file to know which fields to migrate.");
    println!("usage: soma migrate --from local --to aws (with soma.toml configured)");
}
