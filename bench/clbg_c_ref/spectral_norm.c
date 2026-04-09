// CLBG spectral-norm — C reference (sequential).
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <time.h>

static inline double a(long i, long j) {
    double k = (double)(i + j);
    return 1.0 / (k * (k + 1.0) * 0.5 + (double)i + 1.0);
}

static void mul_av(long n, const double *u, double *v) {
    for (long i = 0; i < n; i++) {
        double s = 0.0;
        for (long j = 0; j < n; j++) s += a(i, j) * u[j];
        v[i] = s;
    }
}

static void mul_atv(long n, const double *u, double *v) {
    for (long i = 0; i < n; i++) {
        double s = 0.0;
        for (long j = 0; j < n; j++) s += a(j, i) * u[j];
        v[i] = s;
    }
}

static void mul_atav(long n, const double *u, double *v, double *t) {
    mul_av(n, u, t);
    mul_atv(n, t, v);
}

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 5500;
    double *u = malloc(n * sizeof(double));
    double *v = malloc(n * sizeof(double));
    double *t = malloc(n * sizeof(double));
    for (long i = 0; i < n; i++) { u[i] = 1.0; v[i] = 0.0; t[i] = 0.0; }

    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (int i = 0; i < 10; i++) {
        mul_atav(n, u, v, t);
        mul_atav(n, v, u, t);
    }
    double vbv = 0.0, vv = 0.0;
    for (long k = 0; k < n; k++) { vbv += u[k]*v[k]; vv += v[k]*v[k]; }
    double result = sqrt(vbv / vv);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    double elapsed_ms = (t1.tv_sec - t0.tv_sec) * 1000.0 + (t1.tv_nsec - t0.tv_nsec) / 1.0e6;
    printf("spectral-norm N=%ld = %.9f\n", n, result);
    printf("elapsed: %.0fms\n", elapsed_ms);
    free(u); free(v); free(t);
    return 0;
}
