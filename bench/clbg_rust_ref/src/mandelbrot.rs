// CLBG mandelbrot — Rust reference (sequential, no SIMD intrinsics).
// Same algorithm as the Soma version. The published CLBG Rust uses
// hand-written SIMD; this version is a fair head-to-head with Soma.

use std::env;
use std::time::Instant;

fn count_inside(grid_n: usize, max_iter: usize) -> usize {
    let x_min: f64 = -2.0;
    let x_max: f64 = 1.0;
    let y_min: f64 = -1.5;
    let y_max: f64 = 1.5;
    let dx = (x_max - x_min) / grid_n as f64;
    let dy = (y_max - y_min) / grid_n as f64;
    let mut inside = 0usize;
    for py in 0..grid_n {
        let cy = y_min + py as f64 * dy;
        for px in 0..grid_n {
            let cx = x_min + px as f64 * dx;
            let mut zx = 0.0f64;
            let mut zy = 0.0f64;
            let mut zx2 = 0.0f64;
            let mut zy2 = 0.0f64;
            let mut iter = 0usize;
            while iter < max_iter {
                if zx2 + zy2 >= 4.0 {
                    iter = max_iter + 1;
                } else {
                    zy = 2.0 * zx * zy + cy;
                    zx = zx2 - zy2 + cx;
                    zx2 = zx * zx;
                    zy2 = zy * zy;
                    iter += 1;
                }
            }
            if iter == max_iter {
                inside += 1;
            }
        }
    }
    inside
}

fn main() {
    let n: usize = env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(16000);
    let iter: usize = env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(50);
    let t0 = Instant::now();
    let r = count_inside(n, iter);
    let elapsed = t0.elapsed();
    println!("mandelbrot N={} iter={} inside={}", n, iter, r);
    println!("elapsed: {:?}", elapsed);
}
