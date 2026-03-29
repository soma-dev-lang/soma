use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::fs;

use super::bytecode::{Chunk, Constant};

const CACHE_DIR: &str = ".soma_cache";
const MAGIC: &[u8; 4] = b"SOMA";
const VERSION: u8 = 1;

/// Compute a hash of the source code
pub fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// Get the cache file path for a source hash
fn cache_path(hash: u64) -> PathBuf {
    PathBuf::from(CACHE_DIR).join(format!("{:016x}.bin", hash))
}

/// Try to load cached bytecode for the given source
pub fn load_cached(source: &str) -> Option<Vec<Chunk>> {
    let hash = hash_source(source);
    let path = cache_path(hash);

    if !path.exists() {
        return None;
    }

    let data = fs::read(&path).ok()?;
    deserialize_chunks(&data)
}

/// Save compiled bytecode to cache
pub fn save_cache(source: &str, chunks: &[Chunk]) {
    let hash = hash_source(source);
    let path = cache_path(hash);

    let _ = fs::create_dir_all(CACHE_DIR);
    let data = serialize_chunks(chunks);
    let _ = fs::write(&path, data);
}

// ── Serialization ────────────────────────────────────────────────────
// Simple binary format:
//   MAGIC (4 bytes) | VERSION (1) | num_chunks (u32)
//   For each chunk:
//     cell_name_len (u16) | cell_name | signal_name_len (u16) | signal_name
//     num_locals (u16) | [local_name_len (u16) | local_name]*
//     num_constants (u16) | [constant_type (u8) | constant_data]*
//     code_len (u32) | code_bytes

fn serialize_chunks(chunks: &[Chunk]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4096);

    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    write_u32(&mut buf, chunks.len() as u32);

    for chunk in chunks {
        // Cell name
        write_str(&mut buf, &chunk.cell_name);
        // Signal name
        write_str(&mut buf, &chunk.signal_name);
        // Locals
        write_u16(&mut buf, chunk.locals.len() as u16);
        for local in &chunk.locals {
            write_str(&mut buf, local);
        }
        // Constants — skip LambdaAst (not serializable); if any exist, don't cache
        let has_ast = chunk.constants.iter().any(|c| matches!(c, Constant::LambdaAst { .. } | Constant::TryAst(_)));
        if has_ast {
            // Can't cache chunks with AST nodes (lambda/try); return empty to signal no-cache
            return Vec::new();
        }
        write_u16(&mut buf, chunk.constants.len() as u16);
        for constant in &chunk.constants {
            match constant {
                Constant::Int(n) => {
                    buf.push(0);
                    write_i64(&mut buf, *n);
                }
                Constant::Float(n) => {
                    buf.push(1);
                    write_f64(&mut buf, *n);
                }
                Constant::String(s) => {
                    buf.push(2);
                    write_str(&mut buf, s);
                }
                Constant::Name(s) => {
                    buf.push(3);
                    write_str(&mut buf, s);
                }
                Constant::LambdaAst { .. } | Constant::TryAst(_) => unreachable!(),
            }
        }
        // Code
        write_u32(&mut buf, chunk.code.len() as u32);
        buf.extend_from_slice(&chunk.code);
    }

    buf
}

fn deserialize_chunks(data: &[u8]) -> Option<Vec<Chunk>> {
    let mut pos = 0;

    // Magic
    if data.len() < 5 || &data[0..4] != MAGIC {
        return None;
    }
    pos += 4;

    // Version
    if data[pos] != VERSION {
        return None;
    }
    pos += 1;

    let num_chunks = read_u32(data, &mut pos)?;
    let mut chunks = Vec::with_capacity(num_chunks as usize);

    for _ in 0..num_chunks {
        let cell_name = read_str(data, &mut pos)?;
        let signal_name = read_str(data, &mut pos)?;

        let num_locals = read_u16(data, &mut pos)?;
        let mut locals = Vec::with_capacity(num_locals as usize);
        for _ in 0..num_locals {
            locals.push(read_str(data, &mut pos)?);
        }

        let num_constants = read_u16(data, &mut pos)?;
        let mut constants = Vec::with_capacity(num_constants as usize);
        for _ in 0..num_constants {
            if pos >= data.len() { return None; }
            let tag = data[pos];
            pos += 1;
            let c = match tag {
                0 => Constant::Int(read_i64(data, &mut pos)?),
                1 => Constant::Float(read_f64(data, &mut pos)?),
                2 => Constant::String(read_str(data, &mut pos)?),
                3 => Constant::Name(read_str(data, &mut pos)?),
                _ => return None,
            };
            constants.push(c);
        }

        let code_len = read_u32(data, &mut pos)? as usize;
        if pos + code_len > data.len() { return None; }
        let code = data[pos..pos + code_len].to_vec();
        pos += code_len;

        chunks.push(Chunk {
            code,
            constants,
            locals,
            cell_name,
            signal_name,
        });
    }

    Some(chunks)
}

// ── Binary helpers ───────────────────────────────────────────────────

fn write_u16(buf: &mut Vec<u8>, val: u16) {
    buf.push((val >> 8) as u8);
    buf.push((val & 0xff) as u8);
}

fn write_u32(buf: &mut Vec<u8>, val: u32) {
    buf.extend_from_slice(&val.to_be_bytes());
}

fn write_i64(buf: &mut Vec<u8>, val: i64) {
    buf.extend_from_slice(&val.to_be_bytes());
}

fn write_f64(buf: &mut Vec<u8>, val: f64) {
    buf.extend_from_slice(&val.to_be_bytes());
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u16(buf, s.len() as u16);
    buf.extend_from_slice(s.as_bytes());
}

fn read_u16(data: &[u8], pos: &mut usize) -> Option<u16> {
    if *pos + 2 > data.len() { return None; }
    let val = ((data[*pos] as u16) << 8) | (data[*pos + 1] as u16);
    *pos += 2;
    Some(val)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > data.len() { return None; }
    let val = u32::from_be_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]);
    *pos += 4;
    Some(val)
}

fn read_i64(data: &[u8], pos: &mut usize) -> Option<i64> {
    if *pos + 8 > data.len() { return None; }
    let val = i64::from_be_bytes([
        data[*pos], data[*pos+1], data[*pos+2], data[*pos+3],
        data[*pos+4], data[*pos+5], data[*pos+6], data[*pos+7],
    ]);
    *pos += 8;
    Some(val)
}

fn read_f64(data: &[u8], pos: &mut usize) -> Option<f64> {
    if *pos + 8 > data.len() { return None; }
    let val = f64::from_be_bytes([
        data[*pos], data[*pos+1], data[*pos+2], data[*pos+3],
        data[*pos+4], data[*pos+5], data[*pos+6], data[*pos+7],
    ]);
    *pos += 8;
    Some(val)
}

fn read_str(data: &[u8], pos: &mut usize) -> Option<String> {
    let len = read_u16(data, pos)? as usize;
    if *pos + len > data.len() { return None; }
    let s = String::from_utf8(data[*pos..*pos + len].to_vec()).ok()?;
    *pos += len;
    Some(s)
}
