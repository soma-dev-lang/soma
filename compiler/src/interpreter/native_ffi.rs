//! Compile [native] handlers to shared libraries and load them via FFI.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::ast::*;
use crate::checker::native::{check_native_handler, NativeSiblings};
use crate::codegen::native::{self, NativeHandler, NativeSig, NativeType};

/// A loaded native function, ready to call.
pub struct LoadedNative {
    /// Keep the library alive as long as the function pointer is in use
    pub lib: Option<std::sync::Arc<libloading::Library>>,
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
    compile_and_load_natives_with_config(program, &native::ParallelConfig::default())
}

pub fn compile_and_load_natives_with_config(
    program: &Program,
    parallel_config: &native::ParallelConfig,
) -> Result<HashMap<(String, String), LoadedNative>, String> {
    let mut result = HashMap::new();

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
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
                        properties: on.properties.clone(),
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
        let (rust_source, sigs) = native::generate_native_source_with_config(&native_handlers, parallel_config);

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

            // Compile with cargo (enables num-bigint for BigInt support)
            eprintln!("[native] compiling {} handler(s) for cell '{}'...",
                native_handlers.len(), cell.node.name);

            // Create a mini cargo project for this native compilation
            let proj_dir = cache_dir().join("proj");
            let proj_src = proj_dir.join("src");
            std::fs::create_dir_all(&proj_src).ok();

            // Write Cargo.toml — only include rug if a Rug-mode handler is present
            let uses_rug = rust_source.contains("use rug::Integer");
            let deps = if uses_rug {
                "rug = { version = \"1\", default-features = false, features = [\"integer\"] }\n"
            } else {
                ""
            };
            let cargo_toml = format!(
                "[package]\nname = \"soma_native\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n{}\n[profile.release]\nopt-level = 3\noverflow-checks = true\npanic = \"unwind\"\n",
                deps
            );
            std::fs::write(proj_dir.join("Cargo.toml"), &cargo_toml)
                .map_err(|e| format!("cannot write Cargo.toml: {}", e))?;

            // Write the generated source as lib.rs
            std::fs::write(proj_src.join("lib.rs"), &rust_source)
                .map_err(|e| format!("cannot write lib.rs: {}", e))?;

            // Build with cargo
            let output = std::process::Command::new("cargo")
                .args(["build", "--release", "--quiet"])
                .current_dir(&proj_dir)
                .output()
                .map_err(|e| format!("cargo not found: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!(
                    "[native] compilation failed for cell '{}':\n{}\n\nGenerated source:\n{}",
                    cell.node.name, stderr, rust_source
                ));
            }

            // Copy the built dylib to the cache
            let built_dylib = if cfg!(target_os = "macos") {
                proj_dir.join("target/release/libsoma_native.dylib")
            } else {
                proj_dir.join("target/release/libsoma_native.so")
            };
            std::fs::copy(&built_dylib, &dylib_path)
                .map_err(|e| format!("cannot copy dylib: {}", e))?;

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

            let raw_name = sig.fn_name
                .strip_prefix("handler_").unwrap_or(&sig.fn_name);
            let handler_name = raw_name
                .strip_suffix("_par_arr")
                .or_else(|| raw_name.strip_suffix("_par"))
                .or_else(|| raw_name.strip_suffix("_arr"))
                .unwrap_or(raw_name);
            let key = (cell.node.name.clone(), handler_name.to_string());

            // We need to keep the library alive. Since multiple functions share one lib,
            // we'll re-open the library for each (cheap on most OSes — same mapping).
            let lib_copy = unsafe {
                libloading::Library::new(&dylib_path)
                    .map_err(|e| format!("failed to reload native library: {}", e))?
            };

            result.insert(key, LoadedNative {
                lib: Some(std::sync::Arc::new(lib_copy)),
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

    let param_count = native.sig.param_types.len();

    if args.len() != param_count {
        return Err(format!(
            "[native] '{}' expects {} args, got {}",
            native.sig.fn_name, param_count, args.len()
        ));
    }

    if native.sig.uses_shared_args {
        // Shared buffer path: push args via _soma_push_*, call zero-arg handler
        return call_native_shared(native, args);
    }

    // Direct path: pass args through C ABI (for Float/Bool-returning handlers)
    let mut raw_args: Vec<u64> = Vec::new();
    for (i, (arg, expected_ty)) in args.iter().zip(native.sig.param_types.iter()).enumerate() {
        match expected_ty {
            NativeType::Int => {
                let val = match arg {
                    Value::Int(si) => si.to_i64().ok_or_else(|| format!(
                        "[native] '{}' arg {} is a BigInt that doesn't fit i64; \
                         the handler runs in Direct mode (i64). Mark it [native, bigint] \
                         or use a String return type to opt into Rug mode.",
                        native.sig.fn_name, i
                    ))?,
                    Value::Float(f) => *f as i64,
                    Value::Bool(b) => if *b { 1 } else { 0 },
                    _ => return Err(format!("[native] arg {} must be Int", i)),
                };
                raw_args.push(val as u64);
            }
            NativeType::Float => {
                let val = match arg {
                    Value::Float(f) => *f,
                    Value::Int(si) => si.to_f64(),
                    _ => return Err(format!("[native] arg {} must be Float", i)),
                };
                raw_args.push(val.to_bits());
            }
            NativeType::Bool => {
                let val = match arg {
                    Value::Bool(b) => *b,
                    Value::Int(si) => si.to_i64().unwrap_or(0) != 0,
                    _ => return Err(format!("[native] arg {} must be Bool", i)),
                };
                raw_args.push(if val { 1 } else { 0 });
            }
            NativeType::String => {
                // String args go through shared buffer path, not direct call_raw
                return Err(format!("[native] arg {} is String — should use shared buffer path", i));
            }
        }
    }

    let raw_result: u64 = unsafe {
        call_raw(native.fn_ptr, &raw_args, &native.sig.param_types, native.sig.return_type)?
    };

    match native.sig.return_type {
        NativeType::Int => {
            let result_i64 = raw_result as i64;
            Ok(Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(result_i64)))
        }
        NativeType::Float => Ok(Value::Float(f64::from_bits(raw_result))),
        NativeType::Bool => Ok(Value::Bool(raw_result != 0)),
        NativeType::String => Ok(Value::String(String::new())), // shouldn't reach here
    }
}

