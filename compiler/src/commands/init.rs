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

    let main_path = project_dir.join("main.cell");
    if !main_path.exists() {
        fs::write(&main_path, r#"// Your Soma app starts here

cell App {
    face {
        signal hello(name: String) -> String
    }

    on hello(name: String) {
        return concat("Hello, ", concat(name, "!"))
    }
}
"#).ok();
    }

    let rel_prefix = if name.is_some() {
        format!("{}/", project_name)
    } else {
        String::new()
    };

    println!("initialized soma project: {}", project_name);
    println!("");
    println!("  {}soma.toml        project manifest", rel_prefix);
    println!("  {}main.cell        entry point", rel_prefix);
    println!("  {}.soma_env/       isolated environment", rel_prefix);
    println!("    stdlib/         {} property definitions", env.all_cell_paths().len());
    println!("    packages/      dependencies (empty)");
    println!("    cache/          compiled bytecode");
    println!("");
    println!("next steps:");
    if name.is_some() {
        println!("  cd {}", project_name);
    }
    println!("  soma run main.cell hello world");
    println!("  soma add mypackage --git https://github.com/user/repo");
    println!("  soma serve main.cell");
}

pub fn cmd_add(package: &str, version: Option<&str>, git: Option<&str>, path: Option<&str>) {
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
        })
    } else if let Some(local_path) = path {
        pkg::Dependency::Full(pkg::DependencySpec {
            git: None,
            path: Some(local_path.to_string()),
            version: None,
            branch: None,
        })
    } else {
        pkg::Dependency::Version(version.unwrap_or("*").to_string())
    };

    manifest.dependencies.insert(package.to_string(), dep);
    manifest.save(&manifest_path).unwrap_or_else(|e| {
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
