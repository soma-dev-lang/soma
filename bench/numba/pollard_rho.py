from _inner import inner
from math import gcd

def pollard_rho(n):
    if n % 2 == 0: return 2
    x, y, c, d = 2, 2, 1, 1
    while d == 1:
        x = (x * x + c) % n
        y = (y * y + c) % n
        y = (y * y + c) % n
        d = gcd(abs(x - y), n)
    return d if d != n else None

def workload():
    for n in [9998000099, 20127115513867, 15214111702481539,
              1073602561, 68718821377, 1125897758834689, 4951760154835678088235319297]:
        pollard_rho(n)

inner(workload)
