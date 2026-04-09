// CLBG fannkuch-redux — C reference (sequential).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void solve(int n, long *out_max, long *out_check) {
    long perm[32], perm1[32], count[32];
    for (int i = 0; i < n; i++) perm1[i] = i;
    for (int i = 0; i < n; i++) count[i] = 0;
    long max_flips = 0, checksum = 0, permutation_count = 0;
    int r = n;
    for (;;) {
        while (r != 1) { count[r-1] = r; r--; }
        for (int i = 0; i < n; i++) perm[i] = perm1[i];
        long flips = 0;
        long k = perm[0];
        while (k != 0) {
            int j = 0; int m = (int)k;
            while (j < m) {
                long tmp = perm[j]; perm[j] = perm[m]; perm[m] = tmp;
                j++; m--;
            }
            flips++;
            k = perm[0];
        }
        if (flips > max_flips) max_flips = flips;
        if ((permutation_count & 1) == 0) checksum += flips;
        else checksum -= flips;
        permutation_count++;

        for (;;) {
            if (r == n) { *out_max = max_flips; *out_check = checksum; return; }
            long p0 = perm1[0];
            for (int i = 0; i < r; i++) perm1[i] = perm1[i+1];
            perm1[r] = p0;
            count[r]--;
            if (count[r] > 0) break;
            r++;
        }
    }
}

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 12;
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    long m, c;
    solve(n, &m, &c);
    clock_gettime(CLOCK_MONOTONIC, &t1);
    double elapsed_ms = (t1.tv_sec - t0.tv_sec) * 1000.0 + (t1.tv_nsec - t0.tv_nsec) / 1.0e6;
    printf("fannkuch N=%d max=%ld checksum=%ld\n", n, m, c);
    printf("elapsed: %.0fms\n", elapsed_ms);
    return 0;
}
