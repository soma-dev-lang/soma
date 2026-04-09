// CLBG spectral-norm — Rust reference (sequential, no Rayon).
// Same algorithm as the Soma version: compute sqrt(<u, v> / <v, v>)
// after 10 power iterations of (A^T A).

use std::env;
use std::time::Instant;

#[inline]
fn a(i: usize, j: usize) -> f64 {
    let k = (i + j) as f64;
    1.0 / (k * (k + 1.0) * 0.5 + (i as f64) + 1.0)
}

fn mul_av(n: usize, u: &[f64], v: &mut [f64]) {
    for i in 0..n {
        let mut s = 0.0;
        for j in 0..n {
            s += a(i, j) * u[j];
        }
        v[i] = s;
    }
}

fn mul_atv(n: usize, u: &[f64], v: &mut [f64]) {
    for i in 0..n {
        let mut s = 0.0;
        for j in 0..n {
            s += a(j, i) * u[j];
        }
        v[i] = s;
    }
}

fn mul_atav(n: usize, u: &[f64], v: &mut [f64], t: &mut [f64]) {
    mul_av(n, u, t);
    mul_atv(n, t, v);
}

fn main() {
    let n: usize = env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(5500);
    let mut u = vec![1.0f64; n];
    let mut v = vec![0.0f64; n];
    let mut t = vec![0.0f64; n];

    let t0 = Instant::now();
    for _ in 0..10 {
        mul_atav(n, &u, &mut v, &mut t);
        mul_atav(n, &v, &mut u, &mut t);
    }
    let mut vbv = 0.0;
    let mut vv = 0.0;
    for k in 0..n {
        vbv += u[k] * v[k];
        vv += v[k] * v[k];
    }
    let result = (vbv / vv).sqrt();
    let elapsed = t0.elapsed();
    println!("spectral-norm N={} = {:.9}", n, result);
    println!("elapsed: {:?}", elapsed);
}
