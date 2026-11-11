# soma (Python)

Use a Soma program as the verified core of a Python program: load a `.cell`
file, call its handlers, get typed values back and errors by kind.

```python
import soma

with soma.load("treasury/app.cell") as app:
    print(app.handlers())                      # {"Ledger": {"debit": {...}}, ...}
    app.seed(1000)                             # a handler, by name
    try:
        app.call("Ledger.debit", amount=5000)  # by parameter name, cell named
    except soma.SomaError as e:
        print(e.kind)                          # "invariant", "requires", "not_found", …
```

It needs the `soma` binary (3.0 or later): on `PATH`, at `~/.soma/bin/soma`,
or named by `SOMA_BIN`. No other dependency.

What crosses the boundary: `None` ↔ `()`, `bool`, `int` (any size), `float`,
`str`, `list`, `dict`, and `soma.Variant` for a value of one of the program's
sum types. Each call is one atomic handler invocation, with the program's
data in `.soma_data/` beside the `.cell` file — exactly as `soma run` would
do it. A program that fails `soma check` does not load (`soma.LoadError`,
with the diagnostics).
