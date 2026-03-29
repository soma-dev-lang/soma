//! `soma deploy` — deploy a cell to a cloud provider.
//!
//! The .cell declares what it needs (scale, memory properties).
//! The provider resolves how (D1, KV, DynamoDB, SQLite).
//! This command bridges the two.
//!
//! soma deploy app.cell --target cloudflare
//! soma deploy app.cell --target fly
//! soma deploy app.cell --target aws --region eu-west-1

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use crate::ast;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

#[derive(Debug, serde::Deserialize)]
struct ProviderConfig {
    provider: ProviderInfo,
    resolve: HashMap<String, String>,
    #[serde(default)]
    services: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    container: HashMap<String, toml::Value>,
    #[serde(default)]
    machine: HashMap<String, toml::Value>,
    #[serde(default)]
    ecs: HashMap<String, toml::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct ProviderInfo {
    name: String,
    compute: String,
}

pub fn cmd_deploy(path: &PathBuf, target: &str, region: Option<&str>) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    // Find the main cell
    let cell = program.cells.iter()
        .find(|c| c.node.kind == ast::CellKind::Cell)
        .unwrap_or_else(|| { eprintln!("error: no cell found"); std::process::exit(1); });

    let scale = cell.node.sections.iter().find_map(|s| {
        if let ast::Section::Scale(ref sc) = s.node { Some(sc) } else { None }
    });

    // Collect memory properties
    let memory_slots: Vec<(String, Vec<String>)> = cell.node.sections.iter()
        .filter_map(|s| {
            if let ast::Section::Memory(ref mem) = s.node {
                Some(mem.slots.iter().map(|slot| {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string()).collect();
                    (slot.node.name.clone(), props)
                }).collect::<Vec<_>>())
            } else { None }
        })
        .flatten()
        .collect();

    // Load provider config
    let provider_path = find_provider(target);
    let provider_content = std::fs::read_to_string(&provider_path)
        .unwrap_or_else(|e| { eprintln!("error: cannot read provider '{}': {}", target, e); std::process::exit(1); });
    let provider: ProviderConfig = toml::from_str(&provider_content)
        .unwrap_or_else(|e| { eprintln!("error: invalid provider config: {}", e); std::process::exit(1); });

    // Resolve memory properties to provider services
    eprintln!("soma deploy v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("cell: {}", cell.node.name);
    eprintln!("target: {} ({})", provider.provider.name, provider.provider.compute);
    eprintln!("---");

    eprintln!("storage resolution:");
    for (slot_name, props) in &memory_slots {
        let key = resolve_key(props);
        let service = provider.resolve.get(&key)
            .or_else(|| provider.resolve.get("ephemeral"))
            .map(|s| s.as_str())
            .unwrap_or("memory");
        eprintln!("  {} [{}] → {}", slot_name, props.join(", "), service);
    }

    if let Some(sc) = scale {
        eprintln!("scale:");
        eprintln!("  replicas: {}", sc.replicas);
        if let Some(ref shard) = sc.shard { eprintln!("  shard: {}", shard); }
        eprintln!("  consistency: {}", sc.consistency);
        if let Some(cpu) = sc.cpu { eprintln!("  cpu: {}", cpu); }
        if let Some(ref mem) = sc.memory { eprintln!("  memory: {}", mem); }
    }
    eprintln!("---");

    // Collect .cell files
    let base_dir = path.parent().unwrap_or(Path::new("."));
    let cell_files = collect_cell_files(path);
    let entry_name = path.file_name().unwrap().to_string_lossy();

    // Read package name
    let pkg_name = base_dir.join("soma.toml").exists()
        .then(|| {
            std::fs::read_to_string(base_dir.join("soma.toml")).ok()
                .and_then(|c| toml::from_str::<crate::pkg::manifest::Manifest>(&c).ok())
                .map(|m| m.package.name)
        })
        .flatten()
        .unwrap_or_else(|| cell.node.name.to_lowercase());

    match target {
        "cloudflare" => generate_cloudflare(&pkg_name, &entry_name, scale, &cell_files, &memory_slots, &provider, base_dir),
        "fly" => generate_fly(&pkg_name, &entry_name, scale, &cell_files, &memory_slots, &provider, base_dir),
        "aws" => generate_aws(&pkg_name, &entry_name, scale, &cell_files, &memory_slots, &provider, base_dir, region),
        _ => {
            eprintln!("error: unknown target '{}'. Available: cloudflare, fly, aws", target);
            eprintln!("hint: add a provider file at providers/{}.toml", target);
            std::process::exit(1);
        }
    }
}

