//! Compile [native] handlers to shared libraries and load them via FFI.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::ast::*;
use crate::checker::native::{check_native_handler, NativeSiblings};
use crate::codegen::native::{self, NativeHandler, NativeSig, NativeType};

/// A loaded native function, ready to call.
pub struct LoadedNative {
    /// Keep the library alive as long as the function pointer is in use
    _lib: libloading::Library,
    pub sig: NativeSig,
    /// Raw function pointer — we'll transmute based on sig at call time
    pub fn_ptr: *const (),
}

// Safety: we control the lifecycle — library stays alive, function is extern "C"
unsafe impl Send for LoadedNative {}
unsafe impl Sync for LoadedNative {}

/// Cache directory for compiled native dylibs
fn cache_dir() -> PathBuf {
    let dir = PathBuf::from(".soma_cache/native");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Hash the Rust source to use as cache key
fn hash_source(source: &str) -> String {
    // Simple FNV-1a hash
    let mut h: u64 = 0xcbf29ce484222325;
    for b in source.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

/// Shared library extension for the current platform
fn dylib_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(target_os = "windows") {
        "dll"
    } else {
        "so"
    }
}

/// Scan a program for [native] handlers, validate, codegen, compile, and load them.
/// Returns a map from (cell_name, signal_name) → LoadedNative.
pub fn compile_and_load_natives(program: &Program) -> Result<HashMap<(String, String), LoadedNative>, String> {
    let mut result = HashMap::new();

    for cell in &program.cells {
        if cell.node.kind != CellKind::Cell {
            continue;
        }

        // Collect native handlers in this cell
        let mut native_handlers: Vec<NativeHandler> = Vec::new();
        let mut native_names = NativeSiblings::new();

        for section in &cell.node.sections {
            if let Section::OnSignal(ref on) = section.node {
                if on.properties.contains(&"native".to_string()) {
                    native_names.insert(on.signal_name.clone());
                    native_handlers.push(NativeHandler {
                        cell_name: cell.node.name.clone(),
                        signal_name: on.signal_name.clone(),
                        params: on.params.clone(),
                        body: on.body.clone(),
                    });
                }
            }
        }

        if native_handlers.is_empty() {
            continue;
        }

        // Step 1: Validate each native handler
        for handler in &native_handlers {
            check_native_handler(
                &handler.signal_name,
                &handler.params,
                &handler.body,
                &native_names,
            ).map_err(|e| e.to_string())?;
        }

        // Step 2: Generate Rust source
        let (rust_source, sigs) = native::generate_native_source(&native_handlers);

        // Step 3: Check cache
        let source_hash = hash_source(&rust_source);
        let cache = cache_dir();
        let dylib_name = format!("native_{}_{}.{}", cell.node.name.to_lowercase(), source_hash, dylib_ext());
        let dylib_path = cache.join(&dylib_name);

        if !dylib_path.exists() {
            // Write Rust source
            let rs_path = cache.join(format!("native_{}_{}.rs", cell.node.name.to_lowercase(), source_hash));
            std::fs::write(&rs_path, &rust_source)
                .map_err(|e| format!("failed to write native source: {}", e))?;

            // Compile with rustc
            eprintln!("[native] compiling {} handler(s) for cell '{}'...",
                native_handlers.len(), cell.node.name);

            let output = std::process::Command::new("rustc")
                .args([
                    "--crate-type=cdylib",
                    "-O",
                    rs_path.to_str().unwrap(),
                    "-o",
                    dylib_path.to_str().unwrap(),
                ])
                .output()
                .map_err(|e| format!("rustc not found: {} — install Rust to use [native] handlers", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "[native] compilation failed for cell '{}':\n{}\n\nGenerated source:\n{}",
                    cell.node.name, stderr, rust_source
                ));
            }

            eprintln!("[native] compiled → {}", dylib_path.display());
        } else {
            eprintln!("[native] using cached {} for cell '{}'", dylib_name, cell.node.name);
        }

        // Step 4: Load the shared library
        let lib = unsafe {
            libloading::Library::new(&dylib_path)
                .map_err(|e| format!("failed to load native library: {}", e))?
        };

        // Step 5: Resolve function symbols
        for sig in sigs {
            let fn_ptr = unsafe {
                let sym: libloading::Symbol<*const ()> = lib.get(sig.fn_name.as_bytes())
                    .map_err(|e| format!("failed to find symbol '{}': {}", sig.fn_name, e))?;
                *sym
            };

            let handler_name = sig.fn_name
                .strip_prefix("handler_").unwrap_or(&sig.fn_name)
                .strip_suffix("_arr").unwrap_or(
                    sig.fn_name.strip_prefix("handler_").unwrap_or(&sig.fn_name)
                );
            let key = (cell.node.name.clone(), handler_name.to_string());

            // We need to keep the library alive. Since multiple functions share one lib,
            // we'll re-open the library for each (cheap on most OSes — same mapping).
            let lib_copy = unsafe {
                libloading::Library::new(&dylib_path)
                    .map_err(|e| format!("failed to reload native library: {}", e))?
            };

            result.insert(key, LoadedNative {
                _lib: lib_copy,
                sig,
                fn_ptr,
            });
        }
    }

    Ok(result)
}

