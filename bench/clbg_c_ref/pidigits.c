// CLBG pidigits — C reference using GMP (libgmp). Same Gibbons spigot
// algorithm as the Soma + Rust versions.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <gmp.h>

static void compute(long target, char *out, size_t out_cap) {
    mpz_t q, r, s, t, num, den, num4, den4, d, d4, ds, dt_, nq, nr, ns, nt, qu, rv, su, tv, kk, u, v, tmp;
    mpz_inits(q, r, s, t, num, den, num4, den4, d, d4, ds, dt_, nq, nr, ns, nt, qu, rv, su, tv, kk, u, v, tmp, NULL);
    mpz_set_ui(q, 1);
    mpz_set_ui(r, 0);
    mpz_set_ui(s, 0);
    mpz_set_ui(t, 1);
    unsigned long k = 1;
    long digits = 0;
    size_t out_len = 0;

    while (digits <= target) {
        // num = q*3 + r
        mpz_mul_ui(num, q, 3); mpz_add(num, num, r);
        // den = s*3 + t
        mpz_mul_ui(den, s, 3); mpz_add(den, den, t);
        mpz_tdiv_q(d, num, den);
        // num4 = q*4 + r ; den4 = s*4 + t
        mpz_mul_ui(num4, q, 4); mpz_add(num4, num4, r);
        mpz_mul_ui(den4, s, 4); mpz_add(den4, den4, t);
        mpz_tdiv_q(d4, num4, den4);
        if (mpz_cmp(d, d4) == 0) {
            if (digits == 0) {
                digits = 1;
            } else {
                // append digit
                char buf[64];
                gmp_snprintf(buf, sizeof(buf), "%Zd", d);
                size_t L = strlen(buf);
                if (out_len + L < out_cap) { memcpy(out + out_len, buf, L); out_len += L; }
                digits++;
            }
            // q = (q - d*s) * 10 ; r = (r - d*t) * 10
            mpz_mul(ds, d, s);
            mpz_sub(nq, q, ds);
            mpz_mul_ui(nq, nq, 10);
            mpz_mul(dt_, d, t);
            mpz_sub(nr, r, dt_);
            mpz_mul_ui(nr, nr, 10);
            mpz_set(q, nq);
            mpz_set(r, nr);
        } else {
            mpz_set_ui(u, 4*k + 2);
            mpz_set_ui(v, 2*k + 1);
            // nq = q*k
            mpz_mul_ui(nq, q, k);
            // nr = q*u + r*v
            mpz_mul(qu, q, u);
            mpz_mul(rv, r, v);
            mpz_add(nr, qu, rv);
            // ns = s*k
            mpz_mul_ui(ns, s, k);
            // nt = s*u + t*v
            mpz_mul(su, s, u);
            mpz_mul(tv, t, v);
            mpz_add(nt, su, tv);
            mpz_set(q, nq);
            mpz_set(r, nr);
            mpz_set(s, ns);
            mpz_set(t, nt);
            k++;
        }
    }
    out[out_len] = '\0';
    mpz_clears(q, r, s, t, num, den, num4, den4, d, d4, ds, dt_, nq, nr, ns, nt, qu, rv, su, tv, kk, u, v, tmp, NULL);
}

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 10000;
    size_t cap = (size_t)n * 2 + 64;
    char *buf = malloc(cap);
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    compute(n, buf, cap);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    double elapsed_ms = (t1.tv_sec - t0.tv_sec) * 1000.0 + (t1.tv_nsec - t0.tv_nsec) / 1.0e6;
    size_t L = strlen(buf);
    const char *last10 = (L >= 10) ? buf + (L - 10) : buf;
    printf("pidigits N=%ld last10=%s\n", n, last10);
    printf("elapsed: %.0fms\n", elapsed_ms);
    free(buf);
    return 0;
}
