// CLBG fannkuch-redux — Rust reference (sequential, no parallelism).
// Same algorithm as the Soma version: enumerate permutations,
// count flips, track max + checksum.

use std::env;
use std::time::Instant;

fn solve(n: usize) -> (i64, i64) {
    let mut perm = vec![0i64; n];
    let mut perm1: Vec<i64> = (0..n as i64).collect();
    let mut count = vec![0i64; n];
    let mut max_flips = 0i64;
    let mut checksum = 0i64;
    let mut r = n;
    let mut permutation_count = 0i64;

    loop {
        while r != 1 {
            count[r - 1] = r as i64;
            r -= 1;
        }
        for i in 0..n {
            perm[i] = perm1[i];
        }
        let mut flips = 0i64;
        let mut k = perm[0];
        while k != 0 {
            let mut j = 0;
            let mut m = k as usize;
            while j < m {
                perm.swap(j, m);
                j += 1;
                m -= 1;
            }
            flips += 1;
            k = perm[0];
        }
        if flips > max_flips { max_flips = flips; }
        if permutation_count % 2 == 0 {
            checksum += flips;
        } else {
            checksum -= flips;
        }
        permutation_count += 1;

        loop {
            if r == n {
                return (max_flips, checksum);
            }
            let p0 = perm1[0];
            for i in 0..r {
                perm1[i] = perm1[i + 1];
            }
            perm1[r] = p0;
            count[r] -= 1;
            if count[r] > 0 { break; }
            r += 1;
        }
    }
}

fn main() {
    let n: usize = env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(12);
    let t0 = Instant::now();
    let (m, c) = solve(n);
    let elapsed = t0.elapsed();
    println!("fannkuch N={} max={} checksum={}", n, m, c);
    println!("elapsed: {:?}", elapsed);
}
