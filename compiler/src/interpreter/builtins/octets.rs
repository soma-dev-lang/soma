//! Bytes as hexadecimal text: `hex_encode`, `hmac_hex`, `base32_encode`, `base32_decode`. What a
//! program needs to verify a one-time code from an authenticator app (RFC 6238, RFC 4648):
//! Soma's strings are UTF-8 text, so bytes travel as hex.

use super::super::{RuntimeError, Value};

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "hex_encode" => Some(match args {
            [Value::String(s)] => Ok(Value::String(hex(s.as_bytes()))),
            _ => Err(RuntimeError::TypeError("hex_encode(text: String) — the UTF-8 bytes of the text, as hex".to_string())),
        }),
        "hmac_hex" => Some(match args {
            [Value::String(algo), Value::String(key), Value::String(msg)] => hmac_hex(algo, key, msg),
            _ => Err(RuntimeError::TypeError("hmac_hex(algo: \"sha1\" | \"sha256\" | \"sha512\", key_hex: String, message_hex: String)".to_string())),
        }),
        "base32_encode" => Some(match args {
            [Value::String(h)] => match unhex(h) {
                Some(b) => Ok(Value::String(base32_encode(&b))),
                None => Err(RuntimeError::TypeError(format!("base32_encode: `{}` is not hexadecimal bytes", h))),
            },
            _ => Err(RuntimeError::TypeError("base32_encode(bytes_hex: String)".to_string())),
        }),
        "base32_decode" => Some(match args {
            [Value::String(s)] => Ok(base32_decode(s).map(|b| Value::String(hex(&b))).unwrap_or(Value::Unit)),
            _ => Err(RuntimeError::TypeError("base32_decode(text: String) -> hex or ()".to_string())),
        }),
        _ => None,
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 { return None; }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn hmac_hex(algo: &str, key: &str, msg: &str) -> Result<Value, RuntimeError> {
    use hmac::Mac;
    let k = unhex(key).ok_or_else(|| RuntimeError::TypeError(format!("hmac_hex: the key `{}` is not hexadecimal bytes", key)))?;
    let m = unhex(msg).ok_or_else(|| RuntimeError::TypeError(format!("hmac_hex: the message `{}` is not hexadecimal bytes", msg)))?;
    let out = match algo {
        "sha1" => { let mut h = <hmac::Hmac<sha1::Sha1> as Mac>::new_from_slice(&k).expect("any key"); h.update(&m); h.finalize().into_bytes().to_vec() }
        "sha256" => { let mut h = <hmac::Hmac<sha2::Sha256> as Mac>::new_from_slice(&k).expect("any key"); h.update(&m); h.finalize().into_bytes().to_vec() }
        "sha512" => { let mut h = <hmac::Hmac<sha2::Sha512> as Mac>::new_from_slice(&k).expect("any key"); h.update(&m); h.finalize().into_bytes().to_vec() }
        other => return Err(RuntimeError::TypeError(format!("hmac_hex: unknown algorithm `{}` (sha1, sha256, sha512)", other))),
    };
    Ok(Value::String(hex(&out)))
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32_encode(b: &[u8]) -> String {
    let mut out = String::new();
    let mut buf: u64 = 0;
    let mut bits = 0;
    for &x in b {
        buf = (buf << 8) | x as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 { out.push(ALPHABET[((buf << (5 - bits)) & 31) as usize] as char); }
    while out.len() % 8 != 0 { out.push('='); }
    out
}

/// Case-insensitive; spaces, hyphens and padding are ignored; any other character is ().
fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u64 = 0;
    let mut bits = 0;
    for c in s.chars() {
        if c == ' ' || c == '-' || c == '=' { continue; }
        let v = ALPHABET.iter().position(|&a| a as char == c.to_ascii_uppercase())? as u64;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 255) as u8);
        }
    }
    Some(out)
}
