// CLBG n-body — C reference (sequential, no SIMD intrinsics).
// Same algorithm as the Soma + Rust versions: 5 bodies.
// Build: clang -O3 -march=native -ffast-math nbody.c -o nbody -lm
//   (we omit -ffast-math to keep the answer bit-comparable; see Makefile)
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <time.h>

#define PI 3.141592653589793
#define SOLAR_MASS (4.0 * PI * PI)
#define DAYS_PER_YEAR 365.24
#define N_BODIES 5

typedef struct {
    double x, y, z;
    double vx, vy, vz;
    double mass;
} Body;

static void advance(Body *bodies, double dt) {
    for (int i = 0; i < N_BODIES; i++) {
        Body *bi = &bodies[i];
        for (int j = i + 1; j < N_BODIES; j++) {
            Body *bj = &bodies[j];
            double dx = bi->x - bj->x;
            double dy = bi->y - bj->y;
            double dz = bi->z - bj->z;
            double d2 = dx*dx + dy*dy + dz*dz;
            double mag = dt / (d2 * sqrt(d2));
            bi->vx -= dx * bj->mass * mag;
            bi->vy -= dy * bj->mass * mag;
            bi->vz -= dz * bj->mass * mag;
            bj->vx += dx * bi->mass * mag;
            bj->vy += dy * bi->mass * mag;
            bj->vz += dz * bi->mass * mag;
        }
    }
    for (int i = 0; i < N_BODIES; i++) {
        bodies[i].x += dt * bodies[i].vx;
        bodies[i].y += dt * bodies[i].vy;
        bodies[i].z += dt * bodies[i].vz;
    }
}

static double energy(const Body *bodies) {
    double e = 0.0;
    for (int i = 0; i < N_BODIES; i++) {
        const Body *bi = &bodies[i];
        e += 0.5 * bi->mass * (bi->vx*bi->vx + bi->vy*bi->vy + bi->vz*bi->vz);
        for (int j = i + 1; j < N_BODIES; j++) {
            const Body *bj = &bodies[j];
            double dx = bi->x - bj->x;
            double dy = bi->y - bj->y;
            double dz = bi->z - bj->z;
            e -= bi->mass * bj->mass / sqrt(dx*dx + dy*dy + dz*dz);
        }
    }
    return e;
}

static void offset_momentum(Body *bodies) {
    double px = 0, py = 0, pz = 0;
    for (int i = 0; i < N_BODIES; i++) {
        px += bodies[i].vx * bodies[i].mass;
        py += bodies[i].vy * bodies[i].mass;
        pz += bodies[i].vz * bodies[i].mass;
    }
    bodies[0].vx = -px / SOLAR_MASS;
    bodies[0].vy = -py / SOLAR_MASS;
    bodies[0].vz = -pz / SOLAR_MASS;
}

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 50000000;

    Body bodies[N_BODIES] = {
        // Sun
        {0,0,0, 0,0,0, SOLAR_MASS},
        // Jupiter
        {4.84143144246472090, -1.16032004402742839, -0.103622044471123109,
         0.00166007664274403694*DAYS_PER_YEAR, 0.00769901118419740425*DAYS_PER_YEAR, -0.0000690460016972063023*DAYS_PER_YEAR,
         0.000954791938424326609*SOLAR_MASS},
        // Saturn
        {8.34336671824457987, 4.12479856412430479, -0.403523417114321381,
         -0.00276742510726862411*DAYS_PER_YEAR, 0.00499852801234917238*DAYS_PER_YEAR, 0.0000230417297573763929*DAYS_PER_YEAR,
         0.000285885980666130812*SOLAR_MASS},
        // Uranus
        {12.8943695621391310, -15.1111514016986312, -0.223307578892655734,
         0.00296460137564761618*DAYS_PER_YEAR, 0.00237847173959480950*DAYS_PER_YEAR, -0.0000296589568540237556*DAYS_PER_YEAR,
         0.0000436624404335156298*SOLAR_MASS},
        // Neptune
        {15.3796971148509165, -25.9193146099879641, 0.179258772950371181,
         0.00268067772490389322*DAYS_PER_YEAR, 0.00162824170038242295*DAYS_PER_YEAR, -0.000095159225451971189*DAYS_PER_YEAR,
         0.0000515138902046611451*SOLAR_MASS},
    };

    offset_momentum(bodies);
    double e0 = energy(bodies);

    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (long i = 0; i < n; i++) advance(bodies, 0.01);
    clock_gettime(CLOCK_MONOTONIC, &t1);

    double e1 = energy(bodies);
    double elapsed_ms = (t1.tv_sec - t0.tv_sec) * 1000.0 + (t1.tv_nsec - t0.tv_nsec) / 1.0e6;
    printf("e0=%.9f\n", e0);
    printf("e1=%.9f\n", e1);
    printf("nbody N=%ld elapsed: %.0fms\n", n, elapsed_ms);
    return 0;
}