/// Call a loaded native function with the given interpreter Values.
/// Returns the result as a Value.
pub fn call_native(native: &LoadedNative, args: &[super::Value]) -> Result<super::Value, String> {
    use super::Value;

    // We need to build the right function pointer type based on the signature.
    // Support up to 8 params of i64 or f64.
    let param_count = native.sig.param_types.len();

    if args.len() != param_count {
        return Err(format!(
            "[native] '{}' expects {} args, got {}",
            native.sig.fn_name, param_count, args.len()
        ));
    }

    // Convert each arg to the expected type
    let mut raw_args: Vec<u64> = Vec::new(); // store as u64 (both i64 and f64 are 8 bytes)
    for (i, (arg, expected_ty)) in args.iter().zip(native.sig.param_types.iter()).enumerate() {
        match expected_ty {
            NativeType::Int => {
                let val = match arg {
                    Value::Int(n) => *n,
                    Value::Float(f) => *f as i64,
                    Value::Bool(b) => if *b { 1 } else { 0 },
                    _ => return Err(format!("[native] arg {} must be Int", i)),
                };
                raw_args.push(val as u64);
            }
            NativeType::Float => {
                let val = match arg {
                    Value::Float(f) => *f,
                    Value::Int(n) => *n as f64,
                    _ => return Err(format!("[native] arg {} must be Float", i)),
                };
                raw_args.push(val.to_bits());
            }
            NativeType::Bool => {
                let val = match arg {
                    Value::Bool(b) => *b,
                    Value::Int(n) => *n != 0,
                    _ => return Err(format!("[native] arg {} must be Bool", i)),
                };
                raw_args.push(if val { 1 } else { 0 });
            }
        }
    }

    // Call via transmuted function pointer based on parameter count and types.
    // We use a macro-style dispatch for up to 8 parameters.
    let raw_result: u64 = unsafe {
        call_raw(native.fn_ptr, &raw_args, &native.sig.param_types, native.sig.return_type)?
    };

    // Convert result back to Value
    match native.sig.return_type {
        NativeType::Int => Ok(Value::Int(raw_result as i64)),
        NativeType::Float => Ok(Value::Float(f64::from_bits(raw_result))),
        NativeType::Bool => Ok(Value::Bool(raw_result != 0)),
    }
}

/// Low-level function call dispatcher. Transmutes the function pointer based on
/// the exact signature and calls it.
unsafe fn call_raw(
    fn_ptr: *const (),
    args: &[u64],
    param_types: &[NativeType],
    ret_type: NativeType,
) -> Result<u64, String> {
    // We handle common signatures explicitly for safety and correctness.
    // All params are either i64 or f64 (both 8 bytes in C ABI on 64-bit).

    // Generate dispatch for 0..8 params.
    // Since all params are i64/f64 which have the same ABI size, we can use
    // a unified i64-based calling convention on most platforms, BUT f64 goes
    // in float registers on System V AMD64 and ARM64. So we need type-aware dispatch.

    match args.len() {
        0 => {
            match ret_type {
                NativeType::Float => {
                    let f: extern "C" fn() -> f64 = std::mem::transmute(fn_ptr);
                    Ok(f().to_bits())
                }
                NativeType::Int => {
                    let f: extern "C" fn() -> i64 = std::mem::transmute(fn_ptr);
                    Ok(f() as u64)
                }
                NativeType::Bool => {
                    let f: extern "C" fn() -> i64 = std::mem::transmute(fn_ptr);
                    Ok(f() as u64)
                }
            }
        }
        1 => call_1(fn_ptr, args, param_types, ret_type),
        2 => call_2(fn_ptr, args, param_types, ret_type),
        3 => call_3(fn_ptr, args, param_types, ret_type),
        _ => {
            // Generic fallback: pack all args as f64 into an array, call via (ptr, count) -> f64
            // This works because our codegen generates a wrapper that unpacks
            // Actually, use direct transmute with known sizes up to 12 params
            call_generic(fn_ptr, args, param_types, ret_type)
        }
    }
}

