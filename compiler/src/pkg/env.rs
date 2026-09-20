use std::path::{Path, PathBuf};
use std::fs;

const ENV_DIR: &str = ".soma_env";

/// A Soma environment — isolated package space per project (like conda env)
#[derive(Debug)]
pub struct SomaEnv {
    pub root: PathBuf,
    pub packages_dir: PathBuf,
    pub stdlib_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl SomaEnv {
    /// Create or load an environment for the given project directory
    pub fn init(project_dir: &Path) -> Result<Self, String> {
        let root = project_dir.join(ENV_DIR);
        let packages_dir = root.join("packages");
        let stdlib_dir = root.join("stdlib");
        let cache_dir = root.join("cache");

        fs::create_dir_all(&packages_dir)
            .map_err(|e| format!("cannot create env: {}", e))?;
        fs::create_dir_all(&stdlib_dir)
            .map_err(|e| format!("cannot create env: {}", e))?;
        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("cannot create env: {}", e))?;

        // Copy stdlib into env if not already there
        let env = Self { root, packages_dir, stdlib_dir, cache_dir };
        env.sync_stdlib(project_dir)?;

        Ok(env)
    }

    /// Load an existing environment
    pub fn load(project_dir: &Path) -> Option<Self> {
        let root = project_dir.join(ENV_DIR);
        if !root.exists() {
            return None;
        }
        Some(Self {
            packages_dir: root.join("packages"),
            stdlib_dir: root.join("stdlib"),
            cache_dir: root.join("cache"),
            root,
        })
    }

    /// Sync stdlib into this env: from a stdlib directory when one is
    /// around (repo checkout, next to the binary, ~/.soma/stdlib), and from
    /// the copy embedded in the binary otherwise — a lone `soma` executable
    /// must still know what `persistent` means.
    fn sync_stdlib(&self, project_dir: &Path) -> Result<(), String> {
        let mut candidates = vec![
            project_dir.join("stdlib"),
            PathBuf::from("stdlib"),
            PathBuf::from("../stdlib"),
        ];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(parent.join("../stdlib"));
                candidates.push(parent.join("stdlib"));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".soma/stdlib"));
        }

        if let Some(src) = candidates.iter().find(|p| dir_has_cells(p)) {
            let entries = fs::read_dir(src).map_err(|e| format!("{}", e))?;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "cell") {
                    let dest = self.stdlib_dir.join(path.file_name().unwrap());
                    if !dest.exists() {
                        fs::copy(&path, &dest).map_err(|e| format!("{}", e))?;
                    }
                }
            }
        }
        // whatever is still missing comes from the binary
        write_embedded_stdlib(&self.stdlib_dir)?;

        Ok(())
    }

    /// Get the package directory for a specific package
    pub fn package_dir(&self, name: &str) -> PathBuf {
        self.packages_dir.join(name)
    }

    /// List all installed packages
    pub fn list_packages(&self) -> Vec<String> {
        let mut packages = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.packages_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        // Resolver checkouts are inputs to installation, not
                        // installed packages (nor a second source of cells).
                        if name.starts_with("_git_") && entry.path().join(".git").exists() { continue; }
                        packages.push(name.to_string());
                    }
                }
            }
        }
        packages.sort();
        packages
    }

    /// Clean the environment (remove all packages and cache)
    pub fn clean(&self) -> Result<(), String> {
        if self.packages_dir.exists() {
            fs::remove_dir_all(&self.packages_dir)
                .map_err(|e| format!("cannot clean packages: {}", e))?;
            fs::create_dir_all(&self.packages_dir)
                .map_err(|e| format!("{}", e))?;
        }
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)
                .map_err(|e| format!("cannot clean cache: {}", e))?;
            fs::create_dir_all(&self.cache_dir)
                .map_err(|e| format!("{}", e))?;
        }
        Ok(())
    }

    /// Get all .cell file paths from the environment (stdlib + packages)
    pub fn all_cell_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Stdlib
        if let Ok(entries) = fs::read_dir(&self.stdlib_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "cell") {
                    paths.push(path);
                }
            }
        }

        // Packages
        for pkg_name in self.list_packages() {
            let pkg_dir = self.packages_dir.join(&pkg_name);
            if let Ok(entries) = fs::read_dir(&pkg_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "cell") {
                        paths.push(path);
                    }
                }
            }
        }

        paths
    }
}

/// The standard property definitions, embedded at build time.
pub const EMBEDDED_STDLIB: &[(&str, &str)] = &[
    ("access.cell", include_str!("../../../stdlib/access.cell")),
    ("backends.cell", include_str!("../../../stdlib/backends.cell")),
    ("budget.cell", include_str!("../../../stdlib/budget.cell")),
    ("builtins.cell", include_str!("../../../stdlib/builtins.cell")),
    ("consistency.cell", include_str!("../../../stdlib/consistency.cell")),
    ("durability.cell", include_str!("../../../stdlib/durability.cell")),
    ("lifecycle.cell", include_str!("../../../stdlib/lifecycle.cell")),
    ("mutability.cell", include_str!("../../../stdlib/mutability.cell")),
    ("redundancy.cell", include_str!("../../../stdlib/redundancy.cell")),
];

/// True when `dir` holds at least one .cell file (an empty stdlib
/// directory is worse than none: it hides the real one).
pub fn dir_has_cells(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|it| it.flatten().any(|e| e.path().extension().map_or(false, |x| x == "cell")))
        .unwrap_or(false)
}

/// Write the embedded stdlib files that `dir` does not have yet.
pub fn write_embedded_stdlib(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    for (name, text) in EMBEDDED_STDLIB {
        let dest = dir.join(name);
        if !dest.exists() {
            fs::write(&dest, text).map_err(|e| format!("cannot write {}: {}", dest.display(), e))?;
        }
    }
    Ok(())
}
