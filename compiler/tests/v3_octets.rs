//! HMAC over bytes and base32: what a program needs to verify a one-time code from an
//! authenticator app (RFC 6238). Bytes travel as hexadecimal text: `hex_encode` turns a text's
//! UTF-8 bytes into hex, `hmac_hex(algo, key_hex, message_hex)` signs bytes, `base32_encode` and
//! `base32_decode` (RFC 4648) carry the shared secret.

use std::process::Command;

fn soma(dir: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir).output().expect("soma runs");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_octets_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

const PROG: &str = r#"cell T {
    // RFC 6238 : HOTP du pas `t` (8 octets, gros-boutiste), troncature dynamique, `chiffres` chiffres
    on code(algo: String, cle_hex: String, t: Int, chiffres: Int) {
        let compteur = pad_left(_hex(t), 16, "0")
        let h = hmac_hex(algo, cle_hex, compteur)
        let o = parse_int(substring(h, len(h) - 1, len(h)), 16)
        let n = band(parse_int(substring(h, o * 2, o * 2 + 8), 16), 2147483647)
        return pad_left(to_string(n % ipow(10, chiffres)), chiffres, "0")
    }
    on _hex(n: Int) {
        let chiffres = "0123456789abcdef"
        let r = ""
        let x = n
        while x > 0 {
            r = substring(chiffres, x % 16, x % 16 + 1) + r
            x = idiv(x, 16)
        }
        return if r == "" { "0" } else { r }
    }
    on vecteurs() {
        let k1 = hex_encode("12345678901234567890")
        let k256 = hex_encode("12345678901234567890123456789012")
        let k512 = hex_encode("1234567890123456789012345678901234567890123456789012345678901234")
        return [code("sha1", k1, 1, 8), code("sha256", k256, 1, 8), code("sha512", k512, 1, 8), code("sha1", k1, 37037036, 8), code("sha1", k1, idiv(20000000000, 30), 8)]
    }
    on base32() {
        let s = base32_encode(hex_encode("12345678901234567890"))
        return [s, base32_decode(s) == hex_encode("12345678901234567890"), base32_decode("gezd gnbv gy3t qojq gezd gnbv gy3t qojq") == hex_encode("12345678901234567890"), base32_decode("1!!")]
    }
    on erreurs() {
        let a = try { hmac_hex("md5", "00", "00") }
        let b = try { hmac_hex("sha1", "zz", "00") }
        return [a.kind, b.kind]
    }
}
"#;

#[test]
fn hmac_hex_gives_the_rfc_6238_one_time_codes() {
    let dir = project("totp", PROG);
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "vecteurs"]);
    assert_eq!(code, 0, "{out}");
    // RFC 6238, appendix B: T = 59 s (step 1) for SHA-1, SHA-256, SHA-512; T = 1111111109 s; T = 20000000000 s
    for want in ["94287082", "46119246", "90693936", "07081804", "65353130"] {
        assert!(out.contains(want), "missing {want}: {out}");
    }
}

#[test]
fn base32_round_trips_the_shared_secret() {
    let dir = project("base32", PROG);
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "base32"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"), "{out}");
    assert!(out.contains("true, true"), "{out}");
    // an invalid text decodes to ()
    assert!(out.contains("()") || out.contains("null"), "{out}");
}

#[test]
fn hmac_hex_refuses_an_unknown_algorithm_and_bad_hex() {
    let dir = project("erreurs", PROG);
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "erreurs"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.matches("type").count() >= 2, "{out}");
}