fn resolve_key(props: &[String]) -> String {
    let has_persistent = props.iter().any(|p| p == "persistent");
    let has_consistent = props.iter().any(|p| p == "consistent");
    let has_ephemeral = props.iter().any(|p| p == "ephemeral");

    if has_persistent && has_consistent { "persistent_consistent".to_string() }
    else if has_persistent { "persistent".to_string() }
    else if has_ephemeral { "ephemeral".to_string() }
    else { "ephemeral".to_string() }
}

fn collect_cell_files(entry: &Path) -> Vec<String> {
    let mut files = vec![entry.file_name().unwrap().to_string_lossy().to_string()];
    let base = entry.parent().unwrap_or(Path::new("."));
    let lib_dir = base.join("lib");
    if lib_dir.is_dir() {
        for e in std::fs::read_dir(&lib_dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".cell") { files.push(format!("lib/{}", name)); }
        }
    }
    if base.join("soma.toml").exists() { files.push("soma.toml".to_string()); }
    files
}

fn find_provider(target: &str) -> PathBuf {
    // Search order: ./providers/, repo providers/, built-in
    let local = PathBuf::from(format!("providers/{}.toml", target));
    if local.exists() { return local; }

    // Try relative to the soma binary
    if let Ok(exe) = std::env::current_exe() {
        let repo = exe.parent().unwrap_or(Path::new("."))
            .join("../../providers").join(format!("{}.toml", target));
        if repo.exists() { return repo; }
    }

    // Try the paradigm repo path
    let paradigm = PathBuf::from(format!("/Users/antoine/paradigm/providers/{}.toml", target));
    if paradigm.exists() { return paradigm; }

    eprintln!("error: provider '{}' not found", target);
    eprintln!("hint: create providers/{}.toml", target);
    std::process::exit(1);
}

// ── Cloudflare ───────────────────────────────────────────────────────

fn generate_cloudflare(
    pkg: &str, entry: &str, scale: Option<&ast::ScaleSection>,
    files: &[String], memory: &[(String, Vec<String>)],
    provider: &ProviderConfig, base_dir: &Path,
) {
    let copy_lines: String = files.iter()
        .map(|f| format!("COPY {} /app/{}", f, f))
        .collect::<Vec<_>>()
        .join("\n");

    let dockerfile = format!(
        "FROM debian:bookworm-slim\n\
         RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*\n\
         COPY soma /usr/local/bin/soma\n\
         {}\n\
         WORKDIR /app\n\
         EXPOSE 8080\n\
         CMD [\"soma\", \"serve\", \"{}\", \"-p\", \"8080\"]\n",
        copy_lines, entry
    );

    // D1 bindings for persistent+consistent slots
    let mut d1_bindings = String::new();
    for (slot, props) in memory {
        let key = resolve_key(props);
        if provider.resolve.get(&key).map(|s| s.as_str()) == Some("d1") {
            d1_bindings.push_str(&format!(
                "\n[[d1_databases]]\nbinding = \"{}\"\ndatabase_name = \"soma-{}-{}\"\ndatabase_id = \"<create with: wrangler d1 create soma-{}-{}>\"\n",
                slot.to_uppercase(), pkg, slot, pkg, slot
            ));
        }
    }

    let wrangler = format!(
        "# Generated by: soma deploy --target cloudflare\n\
         name = \"{}\"\n\
         compatibility_date = \"2024-01-01\"\n\
         main = \"./worker.js\"\n\n\
         [[containers]]\n\
         name = \"soma\"\n\
         image = \"./Dockerfile\"\n\
         max_instances = {}\n{}\n",
        pkg,
        scale.map_or(1, |s| s.replicas),
        d1_bindings,
    );

    std::fs::write(base_dir.join("Dockerfile"), &dockerfile).unwrap();
    std::fs::write(base_dir.join("wrangler.toml"), &wrangler).unwrap();

    eprintln!("generated: Dockerfile");
    eprintln!("generated: wrangler.toml");
    eprintln!();
    eprintln!("next:");
    eprintln!("  1. cp $(which soma) ./soma");
    let mut step = 2;
    for (slot, props) in memory {
        let key = resolve_key(props);
        if provider.resolve.get(&key).map(|s| s.as_str()) == Some("d1") {
            eprintln!("  {}. wrangler d1 create soma-{}-{}", step, pkg, slot);
            eprintln!("     then update database_id in wrangler.toml for {}", slot.to_uppercase());
            step += 1;
        }
    }
    eprintln!("  {}. wrangler deploy", step);
}

// ── Fly.io ───────────────────────────────────────────────────────────