/// 1-param dispatch
unsafe fn call_1(fn_ptr: *const (), args: &[u64], ptypes: &[NativeType], ret: NativeType) -> Result<u64, String> {
    match (ptypes[0], ret) {
        (NativeType::Int, NativeType::Int) => {
            let f: extern "C" fn(i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64) as u64)
        }
        (NativeType::Int, NativeType::Float) => {
            let f: extern "C" fn(i64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64).to_bits())
        }
        (NativeType::Int, NativeType::Bool) => {
            let f: extern "C" fn(i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64) as u64)
        }
        (NativeType::Float, NativeType::Float) => {
            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0])).to_bits())
        }
        (NativeType::Float, NativeType::Int) => {
            let f: extern "C" fn(f64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0])) as u64)
        }
        (NativeType::Bool, NativeType::Bool) => {
            let f: extern "C" fn(i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64) as u64)
        }
        _ => {
            // Generic fallback for remaining combos
            let f: extern "C" fn(i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64) as u64)
        }
    }
}

/// 2-param dispatch
unsafe fn call_2(fn_ptr: *const (), args: &[u64], ptypes: &[NativeType], ret: NativeType) -> Result<u64, String> {
    match (ptypes[0], ptypes[1], ret) {
        (NativeType::Int, NativeType::Int, NativeType::Int) => {
            let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, args[1] as i64) as u64)
        }
        (NativeType::Int, NativeType::Int, NativeType::Float) => {
            let f: extern "C" fn(i64, i64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, args[1] as i64).to_bits())
        }
        (NativeType::Float, NativeType::Float, NativeType::Float) => {
            let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0]), f64::from_bits(args[1])).to_bits())
        }
        (NativeType::Int, NativeType::Float, NativeType::Float) => {
            let f: extern "C" fn(i64, f64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, f64::from_bits(args[1])).to_bits())
        }
        (NativeType::Float, NativeType::Int, NativeType::Float) => {
            let f: extern "C" fn(f64, i64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0]), args[1] as i64).to_bits())
        }
        _ => {
            // Fallback: treat everything as i64
            let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, args[1] as i64) as u64)
        }
    }
}

/// 3-param dispatch
unsafe fn call_3(fn_ptr: *const (), args: &[u64], ptypes: &[NativeType], ret: NativeType) -> Result<u64, String> {
    // Common cases
    match (ptypes[0], ptypes[1], ptypes[2], ret) {
        (NativeType::Int, NativeType::Int, NativeType::Int, NativeType::Int) => {
            let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, args[1] as i64, args[2] as i64) as u64)
        }
        (NativeType::Int, NativeType::Int, NativeType::Int, NativeType::Float) => {
            let f: extern "C" fn(i64, i64, i64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, args[1] as i64, args[2] as i64).to_bits())
        }
        (NativeType::Float, NativeType::Float, NativeType::Float, NativeType::Float) => {
            let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0]), f64::from_bits(args[1]), f64::from_bits(args[2])).to_bits())
        }
        _ => {
            // Fallback: all i64
            let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(args[0] as i64, args[1] as i64, args[2] as i64) as u64)
        }
    }
}

/// Generic multi-param dispatch: converts all args to f64, passes as array pointer.
/// The generated Rust wrapper unpacks from *const f64.
unsafe fn call_generic(
    fn_ptr: *const (),
    args: &[u64],
    param_types: &[NativeType],
    ret_type: NativeType,
) -> Result<u64, String> {
    // Convert all args to f64 representation
    let mut float_args: Vec<f64> = Vec::with_capacity(args.len());
    for (i, (arg, ty)) in args.iter().zip(param_types.iter()).enumerate() {
        match ty {
            NativeType::Int => float_args.push(*arg as i64 as f64),
            NativeType::Float => float_args.push(f64::from_bits(*arg)),
            NativeType::Bool => float_args.push(if *arg != 0 { 1.0 } else { 0.0 }),
        }
    }

    // Call as fn(*const f64, i64) -> f64
    let f: extern "C" fn(*const f64, i64) -> f64 = std::mem::transmute(fn_ptr);
    let result = f(float_args.as_ptr(), float_args.len() as i64);

    match ret_type {
        NativeType::Float => Ok(result.to_bits()),
        NativeType::Int => Ok(result as i64 as u64),
        NativeType::Bool => Ok(if result != 0.0 { 1 } else { 0 }),
    }
}
