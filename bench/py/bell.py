from _inner import inner
def bell(n):
    if n == 0: return 1
    row = [1]
    for _ in range(n):
        new_row = [row[-1]]
        for x in row:
            new_row.append(new_row[-1] + x)
        row = new_row
    return row[0]

def workload():
    # Same as bell.cell run()
    for n in (0, 1, 2, 5, 10, 15, 20, 25, 30):
        bell(n)

inner(workload)
