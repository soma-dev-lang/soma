// CLBG n-body — Rust reference (sequential, no SIMD intrinsics).
// Same algorithm as the Soma version: 5 bodies, hand-unrolled state.
// Build with: cargo build --release
// Run: target/release/nbody 50000000

use std::env;
use std::time::Instant;

const PI: f64 = 3.141592653589793;
const SOLAR_MASS: f64 = 4.0 * PI * PI;
const DAYS_PER_YEAR: f64 = 365.24;

#[derive(Clone, Copy)]
struct Body {
    x: f64, y: f64, z: f64,
    vx: f64, vy: f64, vz: f64,
    mass: f64,
}

fn advance(bodies: &mut [Body; 5], dt: f64) {
    for i in 0..5 {
        let (left, right) = bodies.split_at_mut(i + 1);
        let bi = &mut left[i];
        for bj in right.iter_mut() {
            let dx = bi.x - bj.x;
            let dy = bi.y - bj.y;
            let dz = bi.z - bj.z;
            let d2 = dx*dx + dy*dy + dz*dz;
            let mag = dt / (d2 * d2.sqrt());
            bi.vx -= dx * bj.mass * mag;
            bi.vy -= dy * bj.mass * mag;
            bi.vz -= dz * bj.mass * mag;
            bj.vx += dx * bi.mass * mag;
            bj.vy += dy * bi.mass * mag;
            bj.vz += dz * bi.mass * mag;
        }
    }
    for b in bodies.iter_mut() {
        b.x += dt * b.vx;
        b.y += dt * b.vy;
        b.z += dt * b.vz;
    }
}

fn energy(bodies: &[Body; 5]) -> f64 {
    let mut e = 0.0;
    for i in 0..5 {
        let bi = &bodies[i];
        e += 0.5 * bi.mass * (bi.vx*bi.vx + bi.vy*bi.vy + bi.vz*bi.vz);
        for j in (i+1)..5 {
            let bj = &bodies[j];
            let dx = bi.x - bj.x;
            let dy = bi.y - bj.y;
            let dz = bi.z - bj.z;
            e -= bi.mass * bj.mass / (dx*dx + dy*dy + dz*dz).sqrt();
        }
    }
    e
}

fn offset_momentum(bodies: &mut [Body; 5]) {
    let mut px = 0.0; let mut py = 0.0; let mut pz = 0.0;
    for b in bodies.iter() {
        px += b.vx * b.mass;
        py += b.vy * b.mass;
        pz += b.vz * b.mass;
    }
    bodies[0].vx = -px / SOLAR_MASS;
    bodies[0].vy = -py / SOLAR_MASS;
    bodies[0].vz = -pz / SOLAR_MASS;
}

fn main() {
    let n: usize = env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(50_000_000);

    let mut bodies = [
        // Sun
        Body { x: 0.0, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, mass: SOLAR_MASS },
        // Jupiter
        Body { x: 4.84143144246472090, y: -1.16032004402742839, z: -0.103622044471123109,
               vx: 0.00166007664274403694 * DAYS_PER_YEAR, vy: 0.00769901118419740425 * DAYS_PER_YEAR, vz: -0.0000690460016972063023 * DAYS_PER_YEAR,
               mass: 0.000954791938424326609 * SOLAR_MASS },
        // Saturn
        Body { x: 8.34336671824457987, y: 4.12479856412430479, z: -0.403523417114321381,
               vx: -0.00276742510726862411 * DAYS_PER_YEAR, vy: 0.00499852801234917238 * DAYS_PER_YEAR, vz: 0.0000230417297573763929 * DAYS_PER_YEAR,
               mass: 0.000285885980666130812 * SOLAR_MASS },
        // Uranus
        Body { x: 12.8943695621391310, y: -15.1111514016986312, z: -0.223307578892655734,
               vx: 0.00296460137564761618 * DAYS_PER_YEAR, vy: 0.00237847173959480950 * DAYS_PER_YEAR, vz: -0.0000296589568540237556 * DAYS_PER_YEAR,
               mass: 0.0000436624404335156298 * SOLAR_MASS },
        // Neptune
        Body { x: 15.3796971148509165, y: -25.9193146099879641, z: 0.179258772950371181,
               vx: 0.00268067772490389322 * DAYS_PER_YEAR, vy: 0.00162824170038242295 * DAYS_PER_YEAR, vz: -0.000095159225451971189 * DAYS_PER_YEAR,
               mass: 0.0000515138902046611451 * SOLAR_MASS },
    ];

    offset_momentum(&mut bodies);
    let e0 = energy(&bodies);

    let t0 = Instant::now();
    for _ in 0..n {
        advance(&mut bodies, 0.01);
    }
    let elapsed = t0.elapsed();

    let e1 = energy(&bodies);
    println!("e0={:.9}", e0);
    println!("e1={:.9}", e1);
    println!("nbody N={} elapsed: {:?}", n, elapsed);
}