/// Call a native handler via the shared arg buffer (supports BigInt).
fn call_native_shared(native: &LoadedNative, args: &[super::Value]) -> Result<super::Value, String> {
    use super::Value;

    let lib = native.lib.as_ref()
        .ok_or_else(|| "[native] shared call requires library".to_string())?;

    unsafe {
        // 1. Clear args
        let clear_fn: libloading::Symbol<unsafe extern "C" fn()> =
            lib.get(b"_soma_clear_args")
                .map_err(|e| format!("[native] missing _soma_clear_args: {}", e))?;
        clear_fn();

        // 2. Push each arg into the appropriate buffer
        let push_i64_fn: libloading::Symbol<unsafe extern "C" fn(i64)> =
            lib.get(b"_soma_push_i64")
                .map_err(|e| format!("[native] missing _soma_push_i64: {}", e))?;
        let push_bigint_fn: libloading::Symbol<unsafe extern "C" fn(*const u8, i64)> =
            lib.get(b"_soma_push_bigint")
                .map_err(|e| format!("[native] missing _soma_push_bigint: {}", e))?;
        let push_string_fn: Option<libloading::Symbol<unsafe extern "C" fn(*const u8, i64)>> =
            lib.get(b"_soma_push_string").ok();

        for (i, arg) in args.iter().enumerate() {
            match arg {
                Value::Int(si) => {
                    match si.to_i64() {
                        Some(v) => push_i64_fn(v),
                        None => {
                            // BigInt: serialize to string and push
                            let s = si.to_string();
                            push_bigint_fn(s.as_ptr(), s.len() as i64);
                        }
                    }
                }
                // Float pushed as bit-encoded i64 (handler decodes via from_bits)
                Value::Float(f) => push_i64_fn(f.to_bits() as i64),
                Value::Bool(b) => push_i64_fn(if *b { 1 } else { 0 }),
                Value::String(s) => {
                    if let Some(ref push) = push_string_fn {
                        push(s.as_ptr(), s.len() as i64);
                    } else {
                        return Err(format!("[native] arg {} is a String but the cell has no _soma_push_string (rebuild needed)", i));
                    }
                }
                _ => return Err(format!("[native] arg {} unsupported type", i)),
            }
        }

        // 3. Call the zero-arg handler
        let handler_fn: extern "C" fn() -> i64 = std::mem::transmute(native.fn_ptr);
        let result_i64 = handler_fn();

        // 4. Read result based on return type. Rug-mode handlers return
        // an i64 with these encodings (matching gen_rug_handler's emitter):
        //   String → result is i64::MIN+1, value in _SOMA_RESULT
        //   Int    → result is i64 if it fits, else i64::MIN with value in _SOMA_RESULT
        //   Float  → result is f64::to_bits(value) as i64
        //   Bool   → result is 0 or 1
        match native.sig.return_type {
            NativeType::String => {
                let s = read_shared_result(lib)?;
                Ok(Value::String(s))
            }
            NativeType::Int => {
                if result_i64 == i64::MIN {
                    let s = read_shared_result(lib)?;
                    Ok(Value::Int(crate::interpreter::soma_int::SomaInt::from_decimal_str(&s)))
                } else {
                    Ok(Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(result_i64)))
                }
            }
            NativeType::Float => {
                Ok(Value::Float(f64::from_bits(result_i64 as u64)))
            }
            NativeType::Bool => {
                Ok(Value::Bool(result_i64 != 0))
            }
        }
    }
}