fn generate_fly(
    pkg: &str, entry: &str, scale: Option<&ast::ScaleSection>,
    files: &[String], _memory: &[(String, Vec<String>)],
    _provider: &ProviderConfig, base_dir: &Path,
) {
    let copy_lines: String = files.iter()
        .map(|f| format!("COPY {} /app/{}", f, f))
        .collect::<Vec<_>>()
        .join("\n");

    let mem = scale.and_then(|s| s.memory.as_deref()).unwrap_or("256");

    let dockerfile = format!(
        "FROM debian:bookworm-slim\n\
         RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*\n\
         COPY soma /usr/local/bin/soma\n\
         {}\n\
         WORKDIR /app\n\
         EXPOSE 8080\n\
         CMD [\"soma\", \"serve\", \"{}\", \"-p\", \"8080\"]\n",
        copy_lines, entry
    );

    let fly_toml = format!(
        "# Generated by: soma deploy --target fly\n\
         app = \"{}\"\n\
         primary_region = \"cdg\"\n\n\
         [build]\n\
         dockerfile = \"Dockerfile\"\n\n\
         [http_service]\n\
         internal_port = 8080\n\
         force_https = true\n\n\
         [[vm]]\n\
         size = \"shared-cpu-1x\"\n\
         memory = \"{}\"\n\n\
         [mounts]\n\
         source = \"soma_data\"\n\
         destination = \"/app/.soma_data\"\n",
        pkg, mem,
    );

    std::fs::write(base_dir.join("Dockerfile"), &dockerfile).unwrap();
    std::fs::write(base_dir.join("fly.toml"), &fly_toml).unwrap();

    eprintln!("generated: Dockerfile");
    eprintln!("generated: fly.toml");
    eprintln!();
    eprintln!("next:");
    eprintln!("  1. cp $(which soma) {}/soma", base_dir.display());
    eprintln!("  2. fly launch --copy-config");
    eprintln!("  3. fly volumes create soma_data --size 1");
    eprintln!("  4. fly deploy");
}

// ── AWS ──────────────────────────────────────────────────────────────

fn generate_aws(
    pkg: &str, entry: &str, scale: Option<&ast::ScaleSection>,
    files: &[String], memory: &[(String, Vec<String>)],
    provider: &ProviderConfig, base_dir: &Path,
    region: Option<&str>,
) {
    let copy_lines: String = files.iter()
        .map(|f| format!("COPY {} /app/{}", f, f))
        .collect::<Vec<_>>()
        .join("\n");

    let region = region.unwrap_or("us-east-1");

    let dockerfile = format!(
        "FROM debian:bookworm-slim\n\
         RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*\n\
         COPY soma /usr/local/bin/soma\n\
         {}\n\
         WORKDIR /app\n\
         EXPOSE 8080\n\
         CMD [\"soma\", \"serve\", \"{}\", \"-p\", \"8080\"]\n",
        copy_lines, entry
    );

    let cpu = scale.and_then(|s| s.cpu).unwrap_or(1);
    let mem = scale.and_then(|s| s.memory.as_deref()).unwrap_or("512Mi");

    // Generate a simple task definition
    let task_def = serde_json::json!({
        "family": pkg,
        "networkMode": "awsvpc",
        "requiresCompatibilities": ["FARGATE"],
        "cpu": format!("{}", cpu * 256),
        "memory": mem.replace("Mi", "").replace("Gi", "024"),
        "containerDefinitions": [{
            "name": "soma",
            "image": format!("{}.dkr.ecr.{}.amazonaws.com/{}:latest", "ACCOUNT_ID", region, pkg),
            "portMappings": [{"containerPort": 8080, "protocol": "tcp"}],
            "essential": true,
        }]
    });

    std::fs::write(base_dir.join("Dockerfile"), &dockerfile).unwrap();
    std::fs::write(base_dir.join("task-definition.json"),
        serde_json::to_string_pretty(&task_def).unwrap()).unwrap();

    eprintln!("generated: Dockerfile");
    eprintln!("generated: task-definition.json");
    eprintln!();
    eprintln!("next:");
    eprintln!("  1. cp $(which soma) {}/soma", base_dir.display());
    eprintln!("  2. docker build -t {} .", pkg);
    eprintln!("  3. aws ecr create-repository --repository-name {}", pkg);
    eprintln!("  4. docker push <ecr-url>/{}:latest", pkg);
    eprintln!("  5. aws ecs register-task-definition --cli-input-json file://task-definition.json");
    for (slot, props) in memory {
        let key = resolve_key(props);
        if provider.resolve.get(&key).map(|s| s.as_str()) == Some("dynamodb") {
            eprintln!("  6. aws dynamodb create-table --table-name soma_{}_{} --attribute-definitions ...", pkg, slot);
        }
    }
}
