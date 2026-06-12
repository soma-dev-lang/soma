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
        // A version string with a slash is a git shorthand
        // ("user/repo" → https://github.com/user/repo). A bare semver
        // (or "*") resolves through the registry by NAME.
        let version = dep.version_str();
        if version.contains('/') {
            let url = if version.starts_with("http") {
                version.to_string()
            } else {
                format!("https://github.com/{}", version)
            };
            resolve_git(name, &url, dep, cache_dir, lock)
        } else {
            resolve_from_registry(name, version, cache_dir, lock)
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

/// Resolve a git dependency. Clones the repo into a side cache, then
/// copies the package's `.cell` files (from `subdir`, or the repo root)
/// into `<cache>/<name>` — so a monorepo can host many packages.
fn resolve_git(
    name: &str,
    url: &str,
    dep: &Dependency,
    cache_dir: &Path,
    lock: &mut LockFile,
) -> Result<PathBuf, String> {
    resolve_git_with(name, url, dep.branch(), dep.subdir(), dep.version_str(), cache_dir, lock)
}

#[allow(clippy::too_many_arguments)]
fn resolve_git_with(
    name: &str,
    url: &str,
    branch: Option<&str>,
    subdir: Option<&str>,
    version: &str,
    cache_dir: &Path,
    lock: &mut LockFile,
) -> Result<PathBuf, String> {
    let clone_dir = cache_dir.join(format!("_git_{}", name));

    if clone_dir.join(".git").exists() {
        let output = Command::new("git")
            .args(["pull", "--quiet"])
            .current_dir(&clone_dir)
            .output()
            .map_err(|e| format!("git pull failed: {}", e))?;
        if !output.status.success() {
            return Err(format!("git pull failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
    } else {
        let mut args = vec!["clone", "--quiet", "--depth", "1"];
        if let Some(b) = branch { args.push("-b"); args.push(b); }
        args.push(url);
        args.push(clone_dir.to_str().unwrap_or(""));
        let output = Command::new("git")
            .args(&args)
            .output()
            .map_err(|e| format!("git clone failed: {}", e))?;
        if !output.status.success() {
            return Err(format!("git clone failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
    }

    let hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&clone_dir)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // The package files live at the subdir (or the repo root).
    let pkg_src = match subdir {
        Some(s) => clone_dir.join(s),
        None => clone_dir.clone(),
    };
    if !pkg_src.exists() {
        return Err(format!("package '{}': subdir '{}' not found in {}",
            name, subdir.unwrap_or("."), url));
    }
    let dest = cache_dir.join(name);
    let files = copy_cell_files(&pkg_src, &dest)?;

    let source = match subdir {
        Some(s) => format!("git:{}#{}", url, s),
        None => format!("git:{}", url),
    };
    lock.packages.insert(name.to_string(), LockedPackage {
        name: name.to_string(),
        version: version.to_string(),
        source: source.clone(),
        hash,
        files,
    });

    eprintln!("  {} {} ({})", name, version, source);
    Ok(dest)
}

/// Resolve a named dependency through the sparse HTTP registry.
/// GET `<registry>/<name>.json` → { versions: { "x.y.z": {git, subdir,
/// branch} }, latest }. A "*" requirement takes `latest`; an exact
/// version takes that key.
fn resolve_from_registry(
    name: &str,
    requirement: &str,
    cache_dir: &Path,
    lock: &mut LockFile,
) -> Result<PathBuf, String> {
    let base = super::manifest::registry_url();
    let url = format!("{}/{}.json", base.trim_end_matches('/'), name);
    eprintln!("  resolving {} from {}", name, base);

    let body: serde_json::Value = ureq::get(&url)
        .call()
        .map_err(|e| format!("registry: cannot fetch {} ({}). Is the package name right?", url, e))?
        .into_json()
        .map_err(|e| format!("registry: {} returned invalid JSON: {}", url, e))?;

    let versions = body.get("versions").and_then(|v| v.as_object())
        .ok_or_else(|| format!("registry: {} has no 'versions'", name))?;

    // Parse the requirement as a semver range. A bare "0.1.0" is caret
    // (^0.1.0) per Cargo; "=0.1.0" pins exactly; "*" matches anything.
    let req_str = if requirement.is_empty() { "*" } else { requirement };
    let req = semver::VersionReq::parse(req_str)
        .map_err(|e| format!("invalid version requirement '{}' for {}: {}", req_str, name, e))?;

    // Published versions that are valid semver, ascending.
    let mut candidates: Vec<semver::Version> = versions.keys()
        .filter_map(|k| semver::Version::parse(k).ok())
        .collect();
    candidates.sort();

    // Highest published version satisfying the range.
    let chosen = candidates.iter().rev()
        .find(|v| req.matches(v))
        .map(|v| v.to_string())
        .ok_or_else(|| {
            let avail: Vec<String> = candidates.iter().map(|v| v.to_string()).collect();
            format!("registry: no version of {} matches '{}' (available: {})",
                name, req_str, if avail.is_empty() { "none".into() } else { avail.join(", ") })
        })?;

    let entry = &versions[&chosen];
    let git = entry.get("git").and_then(|v| v.as_str())
        .ok_or_else(|| format!("registry: {}@{} has no git source", name, chosen))?;
    let subdir = entry.get("subdir").and_then(|v| v.as_str());
    let branch = entry.get("branch").and_then(|v| v.as_str());

    resolve_git_with(name, git, branch, subdir, &chosen, cache_dir, lock)
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
