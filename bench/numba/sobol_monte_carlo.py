# Port of the Soma cell's full sobol_monte_carlo workload: Black-Scholes
# pricing + Monte Carlo Pi (pseudo-random and Sobol) at multiple path counts.
# Numba's @njit handles the float/i64 inner loops natively.
from _inner import inner
from numba import njit
from math import log, sqrt, exp


@njit(cache=True)
def van_der_corput(n):
    result = 0.0
    base = 0.5
    num = n
    while num > 0:
        if num % 2 == 1:
            result = result + base
        num = num // 2
        base = base / 2
    return result


@njit(cache=True)
def inv_norm(u):
    if u <= 0.0: return -8.0
    if u >= 1.0: return 8.0
    t = u - 0.5
    if abs(t) < 0.42:
        r = t * t
        return t * ((((-25.44106049637 * r + 41.39119773534) * r + -18.61500062529) * r + 2.50662823884)
                    / ((((3.13082909833 * r + -21.06224101826) * r + 23.08336743743) * r + -8.47351093090) * r + 1.0))
    r = u
    if t > 0: r = 1.0 - u
    s = log(-log(r))
    z = 0.3374754822726147 + s * (0.9761690190917186 + s * (0.1607979714918209 + s * (0.0276438810333863 + s * 0.0038405729373609)))
    if t < 0: return -z
    return z


@njit(cache=True)
def bs_d1(S, K, r, sigma, T):
    return (log(S / K) + (r + sigma * sigma / 2) * T) / (sigma * sqrt(T))


@njit(cache=True)
def norm_cdf(x):
    if x < -8.0: return 0.0
    if x > 8.0: return 1.0
    b1 = 0.319381530; b2 = -0.356563782; b3 = 1.781477937
    b4 = -1.821255978; b5 = 1.330274429
    pp = 0.2316419
    ax = abs(x)
    t_val = 1.0 / (1.0 + pp * ax)
    pdf = exp(-ax * ax / 2.0) / 2.5066282746310002
    cdf = 1.0 - pdf * t_val * (b1 + t_val * (b2 + t_val * (b3 + t_val * (b4 + t_val * b5))))
    if x < 0: return 1.0 - cdf
    return cdf


@njit(cache=True)
def bs_price(S, K, r, sigma, T):
    d1 = bs_d1(S, K, r, sigma, T)
    d2 = d1 - sigma * sqrt(T)
    return S * norm_cdf(d1) - K * exp(-r * T) * norm_cdf(d2)


@njit(cache=True)
def mc_price(S, K, r, sigma, T, n_paths, use_sobol):
    payoff_sum = 0.0
    drift = (r - sigma * sigma / 2) * T
    vol = sigma * sqrt(T)
    state = 12345
    for i in range(1, n_paths + 1):
        if use_sobol == 1:
            u = van_der_corput(i)
            z = inv_norm(u)
        else:
            state = (state * 1103515245 + 12345) & 0x7FFFFFFF
            u = state / 2147483648.0
            if u < 0.0001: u = 0.0001
            if u > 0.9999: u = 0.9999
            z = inv_norm(u)
        ST = S * exp(drift + vol * z)
        payoff = ST - K
        if payoff < 0: payoff = 0.0
        payoff_sum = payoff_sum + payoff
    return exp(-r * T) * payoff_sum / n_paths


def workload():
    S = 100.0; K = 105.0; r = 0.05; sigma = 0.2; T = 1.0
    bs_price(S, K, r, sigma, T)
    for n in (100, 500, 1000, 5000, 100000, 1000000):
        mc_price(S, K, r, sigma, T, n, 0)
        mc_price(S, K, r, sigma, T, n, 1)
    for i in range(1, 11):
        van_der_corput(i)


def warmup():
    try: bs_price(100.0, 105.0, 0.05, 0.2, 1.0)
    except: pass
    try: mc_price(100.0, 105.0, 0.05, 0.2, 1.0, 2, 0)
    except: pass
    try: mc_price(100.0, 105.0, 0.05, 0.2, 1.0, 2, 1)
    except: pass


inner(workload, warmup=warmup)
