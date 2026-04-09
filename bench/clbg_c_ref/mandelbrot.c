// CLBG mandelbrot — C reference (scalar, no SIMD intrinsics).
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static long count_inside(long grid_n, long max_iter) {
    const double x_min = -2.0, x_max = 1.0;
    const double y_min = -1.5, y_max = 1.5;
    double dx = (x_max - x_min) / (double)grid_n;
    double dy = (y_max - y_min) / (double)grid_n;
    long inside = 0;
    for (long py = 0; py < grid_n; py++) {
        double cy = y_min + (double)py * dy;
        for (long px = 0; px < grid_n; px++) {
            double cx = x_min + (double)px * dx;
            double zx = 0, zy = 0, zx2 = 0, zy2 = 0;
            long iter = 0;
            while (iter < max_iter) {
                if (zx2 + zy2 >= 4.0) { iter = max_iter + 1; }
                else {
                    zy = 2.0 * zx * zy + cy;
                    zx = zx2 - zy2 + cx;
                    zx2 = zx * zx;
                    zy2 = zy * zy;
                    iter++;
                }
            }
            if (iter == max_iter) inside++;
        }
    }
    return inside;
}

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 16000;
    long iter = (argc > 2) ? atol(argv[2]) : 50;
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    long r = count_inside(n, iter);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    double elapsed_ms = (t1.tv_sec - t0.tv_sec) * 1000.0 + (t1.tv_nsec - t0.tv_nsec) / 1.0e6;
    printf("mandelbrot N=%ld iter=%ld inside=%ld\n", n, iter, r);
    printf("elapsed: %.0fms\n", elapsed_ms);
    return 0;
}
