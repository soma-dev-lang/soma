use std::fs;
use std::process;

use crate::pkg;

pub fn cmd_init(name: Option<&str>) {
    let cwd = std::env::current_dir().unwrap();

    let (project_dir, project_name) = if let Some(n) = name {
        let dir = cwd.join(n);
        if dir.exists() {
            eprintln!("error: directory '{}' already exists", n);
            process::exit(1);
        }
        fs::create_dir_all(&dir).unwrap_or_else(|e| {
            eprintln!("error: cannot create directory '{}': {}", n, e);
            process::exit(1);
        });
        (dir, n.to_string())
    } else {
        let n = cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("myapp")
            .to_string();
        (cwd.clone(), n)
    };

    let manifest_path = project_dir.join("soma.toml");
    if manifest_path.exists() {
        eprintln!("soma.toml already exists");
        process::exit(1);
    }

    let manifest = pkg::Manifest::new(&project_name);
    manifest.save(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    let env = pkg::SomaEnv::init(&project_dir).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    // app.cell — the name every doc, the help text and the site use.
    let main_path = project_dir.join("app.cell");
    if !main_path.exists() {
        fs::write(&main_path, STARTER_APP).ok();
    }
    // AGENTS.md: any coding agent opening this project learns the loop
    // (check → verify → test) and where the exact references are.
    let agents_path = project_dir.join("AGENTS.md");
    if !agents_path.exists() {
        fs::write(&agents_path, AGENTS_MD).ok();
    }

    let rel_prefix = if name.is_some() {
        format!("{}/", project_name)
    } else {
        String::new()
    };

    println!("initialized soma project: {}", project_name);
    println!("");
    println!("  {}soma.toml        project manifest", rel_prefix);
    println!("  {}app.cell         entry point — a counter with an invariant, a lifecycle and tests", rel_prefix);
    println!("  {}AGENTS.md        instructions for coding agents working in this project", rel_prefix);
    println!("  {}.soma_env/       isolated environment", rel_prefix);
    println!("    stdlib/         {} property definitions", env.all_cell_paths().len());
    println!("    packages/      dependencies (empty)");
    println!("    cache/          compiled bytecode");
    println!("");
    println!("next steps:");
    if name.is_some() {
        println!("  cd {}", project_name);
    }
    println!("  soma check app.cell && soma verify app.cell && soma test app.cell");
    println!("  soma run app.cell add 5");
    println!("  soma serve app.cell              # http://127.0.0.1:8080 — GET /, POST /counter/5");
    println!("  soma example invariant state_machine      # verified programs to start from");
    println!("  soma docs agent | guarantees | serving    # the language, offline");
}

/// The `[dependencies.<name>]` block for a dependency, as TOML text.
fn dependency_block(name: &str, dep: &pkg::Dependency, nl: &str) -> String {
    match dep {
        pkg::Dependency::Version(v) => format!("[dependencies]{nl}{} = \"{}\"{nl}", name, v),
        pkg::Dependency::Full(spec) => {
            let mut out = format!("[dependencies.{}]{nl}", name);
            for (k, v) in [("git", &spec.git), ("path", &spec.path), ("version", &spec.version),
                           ("branch", &spec.branch), ("subdir", &spec.subdir)] {
                if let Some(v) = v { out.push_str(&format!("{} = \"{}\"{nl}", k, v)); }
            }
            out
        }
    }
}

/// Put `name` in the manifest text, replacing any entry it already has,
/// and leaving every other line — comments included — exactly as written.
fn insert_dependency(text: &str, name: &str, dep: &pkg::Dependency) -> String {
    // a CRLF manifest stays CRLF: rewriting every line ending is the kind of
    // whole-file diff this function exists to avoid
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let header_of = |l: &str| {
        let t = l.trim();
        (t.starts_with('[') && t.ends_with(']')).then(|| t[1..t.len() - 1].trim().to_string())
    };
    // drop what this package already has: its line under [dependencies],
    // and any [dependencies.<name>] block
    let mut kept: Vec<&str> = Vec::new();
    let mut table = String::new();
    let mut skipping = false;
    for line in text.lines() {
        if let Some(h) = header_of(line) {
            skipping = h == format!("dependencies.{}", name);
            table = h;
            if skipping { continue; }
        } else if skipping {
            continue;
        } else if table == "dependencies" {
            let t = line.trim_start();
            if t.starts_with(name) && t[name.len()..].trim_start().starts_with('=') { continue; }
        }
        kept.push(line);
    }
    let mut out = kept.join(nl);
    if !out.ends_with(nl) { out.push_str(nl); }
    match dep {
        // a simple version goes under the existing [dependencies] header
        pkg::Dependency::Version(v) if out.lines().any(|l| header_of(l).as_deref() == Some("dependencies")) => {
            let at = out.lines().position(|l| header_of(l).as_deref() == Some("dependencies")).unwrap();
            let mut lines: Vec<String> = out.lines().map(|l| l.to_string()).collect();
            lines.insert(at + 1, format!("{} = \"{}\"", name, v));
            lines.join(nl) + nl
        }
        _ => {
            let blank = format!("{nl}{nl}");
            if !out.ends_with(&blank) { out.push_str(nl); }
            out + &dependency_block(name, dep, nl)
        }
    }
}

pub fn cmd_add(package: &str, version: Option<&str>, git: Option<&str>, path: Option<&str>) {
    if !crate::pkg::resolver::valid_package_name(package) {
        eprintln!("error: '{}' is not a package name (letters, digits, `_`, `-`, `.`; no `/`, no `..`)", package);
        std::process::exit(1);
    }
    let cwd = std::env::current_dir().unwrap();
    let manifest_path = cwd.join("soma.toml");

    if !manifest_path.exists() {
        eprintln!("error: no soma.toml found (run `soma init` first)");
        process::exit(1);
    }

    let mut manifest = pkg::Manifest::load(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    let dep = if let Some(git_url) = git {
        pkg::Dependency::Full(pkg::DependencySpec {
            git: Some(git_url.to_string()),
            path: None,
            version: version.map(|v| v.to_string()),
            branch: None,
            subdir: None,
        })
    } else if let Some(local_path) = path {
        pkg::Dependency::Full(pkg::DependencySpec {
            git: None,
            path: Some(local_path.to_string()),
            version: None,
            branch: None,
            subdir: None,
        })
    } else {
        pkg::Dependency::Version(version.unwrap_or("*").to_string())
    };

    manifest.dependencies.insert(package.to_string(), dep.clone());
    // Edit the manifest's TEXT: re-serializing the struct dropped every
    // comment the author wrote and spelled out every default value.
    let text = std::fs::read_to_string(&manifest_path).unwrap_or_default();
    let edited = insert_dependency(&text, package, &dep);
    let ok = toml::from_str::<pkg::Manifest>(&edited)
        .map(|m| m.dependencies.contains_key(package))
        .unwrap_or(false);
    if !ok {
        eprintln!("error: soma.toml could not be edited safely — add the dependency by hand:");
        eprintln!("{}", dependency_block(package, &dep, "\n").trim_end());
        process::exit(1);
    }
    std::fs::write(&manifest_path, edited).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("added {} to soma.toml", package);
    println!("run `soma install` to fetch it");
}

pub fn cmd_install() {
    let cwd = std::env::current_dir().unwrap();
    let manifest_path = cwd.join("soma.toml");
    let lock_path = cwd.join("soma.lock");

    if !manifest_path.exists() {
        eprintln!("error: no soma.toml found (run `soma init` first)");
        process::exit(1);
    }

    let manifest = pkg::Manifest::load(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    let mut lock = pkg::LockFile::load(&lock_path).unwrap_or_default();

    let _env = pkg::SomaEnv::init(&cwd).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("installing {} dependencies...", manifest.dependencies.len());

    let _env_packages = cwd.join(".soma_env").join("packages");
    let installed = pkg::resolve_and_install(&cwd.join(".soma_env"), &manifest, &mut lock)
        .unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            process::exit(1);
        });

    lock.save(&lock_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("");
    println!("installed {} packages", installed.len());
    for (name, path) in &installed {
        println!("  {} → {}", name, path.display());
    }
}

pub fn cmd_env() {
    let cwd = std::env::current_dir().unwrap();

    if let Some(env) = pkg::SomaEnv::load(&cwd) {
        let packages = env.list_packages();
        let cell_files = env.all_cell_paths();

        println!("soma environment: {}", env.root.display());
        println!("");
        println!("stdlib: {} files", cell_files.iter().filter(|p| p.starts_with(&env.stdlib_dir)).count());
        println!("packages: {}", packages.len());
        for pkg in &packages {
            println!("  {}", pkg);
        }
        println!("total .cell files: {}", cell_files.len());
    } else {
        println!("no environment found (run `soma init`)");
    }
}

/// Drop-in instructions for coding agents — the same text the site serves
/// at https://soma-lang.dev/agent.md.
const AGENTS_MD: &str = include_str!("../../../site/agent.md");

/// The starter program. It passes check, verify and test as written, and
/// shows the four things a cell is for: a contract, a limit that holds, a
/// lifecycle that is proven, and tests that travel with the code.
const STARTER_APP: &str = r#"// A counter that cannot go negative, behind a session that must be opened
// before it is used and cannot be used once closed.
//
//   soma check  app.cell      static gates
//   soma verify app.cell      proves the `session` state machine
//   soma test   app.cell      runs CounterTests
//   soma run    app.cell add 5
//   soma serve  app.cell      GET /  ·  POST /counter/5  (the routes `request` declares)

cell Counter {
    face {
        signal open_session(id: String) -> String
        signal close_session(id: String) -> String
        signal add(n: Int) -> Int
        signal total() -> Int
        signal request(method: String, path: String, body: String) -> Map
    }

    memory {
        counts: Map<String, Int> [persistent]
        invariant counts >= 0 && counts <= 1000000     // both halves PROVEN by verify: the two `require`s in add() narrow the write
    }

    state session {
        initial: idle
        idle -> open
        open -> closed                                  // closed is terminal
    }

    on total() { return counts.get("n") ?? 0 }

    on add(n: Int) {
        require n >= 0 else NegativeAmount              // refused before anything is written
        let cur = counts.get("n") ?? 0                  // a stored value: verify knows it is in 0..1000000
        require cur + n <= 1000000 else Full "counter would exceed 1000000"
        counts.set("n", cur + n)                        // proven by induction — `soma verify --strict` is green
        return total()
    }

    on open_session(id: String) {
        transition(id, "open")
        return get_status(id)
    }

    on close_session(id: String) {
        transition(id, "closed")
        return get_status(id)
    }

    on request(method: String, path: String, body: String) {
        match map("method", method, "path", path) {
            {method: "GET", path: "/"} -> map("total", total())
            {method: "POST", path: "/counter/" + n} -> {
                let r = try { add(to_int(n)) }                     // a refusal becomes a status code, not a 500
                if r.error != () { return response(400, map("error", r.kind)) }
                map("total", r.value)
            }
            _ -> response(404, map("error", "not found"))
        }
    }
}

cell test CounterTests {
    rules {
        assert add(5) == 5
        assert_fails add(0 - 100) matching "NegativeAmount"   // refused…
        assert total() == 5                                   // …and the slot is unchanged
        assert request("POST", "/counter/-1", "")._status == 400
        assert request("POST", "/counter/2", "").total == 7
        assert open_session("s1") == "open"
        assert close_session("s1") == "closed"
        assert_fails open_session("s1")      // closed is terminal: no way back
    }
}
"#;