/// Read string result from shared buffer (_soma_result_ptr/_soma_result_len)
unsafe fn read_shared_result(lib: &std::sync::Arc<libloading::Library>) -> Result<String, String> {
    let result_len_fn: libloading::Symbol<unsafe extern "C" fn() -> i64> =
        lib.get(b"_soma_result_len")
            .map_err(|e| format!("[native] missing _soma_result_len: {}", e))?;
    let result_ptr_fn: libloading::Symbol<unsafe extern "C" fn() -> *const u8> =
        lib.get(b"_soma_result_ptr")
            .map_err(|e| format!("[native] missing _soma_result_ptr: {}", e))?;

    let len = result_len_fn() as usize;
    let ptr = result_ptr_fn();

    if ptr.is_null() || len == 0 {
        return Ok(String::new());
    }

    let bytes = std::slice::from_raw_parts(ptr, len);
    String::from_utf8(bytes.to_vec())
        .map_err(|_| "[native] invalid UTF-8 in result".to_string())
}

/// Low-level function call dispatcher. Transmutes the function pointer based
/// on the exact signature and calls it.
///
/// Uses a System V / AArch64 fact: integer/pointer types and bool/i32/u32/i64
/// are all passed in integer registers and read with the same calling
/// convention. Floats go in separate FP registers. So for dispatch purposes
/// we only need to distinguish "int-like" (Int / Bool) from "float-like" (Float).
///
/// String params/returns never reach this path — they go through the shared
/// buffer FFI.
unsafe fn call_raw(
    fn_ptr: *const (),
    args: &[u64],
    param_types: &[NativeType],
    ret_type: NativeType,
) -> Result<u64, String> {
    // Normalize Bool to Int for dispatch — they share the same C ABI register class.
    let normalized: Vec<NativeType> = param_types.iter()
        .map(|t| if *t == NativeType::Bool { NativeType::Int } else { *t })
        .collect();
    let ret_norm = if ret_type == NativeType::Bool { NativeType::Int } else { ret_type };

    match args.len() {
        0 => match ret_norm {
            NativeType::Float => {
                let f: extern "C" fn() -> f64 = std::mem::transmute(fn_ptr);
                Ok(f().to_bits())
            }
            _ => {
                let f: extern "C" fn() -> i64 = std::mem::transmute(fn_ptr);
                Ok(f() as u64)
            }
        }
        1 => call_1(fn_ptr, args, &normalized, ret_norm),
        2 => call_2(fn_ptr, args, &normalized, ret_norm),
        3 => call_3(fn_ptr, args, &normalized, ret_norm),
        _ => call_generic(fn_ptr, args, &normalized, ret_type),
    }
}

/// 1-param dispatch. Bool is normalized to Int by the caller.
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
        (NativeType::Float, NativeType::Float) => {
            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0])).to_bits())
        }
        (NativeType::Float, NativeType::Int) => {
            let f: extern "C" fn(f64) -> i64 = std::mem::transmute(fn_ptr);
            Ok(f(f64::from_bits(args[0])) as u64)
        }
        _ => Err(format!("[native] unsupported 1-arg signature: ({:?}) -> {:?}", ptypes[0], ret)),
    }
}

