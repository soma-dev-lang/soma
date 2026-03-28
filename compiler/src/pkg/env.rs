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

    /// Sync stdlib from the global installation into this env
    fn sync_stdlib(&self, project_dir: &Path) -> Result<(), String> {
        // Find global stdlib
        let candidates = [
            project_dir.join("stdlib"),
            PathBuf::from("stdlib"),
            PathBuf::from("../stdlib"),
        ];

        let global_stdlib = candidates.iter().find(|p| p.exists());

        if let Some(src) = global_stdlib {
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
