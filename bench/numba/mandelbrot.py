from _inner import inner
from numba import njit


@njit(cache=True)
def count_inside(grid_n, max_iter):
    x_min, x_max, y_min, y_max = -2.0, 1.0, -1.5, 1.5
    dx = (x_max - x_min) / grid_n
    dy = (y_max - y_min) / grid_n
    inside = 0
    for py in range(grid_n):
        cy = y_min + py * dy
        for px in range(grid_n):
            cx = x_min + px * dx
            zx = zy = zx2 = zy2 = 0.0
            it = 0
            while it < max_iter:
                if zx2 + zy2 >= 4.0:
                    it = max_iter + 1
                else:
                    zy = 2.0 * zx * zy + cy
                    zx = zx2 - zy2 + cx
                    zx2 = zx * zx
                    zy2 = zy * zy
                    it += 1
            if it == max_iter:
                inside += 1
    return inside

def workload():
    # Match the cell's first three checks (50, 100, 500). The cell ALSO
    # runs 1000 and 2000 grids, but Python takes >5min for those — they
    # are the cell's "headline" stress tests, omitted here.
    count_inside(50, 100)
    count_inside(100, 255)
    count_inside(500, 500)
    count_inside(1000, 1000)

def warmup():
    try: count_inside(2, 2)
    except Exception: pass

inner(workload, warmup=warmup)