/// 2-param dispatch — full {Int, Float}² × {Int, Float} table.
unsafe fn call_2(fn_ptr: *const (), args: &[u64], ptypes: &[NativeType], ret: NativeType) -> Result<u64, String> {
    macro_rules! call {
        ($($t:ty),+; $r:ty; $($a:expr),+) => {{
            let f: extern "C" fn($($t),+) -> $r = std::mem::transmute(fn_ptr);
            f($($a),+)
        }}
    }
    let a0 = args[0];
    let a1 = args[1];
    let i0 = a0 as i64;
    let i1 = a1 as i64;
    let f0 = f64::from_bits(a0);
    let f1 = f64::from_bits(a1);
    use NativeType::{Int as I, Float as F};
    Ok(match (ptypes[0], ptypes[1], ret) {
        (I, I, I) => call!(i64, i64; i64; i0, i1) as u64,
        (I, I, F) => call!(i64, i64; f64; i0, i1).to_bits(),
        (I, F, I) => call!(i64, f64; i64; i0, f1) as u64,
        (I, F, F) => call!(i64, f64; f64; i0, f1).to_bits(),
        (F, I, I) => call!(f64, i64; i64; f0, i1) as u64,
        (F, I, F) => call!(f64, i64; f64; f0, i1).to_bits(),
        (F, F, I) => call!(f64, f64; i64; f0, f1) as u64,
        (F, F, F) => call!(f64, f64; f64; f0, f1).to_bits(),
        _ => return Err(format!("[native] unsupported 2-arg signature: ({:?}, {:?}) -> {:?}",
                                ptypes[0], ptypes[1], ret)),
    })
}

/// 3-param dispatch — full {Int, Float}³ × {Int, Float} table.
unsafe fn call_3(fn_ptr: *const (), args: &[u64], ptypes: &[NativeType], ret: NativeType) -> Result<u64, String> {
    macro_rules! call {
        ($($t:ty),+; $r:ty; $($a:expr),+) => {{
            let f: extern "C" fn($($t),+) -> $r = std::mem::transmute(fn_ptr);
            f($($a),+)
        }}
    }
    let i0 = args[0] as i64;
    let i1 = args[1] as i64;
    let i2 = args[2] as i64;
    let f0 = f64::from_bits(args[0]);
    let f1 = f64::from_bits(args[1]);
    let f2 = f64::from_bits(args[2]);
    use NativeType::{Int as I, Float as F};
    Ok(match (ptypes[0], ptypes[1], ptypes[2], ret) {
        (I, I, I, I) => call!(i64, i64, i64; i64; i0, i1, i2) as u64,
        (I, I, I, F) => call!(i64, i64, i64; f64; i0, i1, i2).to_bits(),
        (I, I, F, I) => call!(i64, i64, f64; i64; i0, i1, f2) as u64,
        (I, I, F, F) => call!(i64, i64, f64; f64; i0, i1, f2).to_bits(),
        (I, F, I, I) => call!(i64, f64, i64; i64; i0, f1, i2) as u64,
        (I, F, I, F) => call!(i64, f64, i64; f64; i0, f1, i2).to_bits(),
        (I, F, F, I) => call!(i64, f64, f64; i64; i0, f1, f2) as u64,
        (I, F, F, F) => call!(i64, f64, f64; f64; i0, f1, f2).to_bits(),
        (F, I, I, I) => call!(f64, i64, i64; i64; f0, i1, i2) as u64,
        (F, I, I, F) => call!(f64, i64, i64; f64; f0, i1, i2).to_bits(),
        (F, I, F, I) => call!(f64, i64, f64; i64; f0, i1, f2) as u64,
        (F, I, F, F) => call!(f64, i64, f64; f64; f0, i1, f2).to_bits(),
        (F, F, I, I) => call!(f64, f64, i64; i64; f0, f1, i2) as u64,
        (F, F, I, F) => call!(f64, f64, i64; f64; f0, f1, i2).to_bits(),
        (F, F, F, I) => call!(f64, f64, f64; i64; f0, f1, f2) as u64,
        (F, F, F, F) => call!(f64, f64, f64; f64; f0, f1, f2).to_bits(),
        _ => return Err(format!("[native] unsupported 3-arg signature: ({:?}, {:?}, {:?}) -> {:?}",
                                ptypes[0], ptypes[1], ptypes[2], ret)),
    })
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
            NativeType::String => float_args.push(0.0),
        }
    }

    // Call as fn(*const f64, i64) -> f64
    let f: extern "C" fn(*const f64, i64) -> f64 = std::mem::transmute(fn_ptr);
    let result = f(float_args.as_ptr(), float_args.len() as i64);

    match ret_type {
        NativeType::Float => Ok(result.to_bits()),
        NativeType::Int => Ok(result as i64 as u64),
        NativeType::Bool => Ok(if result != 0.0 { 1 } else { 0 }),
        NativeType::String => Ok(0),
    }
}
