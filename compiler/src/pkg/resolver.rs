use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::manifest::{Manifest, Dependency};
use super::lock::{LockFile, LockedPackage};

const CACHE_DIR: &str = "packages";

/// Resolve and install all dependencies
pub fn resolve_and_install(
    project_dir: &Path,
    manifest: &Manifest,
    lock: &mut LockFile,
) -> Result<HashMap<String, PathBuf>, String> {
    let mut installed = HashMap::new();
    let cache_dir = project_dir.join(CACHE_DIR);
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("cannot create {}: {}", cache_dir.display(), e))?;

    for (name, dep) in &manifest.dependencies {
        let pkg_path = resolve_package(name, dep, &cache_dir, lock)?;
        installed.insert(name.clone(), pkg_path);
    }

    Ok(installed)
}

/// Resolve a single package
fn resolve_package(
    name: &str,
    dep: &Dependency,
    cache_dir: &Path,
    lock: &mut LockFile,
) -> Result<PathBuf, String> {
    // Check if already locked and cached
    if let Some(locked) = lock.get(name) {
        let cached_path = cache_dir.join(name);
        if cached_path.exists() {
            eprintln!("  {} {} (cached)", name, locked.version);
            return Ok(cached_path);
        }
    }

    // Resolve from source
    if let Some(local_path) = dep.local_path() {
        resolve_local(name, local_path, cache_dir, lock)
    } else if let Some(git_url) = dep.git_url() {
        resolve_git(name, git_url, dep, cache_dir, lock)
    } else {
        // Treat version string as a shorthand git URL
        // e.g., "user/repo" → "https://github.com/user/repo"
        let version = dep.version_str();
        if version.contains('/') {
            let url = if version.starts_with("http") {
                version.to_string()
            } else {
                format!("https://github.com/{}", version)
            };
            resolve_git(name, &url, dep, cache_dir, lock)
        } else {
            Err(format!("cannot resolve package '{}': no git or path specified", name))
        }
    }
}

/// Resolve a local path dependency
fn resolve_local(
    name: &str,
    local_path: &str,
    cache_dir: &Path,
    lock: &mut LockFile,
) -> Result<PathBuf, String> {
    let src = PathBuf::from(local_path);
    if !src.exists() {
        return Err(format!("local path '{}' does not exist", local_path));
    }

    let dest = cache_dir.join(name);

    // Copy .cell files to cache
    let files = copy_cell_files(&src, &dest)?;
    let hash = hash_files(&dest, &files);

    lock.packages.insert(name.to_string(), LockedPackage {
        name: name.to_string(),
        version: "local".to_string(),
        source: format!("path:{}", local_path),
        hash,
        files,
    });

    eprintln!("  {} (local: {})", name, local_path);
    Ok(dest)
}

/// Resolve a git dependency
fn resolve_git(
    name: &str,
    url: &str,
    dep: &Dependency,
    cache_dir: &Path,
    lock: &mut LockFile,
) -> Result<PathBuf, String> {
    let dest = cache_dir.join(name);

    // Clone or update
    if dest.join(".git").exists() {
        // Pull latest
        let output = Command::new("git")
            .args(["pull", "--quiet"])
            .current_dir(&dest)
            .output()
            .map_err(|e| format!("git pull failed: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git pull failed: {}", stderr));
        }
    } else {
        // Fresh clone
        let mut args = vec!["clone", "--quiet", "--depth", "1"];

        // Branch if specified
        if let Dependency::Full(ref spec) = dep {
            if let Some(ref branch) = spec.branch {
                args.push("-b");
                args.push(branch);
            }
        }

        args.push(url);
        args.push(dest.to_str().unwrap_or(""));

        let output = Command::new("git")
            .args(&args)
            .output()
            .map_err(|e| format!("git clone failed: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git clone failed: {}", stderr));
        }
    }

    // Get commit hash
    let hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dest)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Find .cell files
    let files = find_cell_files(&dest);

    lock.packages.insert(name.to_string(), LockedPackage {
        name: name.to_string(),
        version: dep.version_str().to_string(),
        source: format!("git:{}", url),
        hash,
        files,
    });

    eprintln!("  {} {} (git: {})", name, dep.version_str(), url);
    Ok(dest)
}

/// Copy .cell files from src to dest
fn copy_cell_files(src: &Path, dest: &Path) -> Result<Vec<String>, String> {
    let _ = std::fs::create_dir_all(dest);
    let mut files = Vec::new();

    let entries = std::fs::read_dir(src)
        .map_err(|e| format!("cannot read {}: {}", src.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "cell") {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            std::fs::copy(&path, dest.join(&name))
                .map_err(|e| format!("cannot copy {}: {}", name, e))?;
            files.push(name);
        }
    }

    Ok(files)
}

/// Find all .cell files in a directory (recursively)
fn find_cell_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |e| e == "cell") {
                if let Some(name) = path.strip_prefix(dir).ok() {
                    files.push(name.to_string_lossy().to_string());
                }
            }
            if path.is_dir() && !path.file_name().map_or(false, |n| n.to_string_lossy().starts_with('.')) {
                // Recurse
                for sub in find_cell_files(&path) {
                    let rel = path.file_name().unwrap().to_string_lossy().to_string();
                    files.push(format!("{}/{}", rel, sub));
                }
            }
        }
    }
    files
}

/// Hash all files for content addressing
fn hash_files(dir: &Path, files: &[String]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for file in files {
        let path = dir.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            content.hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}
