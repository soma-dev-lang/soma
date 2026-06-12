# matrix — Soma's first package

Higher-level linear algebra on Soma's first-class matrices: `inverse`,
`solve`, `dot`, `norm`, `is_symmetric`, `hadamard`, `mat_pow`, `outer`,
`row`/`col`. Pure Soma, built on the `reshape` / `*` / `det` / `matmul`
builtins.

```soma
use matrix

let Ainv = inverse(A)                 // Gauss-Jordan with partial pivoting
let x    = solve(A, b)                // A x = b
let n    = norm(v)                    // Euclidean
```

## Install

In a consumer project's `soma.toml`:

```toml
[dependencies]
matrix = { path = "../packages/matrix" }   # or { git = "..." }, or a version
```

```
soma install         # → .soma_env/packages/matrix
soma run app.cell
```

## Why this package looks the way it does

It is the reference example of how Soma packages should work, and the
design borrows the best of three managers while adding the thing none of
them has:

- **Cargo's reproducibility** — `soma.toml` + `soma.lock`, immutable
  resolved versions, source-only.
- **Julia's environments** — installs are per-project and content-cached
  under `.soma_env/`, not dumped into a global or a `node_modules` pile.
- **The Soma differentiator — proofs travel with the package.** A
  package's *public API is its cells' `face` sections*:

  ```
  soma describe packages/matrix/matrix.cell --faces
  ```

  emits the contract — `inverse(m: List) -> List`, `solve(...)`, … — with
  no bodies. And the package ships its `cell test` block as the proof a
  consumer re-runs *on their own toolchain* before trusting it:

  ```
  soma test packages/matrix/matrix.cell      # 10 passed, 0 failed
  ```

  npm asks you to trust the registry; Cargo asks you to trust immutable
  source; Soma lets you re-verify the proofs yourself. The trust boundary
  is a theorem you can run, not a reputation you have to accept.

## API

| Signal | Meaning |
|--------|---------|
| `dot(a, b)` | vector dot product |
| `norm(v)` | Euclidean norm |
| `row(m, i)` / `col(m, j)` | extract a row / column |
| `is_square(m)` / `is_symmetric(m)` | shape / symmetry predicates |
| `hadamard(a, b)` | elementwise product |
| `inverse(m)` | matrix inverse (Gauss-Jordan, partial pivoting) |
| `solve(A, b)` | solve `A x = b` |
| `mat_pow(m, k)` | `m` raised to the `k`-th power |
| `outer(a, b)` | outer product |

The `inverse` implementation is a clean showcase of Soma's nested lvalue
mutation (`a[r][j] = …`, `inv[r][j] = …`).
