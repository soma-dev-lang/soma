// CLBG pidigits — Rust reference using `rug` (same backend as Soma).
use rug::Integer;
use std::env;
use std::time::Instant;

fn compute(target: usize) -> String {
    let mut q = Integer::from(1);
    let mut r = Integer::from(0);
    let mut s = Integer::from(0);
    let mut t = Integer::from(1);
    let mut k: u32 = 1;
    let mut digits = 0usize;
    let mut result = String::new();

    while digits <= target {
        let num: Integer = Integer::from(&q * 3) + &r;
        let den: Integer = Integer::from(&s * 3) + &t;
        let d: Integer = Integer::from(&num / &den);
        let num4: Integer = Integer::from(&q * 4) + &r;
        let den4: Integer = Integer::from(&s * 4) + &t;
        let d4: Integer = Integer::from(&num4 / &den4);
        if d == d4 {
            if digits == 0 {
                digits = 1;
            } else {
                result.push_str(&d.to_string());
                digits += 1;
            }
            let ds: Integer = Integer::from(&d * &s);
            let nq: Integer = Integer::from(Integer::from(&q - &ds) * 10);
            let dt: Integer = Integer::from(&d * &t);
            let nr: Integer = Integer::from(Integer::from(&r - &dt) * 10);
            q = nq;
            r = nr;
        } else {
            let u: Integer = Integer::from(4u32 * k + 2);
            let v: Integer = Integer::from(2u32 * k + 1);
            let nq: Integer = Integer::from(&q * k);
            let qu: Integer = Integer::from(&q * &u);
            let rv: Integer = Integer::from(&r * &v);
            let nr: Integer = Integer::from(&qu + &rv);
            let ns: Integer = Integer::from(&s * k);
            let su: Integer = Integer::from(&s * &u);
            let tv: Integer = Integer::from(&t * &v);
            let nt: Integer = Integer::from(&su + &tv);
            q = nq;
            r = nr;
            s = ns;
            t = nt;
            k += 1;
        }
    }
    result
}

fn main() {
    let n: usize = env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(10000);
    let t0 = Instant::now();
    let pi = compute(n);
    let elapsed = t0.elapsed();
    let last10 = &pi[pi.len()-10..];
    println!("pidigits N={} last10={}", n, last10);
    println!("elapsed: {:?}", elapsed);
}
