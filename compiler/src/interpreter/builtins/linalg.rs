// Quantum-inspired classical linear algebra (Tang et al.).
//
// Implements:
//   - importance_sample_rows(A, opts)  ℓ²-norm row sampling
//   - svd_lowrank(A, opts)             FKV-style low-rank SVD
//   - regress_sgd(A, b, opts)          SGD regression à la Gilyén-Song-Tang
//
// All three accept an options Map carrying explicit bounds so the
// budget checker can prove closed-form runtime / sample complexity.
//
// References:
//   Tang 1807.04271 §3-4    (BST data structure, ModFKV)
//   Gilyén-Song-Tang 2009.07268 §2 (SGD regression, Definition 2.1)

use super::super::{Value, RuntimeError};
use super::super::map_from_pairs;
use crate::interpreter::soma_int::SomaInt;
use indexmap::IndexMap;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// ── PRNG (same xorshift+LCG flavour as math::random) ─────────────────

thread_local! {
    static LINALG_RAND: Cell<u64> = Cell::new(0);
}

fn rand_u64() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut x = LINALG_RAND.with(|c| {
        let n = c.get();
        c.set(n.wrapping_add(1));
        nanos ^ n
    });
    x ^= x >> 7;
    x ^= x << 13;
    x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x
}

fn rand_f64() -> f64 {
    (rand_u64() % 1_000_000_007) as f64 / 1_000_000_007.0
}

// ── Value plumbing ───────────────────────────────────────────────────

fn val_to_f64(v: &Value) -> Result<f64, RuntimeError> {
    match v {
        Value::Float(x) => Ok(*x),
        Value::Int(si) => si
            .to_i64()
            .map(|n| n as f64)
            .ok_or_else(|| RuntimeError::TypeError("linalg: integer too large".into())),
        _ => Err(RuntimeError::TypeError("linalg: expected number".into())),
    }
}

fn val_to_usize(v: &Value) -> Result<usize, RuntimeError> {
    match v {
        Value::Int(si) => si
            .to_i64()
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
            .ok_or_else(|| RuntimeError::TypeError("linalg: expected non-negative integer".into())),
        _ => Err(RuntimeError::TypeError("linalg: expected integer".into())),
    }
}

fn to_vector(v: &Value) -> Result<Vec<f64>, RuntimeError> {
    match v {
        Value::List(items) => items.iter().map(val_to_f64).collect(),
        _ => Err(RuntimeError::TypeError(
            "linalg: expected List of numbers (vector)".into(),
        )),
    }
}

fn to_matrix(v: &Value) -> Result<Vec<Vec<f64>>, RuntimeError> {
    let rows = match v {
        Value::List(items) => items,
        _ => {
            return Err(RuntimeError::TypeError(
                "linalg: expected List of Lists (matrix)".into(),
            ))
        }
    };
    if rows.is_empty() {
        return Err(RuntimeError::TypeError("linalg: matrix is empty".into()));
    }
    let m: Vec<Vec<f64>> = rows
        .iter()
        .map(to_vector)
        .collect::<Result<_, _>>()?;
    let n = m[0].len();
    if m.iter().any(|r| r.len() != n) {
        return Err(RuntimeError::TypeError("linalg: ragged matrix".into()));
    }
    if n == 0 {
        return Err(RuntimeError::TypeError("linalg: matrix has 0 columns".into()));
    }
    Ok(m)
}

fn vec_to_value(v: &[f64]) -> Value {
    Value::List(v.iter().map(|x| Value::Float(*x)).collect())
}

fn matrix_to_value(m: &[Vec<f64>]) -> Value {
    Value::List(m.iter().map(|r| vec_to_value(r)).collect())
}

fn opt_get(opts: &Value, key: &str) -> Option<Value> {
    if let Value::Map(m) = opts {
        m.get(key).cloned()
    } else {
        None
    }
}

fn opt_usize(opts: &Value, key: &str, default: usize) -> Result<usize, RuntimeError> {
    match opt_get(opts, key) {
        Some(v) => val_to_usize(&v),
        None => Ok(default),
    }
}

fn opt_f64(opts: &Value, key: &str, default: f64) -> Result<f64, RuntimeError> {
    match opt_get(opts, key) {
        Some(v) => val_to_f64(&v),
        None => Ok(default),
    }
}

// ── Sampled matrix (BST-backed, O(log n) sampling) ───────────────────
//
// A `SampledMatrix` precomputes the binary-search-tree data structure
// from Tang 1807.04271 §3 (Lemma 3.1):
//   - sample a row index in O(log m)
//   - sample a column index of a given row in O(log n)
//   - query an entry in O(1)
//   - return ‖A‖_F or any ‖A_i‖ in O(1)
//
// Construction is O(mn) (one pass over the matrix), but every
// subsequent SQ-style query is sublinear.  This is the data structure
// every dequantization paper assumes the input lives in.
//
// User-facing identity: a `Value::Map` with a `__sampled__` marker
// field carrying a registry handle.  The Map's other entries — `rows`,
// `cols`, `fro_norm` — let user code introspect the matrix without
// touching the registry.

#[derive(Debug)]
pub struct SampledMatrix {
    rows: usize,
    cols: usize,
    data: Vec<Vec<f64>>,
    row_norm_sq: Vec<f64>,
    row_cdf: Vec<f64>, // prefix sums of row_norm_sq, length rows+1
    col_cdf: Vec<Vec<f64>>, // per-row prefix sums of |A_{r,j}|², length rows×(cols+1)
    fro_sq: f64,
}

impl SampledMatrix {
    pub fn from_dense(data: Vec<Vec<f64>>) -> Self {
        let rows = data.len();
        let cols = if rows == 0 { 0 } else { data[0].len() };
        let mut row_norm_sq = vec![0.0f64; rows];
        let mut row_cdf = vec![0.0f64; rows + 1];
        let mut col_cdf = Vec::with_capacity(rows);
        for i in 0..rows {
            let mut acc = 0.0;
            let mut entries = Vec::with_capacity(cols + 1);
            entries.push(0.0);
            for j in 0..cols {
                acc += data[i][j] * data[i][j];
                entries.push(acc);
            }
            row_norm_sq[i] = acc;
            col_cdf.push(entries);
            row_cdf[i + 1] = row_cdf[i] + acc;
        }
        let fro_sq = row_cdf[rows];
        SampledMatrix {
            rows,
            cols,
            data,
            row_norm_sq,
            row_cdf,
            col_cdf,
            fro_sq,
        }
    }

    pub fn rows(&self) -> usize { self.rows }
    pub fn cols(&self) -> usize { self.cols }
    pub fn fro_sq(&self) -> f64 { self.fro_sq }
    pub fn entry(&self, i: usize, j: usize) -> f64 { self.data[i][j] }
    pub fn row(&self, i: usize) -> &[f64] { &self.data[i] }
    pub fn row_norm_sq(&self, i: usize) -> f64 { self.row_norm_sq[i] }
    pub fn dense(&self) -> &[Vec<f64>] { &self.data }

    /// Sample row index ~ ‖A_i‖² / ‖A‖_F²  in O(log m).
    pub fn sample_row(&self) -> usize {
        let target = rand_f64() * self.fro_sq;
        bsearch_cdf(&self.row_cdf, target).min(self.rows - 1)
    }

    /// Sample column j of row i with prob A_{i,j}² / ‖A_i‖²  in O(log n).
    pub fn sample_col_of_row(&self, i: usize) -> usize {
        let cdf = &self.col_cdf[i];
        let target = rand_f64() * cdf[self.cols];
        bsearch_cdf(cdf, target).min(self.cols - 1)
    }
}

fn bsearch_cdf(cdf: &[f64], target: f64) -> usize {
    // CDF is monotone non-decreasing; find smallest i such that cdf[i+1] >= target.
    let n = cdf.len() - 1;
    if n == 0 {
        return 0;
    }
    let mut lo = 0usize;
    let mut hi = n;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if cdf[mid + 1] >= target {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

// Thread-local registry of sampled matrices.  Handles are positive
// i64 IDs allocated by an atomic counter; the user-visible Value is a
// Map carrying { __sampled__: handle, rows, cols, fro_norm } and
// internally the algorithms look up the BST.

thread_local! {
    static SAMPLED_REGISTRY: RefCell<HashMap<i64, Arc<SampledMatrix>>> = RefCell::new(HashMap::new());
    static SAMPLED_COUNTER: Cell<i64> = Cell::new(0);
}

fn register_sampled(m: SampledMatrix) -> i64 {
    let id = SAMPLED_COUNTER.with(|c| {
        let v = c.get().wrapping_add(1);
        c.set(v);
        v
    });
    SAMPLED_REGISTRY.with(|r| r.borrow_mut().insert(id, Arc::new(m)));
    id
}

fn lookup_sampled(id: i64) -> Option<Arc<SampledMatrix>> {
    SAMPLED_REGISTRY.with(|r| r.borrow().get(&id).cloned())
}

/// If `v` is a Map carrying a `__sampled__` handle, return the
/// SampledMatrix; otherwise return None.
fn as_sampled(v: &Value) -> Option<Arc<SampledMatrix>> {
    if let Value::Map(m) = v {
        if let Some(Value::Int(si)) = m.get("__sampled__") {
            if let Some(id) = si.to_i64() {
                return lookup_sampled(id);
            }
        }
    }
    None
}

/// Build the user-facing handle Map.
fn sampled_handle_value(id: i64, m: &SampledMatrix) -> Value {
    let mut out = IndexMap::new();
    out.insert("__sampled__".into(), Value::Int(SomaInt::from_i64(id)));
    out.insert("rows".into(), Value::Int(SomaInt::from_i64(m.rows as i64)));
    out.insert("cols".into(), Value::Int(SomaInt::from_i64(m.cols as i64)));
    out.insert("fro_norm".into(), Value::Float(m.fro_sq.sqrt()));
    out.insert("kind".into(), Value::String("sampled".into()));
    Value::Map(out)
}

/// Best-effort matrix access: accept either a dense `List<List<Float>>`
/// or a registered SampledMatrix handle.  Returns either an owned
/// dense matrix or a shared Arc.
enum MatrixView {
    Dense(Vec<Vec<f64>>),
    Sampled(Arc<SampledMatrix>),
}

impl MatrixView {
    fn rows(&self) -> usize {
        match self {
            MatrixView::Dense(m) => m.len(),
            MatrixView::Sampled(s) => s.rows(),
        }
    }
    fn cols(&self) -> usize {
        match self {
            MatrixView::Dense(m) => if m.is_empty() { 0 } else { m[0].len() },
            MatrixView::Sampled(s) => s.cols(),
        }
    }
    fn as_dense(&self) -> &[Vec<f64>] {
        match self {
            MatrixView::Dense(m) => m,
            MatrixView::Sampled(s) => s.dense(),
        }
    }
    fn row_norm_sq(&self) -> Vec<f64> {
        match self {
            MatrixView::Dense(m) => (0..m.len()).map(|i| row_norm_sq(m, i)).collect(),
            MatrixView::Sampled(s) => (0..s.rows()).map(|i| s.row_norm_sq(i)).collect(),
        }
    }
    fn fro_sq(&self) -> f64 {
        match self {
            MatrixView::Dense(m) => fro_norm_sq(m),
            MatrixView::Sampled(s) => s.fro_sq(),
        }
    }
    fn sample_row(&self, row_sq: &[f64], fro_sq: f64) -> usize {
        match self {
            // For dense inputs we fall back to a linear scan over row_sq.
            MatrixView::Dense(_) => sample_weighted(row_sq, fro_sq),
            // For Sampled inputs we use the O(log m) BST.
            MatrixView::Sampled(s) => s.sample_row(),
        }
    }
    fn sample_col_of_row(&self, i: usize, col_weights: &[f64], row_sq_r: f64) -> usize {
        match self {
            MatrixView::Dense(_) => sample_weighted(col_weights, row_sq_r),
            MatrixView::Sampled(s) => s.sample_col_of_row(i),
        }
    }
}

fn to_matrix_view(v: &Value) -> Result<MatrixView, RuntimeError> {
    if let Some(s) = as_sampled(v) {
        return Ok(MatrixView::Sampled(s));
    }
    let m = to_matrix(v)?;
    Ok(MatrixView::Dense(m))
}

// ── ℓ²-norm sampling primitive (Tang 1807.04271 §3) ──────────────────
//
// Given a weight vector `weights` (nonneg) and its sum, return an
// index sampled with probability weights[i] / sum.  Linear scan;
// adequate for the small slices (r, c ≲ 10³) the algorithms touch.

fn sample_weighted(weights: &[f64], sum: f64) -> usize {
    if sum <= 0.0 {
        return rand_u64() as usize % weights.len();
    }
    let target = rand_f64() * sum;
    let mut acc = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if acc >= target {
            return i;
        }
    }
    weights.len() - 1
}

fn row_norm_sq(m: &[Vec<f64>], i: usize) -> f64 {
    m[i].iter().map(|x| x * x).sum()
}

fn fro_norm_sq(m: &[Vec<f64>]) -> f64 {
    m.iter().map(|r| r.iter().map(|x| x * x).sum::<f64>()).sum()
}

// ── Jacobi SVD for small dense matrices ──────────────────────────────
//
// One-sided Jacobi rotations on A: orthogonalize columns. Stable, simple,
// O(n³) per sweep × O(log n) sweeps. Sufficient for the r×c subsketch
// produced by FKV (r, c ≲ a few hundred). Returns U, Σ, V s.t. A = UΣVᵀ.

fn jacobi_svd(a: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>) {
    let m = a.len();
    let n = a[0].len();
    let mut u: Vec<Vec<f64>> = a.iter().map(|r| r.clone()).collect();
    let mut v: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    let max_sweeps = 30;
    let tol = 1e-12;
    for _sweep in 0..max_sweeps {
        let mut off = 0.0f64;
        for p in 0..n - 1 {
            for q in p + 1..n {
                // Compute α = uₚᵀuₚ,  β = u_qᵀu_q,  γ = uₚᵀu_q
                let mut alpha = 0.0;
                let mut beta = 0.0;
                let mut gamma = 0.0;
                for i in 0..m {
                    alpha += u[i][p] * u[i][p];
                    beta += u[i][q] * u[i][q];
                    gamma += u[i][p] * u[i][q];
                }
                off += gamma * gamma;
                if gamma.abs() <= tol * (alpha * beta).sqrt().max(1e-300) {
                    continue;
                }
                let zeta = (beta - alpha) / (2.0 * gamma);
                let t = if zeta >= 0.0 {
                    1.0 / (zeta + (1.0 + zeta * zeta).sqrt())
                } else {
                    1.0 / (zeta - (1.0 + zeta * zeta).sqrt())
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = c * t;
                for i in 0..m {
                    let upi = u[i][p];
                    let uqi = u[i][q];
                    u[i][p] = c * upi - s * uqi;
                    u[i][q] = s * upi + c * uqi;
                }
                for i in 0..n {
                    let vpi = v[i][p];
                    let vqi = v[i][q];
                    v[i][p] = c * vpi - s * vqi;
                    v[i][q] = s * vpi + c * vqi;
                }
            }
        }
        if off < tol * tol {
            break;
        }
    }
    let mut sigma = vec![0.0f64; n];
    for j in 0..n {
        let s2: f64 = (0..m).map(|i| u[i][j] * u[i][j]).sum();
        sigma[j] = s2.sqrt();
        if sigma[j] > 1e-300 {
            for i in 0..m {
                u[i][j] /= sigma[j];
            }
        }
    }
    // Sort by singular value, descending.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| sigma[b].partial_cmp(&sigma[a]).unwrap_or(std::cmp::Ordering::Equal));
    let u_sorted: Vec<Vec<f64>> = (0..m)
        .map(|i| order.iter().map(|&j| u[i][j]).collect())
        .collect();
    let v_sorted: Vec<Vec<f64>> = (0..n)
        .map(|i| order.iter().map(|&j| v[i][j]).collect())
        .collect();
    let sigma_sorted: Vec<f64> = order.iter().map(|&j| sigma[j]).collect();
    (u_sorted, sigma_sorted, v_sorted)
}

// ── importance_sample_rows ───────────────────────────────────────────

fn importance_sample_rows_impl(a: &Value, opts: &Value) -> Result<Value, RuntimeError> {
    let view = to_matrix_view(a)?;
    let r = opt_usize(opts, "samples", 50)?;
    let m = view.rows();
    let n = view.cols();
    let row_sq = view.row_norm_sq();
    let fro_sq = view.fro_sq();
    if fro_sq <= 0.0 {
        return Err(RuntimeError::TypeError("linalg: zero matrix".into()));
    }
    let dense = view.as_dense();
    let _ = m;
    let mut indices = Vec::with_capacity(r);
    let mut weights = Vec::with_capacity(r);
    let mut submat = Vec::with_capacity(r);
    for _ in 0..r {
        let i = view.sample_row(&row_sq, fro_sq);
        indices.push(i);
        let p_i = row_sq[i] / fro_sq;
        // Row weighting per FKV: S_{s,:} = A_{i,:} / sqrt(r · p_i)
        let scale = 1.0 / ((r as f64 * p_i).sqrt().max(1e-300));
        weights.push(scale);
        let mut row = Vec::with_capacity(n);
        for j in 0..n {
            row.push(dense[i][j] * scale);
        }
        submat.push(row);
    }
    let mut out = IndexMap::new();
    out.insert(
        "indices".into(),
        Value::List(
            indices
                .iter()
                .map(|&i| Value::Int(SomaInt::from_i64(i as i64)))
                .collect(),
        ),
    );
    out.insert("weights".into(), vec_to_value(&weights));
    out.insert("submatrix".into(), matrix_to_value(&submat));
    out.insert("fro_norm".into(), Value::Float(fro_sq.sqrt()));
    Ok(Value::Map(out))
}

// ── svd_lowrank (FKV-style, Tang 1807.04271 §4.2) ────────────────────
//
// Output: Map { U, S, V, rank, fro_norm } where
//   U[i][ℓ] ≈ left singular vector ℓ (m×k)  -- recovered by S·V̂/σ̃
//   S[ℓ]     ≈ singular value ℓ
//   V[i][ℓ] ≈ right singular vector ℓ (n×k)  -- via V̂ = Sᵀ Û Σ̂⁻¹

fn svd_lowrank_impl(a: &Value, opts: &Value) -> Result<Value, RuntimeError> {
    let view = to_matrix_view(a)?;
    let m = view.rows();
    let n = view.cols();
    let r = opt_usize(opts, "row_samples", 100.min(m))?.min(m);
    let c = opt_usize(opts, "col_samples", 100.min(n))?.min(n);
    let want_rank = opt_usize(opts, "rank", r.min(c).min(10))?;
    let want_rank = want_rank.min(r).min(c);

    // 1. Sample r rows according to ℓ² norms → S ∈ R^{r×n}
    let row_sq = view.row_norm_sq();
    let fro_sq = view.fro_sq();
    if fro_sq <= 0.0 {
        return Err(RuntimeError::TypeError("linalg: zero matrix".into()));
    }
    let dense = view.as_dense();
    let mut s_rows: Vec<Vec<f64>> = Vec::with_capacity(r);
    let mut s_indices: Vec<usize> = Vec::with_capacity(r);
    for _ in 0..r {
        let i = view.sample_row(&row_sq, fro_sq);
        s_indices.push(i);
        let p_i = row_sq[i] / fro_sq;
        let scale = 1.0 / ((r as f64 * p_i).sqrt().max(1e-300));
        s_rows.push(dense[i].iter().map(|x| x * scale).collect());
    }

    // 2. Sample c columns of S according to column-norm-squared of S
    let s_col_sq: Vec<f64> = (0..n)
        .map(|j| s_rows.iter().map(|r| r[j] * r[j]).sum::<f64>())
        .collect();
    let s_fro_sq: f64 = s_col_sq.iter().sum();
    if s_fro_sq <= 0.0 {
        return Err(RuntimeError::TypeError("linalg: sampled rows are zero".into()));
    }
    let mut w: Vec<Vec<f64>> = vec![vec![0.0; c]; r];
    let mut col_indices: Vec<usize> = Vec::with_capacity(c);
    for k in 0..c {
        let j = sample_weighted(&s_col_sq, s_fro_sq);
        col_indices.push(j);
        let p_j = s_col_sq[j] / s_fro_sq;
        let scale = 1.0 / ((c as f64 * p_j).sqrt().max(1e-300));
        for i in 0..r {
            w[i][k] = s_rows[i][j] * scale;
        }
    }

    // 3. SVD of W (r × c)
    let (u_w, sigma_w, _v_w) = jacobi_svd(&w);

    // 4. Recover approximate right singular vectors of A:
    //    V̂_{:,ℓ} = Sᵀ u^{(ℓ)} / σ̃_ℓ          (n × k)
    let k = want_rank.min(sigma_w.len());
    let mut v_approx: Vec<Vec<f64>> = vec![vec![0.0; k]; n];
    for ell in 0..k {
        let sigma_l = sigma_w[ell].max(1e-300);
        for j in 0..n {
            let mut acc = 0.0;
            for i in 0..r {
                acc += s_rows[i][j] * u_w[i][ell];
            }
            v_approx[j][ell] = acc / sigma_l;
        }
    }

    // 5. Recover approximate left singular vectors of A:
    //    Û = A V̂ Σ̂⁻¹   (m × k)
    let mut u_approx: Vec<Vec<f64>> = vec![vec![0.0; k]; m];
    for ell in 0..k {
        let sigma_l = sigma_w[ell].max(1e-300);
        for i in 0..m {
            let mut acc = 0.0;
            for j in 0..n {
                acc += dense[i][j] * v_approx[j][ell];
            }
            u_approx[i][ell] = acc / sigma_l;
        }
    }

    let mut out = IndexMap::new();
    out.insert("U".into(), matrix_to_value(&u_approx));
    out.insert("S".into(), vec_to_value(&sigma_w[..k]));
    out.insert("V".into(), matrix_to_value(&v_approx));
    out.insert("rank".into(), Value::Int(SomaInt::from_i64(k as i64)));
    out.insert("fro_norm".into(), Value::Float(fro_sq.sqrt()));
    out.insert(
        "row_indices".into(),
        Value::List(
            s_indices
                .iter()
                .map(|&i| Value::Int(SomaInt::from_i64(i as i64)))
                .collect(),
        ),
    );
    out.insert(
        "col_indices".into(),
        Value::List(
            col_indices
                .iter()
                .map(|&i| Value::Int(SomaInt::from_i64(i as i64)))
                .collect(),
        ),
    );
    Ok(Value::Map(out))
}

// ── regress_sgd (Gilyén-Song-Tang 2009.07268 Algorithm 1) ────────────
//
// Solves min_x (1/2)(||Ax-b||² + λ||x||²) by stochastic gradient descent
// with the importance-sampled gradient estimator of Definition 2.1:
//
//   ∇g(x) = (||A||_F²/||A_r||²) · s · A_rᵀ − Aᵀb + λx
//   where s = (1/C) Σ_j (||A_r||² / A_{r,c_j}²) · A_{r,c_j} · x_{c_j}
//
// Row r ~ ||A_r||²/||A||_F²,  columns c_j ~ |A_{r,c}|²/||A_r||².

fn regress_sgd_impl(a: &Value, b: &Value, opts: &Value) -> Result<Value, RuntimeError> {
    let view = to_matrix_view(a)?;
    let bvec = to_vector(b)?;
    let m = view.rows();
    let n = view.cols();
    if bvec.len() != m {
        return Err(RuntimeError::TypeError(format!(
            "regress_sgd: b has length {}, expected {}",
            bvec.len(),
            m
        )));
    }
    let eps = opt_f64(opts, "eps", 0.01)?;
    let lambda = opt_f64(opts, "lambda", 0.0)?;
    let max_iter = opt_usize(opts, "max_iter", 0)?;
    let inner_samples = opt_usize(opts, "samples_per_iter", 1)?.max(1);
    let user_eta = opt_f64(opts, "eta", 0.0)?;
    let rigorous = match opt_get(opts, "rigorous") {
        Some(Value::Bool(b)) => b,
        _ => false,
    };

    // Precompute: A^T b once  (O(mn))   and the row-norm distribution.
    let dense = view.as_dense();
    let mut atb = vec![0.0f64; n];
    for i in 0..m {
        let bi = bvec[i];
        if bi == 0.0 {
            continue;
        }
        for j in 0..n {
            atb[j] += dense[i][j] * bi;
        }
    }
    let row_sq = view.row_norm_sq();
    let fro_sq = view.fro_sq();
    if fro_sq <= 0.0 {
        return Err(RuntimeError::TypeError("regress_sgd: zero matrix".into()));
    }
    let spec_sq = power_iter_spec_sq(dense).max(1e-12);

    // Step size selection:
    //   - rigorous=true uses the Bach-Moulines bound from
    //     Gilyén-Song-Tang Prop 2.3 (provable convergence, very small η,
    //     astronomical iteration counts).
    //   - default uses the standard SGD heuristic η ≈ 1/(L+λ) with
    //     L = ||A||²  (smoothness of the least-squares objective).
    // Users with their own analysis can pass `eta` directly.
    let mu = if lambda > 0.0 { lambda } else { spec_sq * 1e-6 };
    let eta_rigorous = (eps * eps * mu) / (8.0 * fro_sq * spec_sq + 4.0 * lambda * lambda);
    // Default: η = 1 / (2 ||A||_F²)  keeps the rank-1 update bounded
    // since the per-step amplitude is at most η · ||A||_F² · ||x|| ≈ ||x||/2.
    let eta_default = 1.0 / (2.0 * (fro_sq + lambda) + 1e-12);
    let eta = if user_eta > 0.0 {
        user_eta
    } else if rigorous {
        eta_rigorous
    } else {
        eta_default
    };

    // Iteration count: rigorous formula or default O(n · log(1/ε)).
    let auto_t = if rigorous {
        ((2.0 / (eps * eps)).ln() / (eta * mu)).ceil() as usize + 1
    } else {
        (((n as f64) * 20.0 * (1.0 / eps).ln().max(1.0)).ceil() as usize).max(100)
    };
    let t_total = if max_iter > 0 {
        max_iter
    } else {
        auto_t.max(1)
    };

    // Polyak-Ruppert averaging: return mean of iterates over the
    // second half of the run.  This dramatically reduces variance
    // from the importance-sampled gradient.
    let mut v = vec![0.0f64; n];
    let mut avg = vec![0.0f64; n];
    let avg_start = t_total / 2;
    for t in 0..t_total {
        let r = view.sample_row(&row_sq, fro_sq);
        let row_r = &dense[r];
        let row_sq_r = row_sq[r].max(1e-300);
        // C i.i.d. column samples ~ |A_{r,c}|² / ||A_r||²
        let col_weights: Vec<f64> = row_r.iter().map(|x| x * x).collect();
        let mut grad_scalar = 0.0;
        for _ in 0..inner_samples {
            let cj = view.sample_col_of_row(r, &col_weights, row_sq_r);
            let p = col_weights[cj] / row_sq_r;
            if p <= 0.0 {
                continue;
            }
            grad_scalar += row_r[cj] * v[cj] / p;
        }
        grad_scalar /= inner_samples as f64;
        let scale = fro_sq / row_sq_r * grad_scalar;
        for j in 0..n {
            v[j] = (1.0 - eta * lambda) * v[j] + eta * atb[j] - eta * scale * row_r[j];
        }
        if t >= avg_start {
            for j in 0..n {
                avg[j] += v[j];
            }
        }
    }
    let avg_count = (t_total - avg_start).max(1) as f64;
    for j in 0..n {
        avg[j] /= avg_count;
    }

    let mut out = IndexMap::new();
    out.insert("x".into(), vec_to_value(&avg));
    out.insert("x_last".into(), vec_to_value(&v));
    out.insert("iterations".into(), Value::Int(SomaInt::from_i64(t_total as i64)));
    out.insert("eta".into(), Value::Float(eta));
    out.insert("spec_norm".into(), Value::Float(spec_sq.sqrt()));
    out.insert("fro_norm".into(), Value::Float(fro_sq.sqrt()));
    Ok(Value::Map(out))
}

// ── RMT covariance cleaning (Bouchaud-Potters) ───────────────────────
//
// Sample covariance on a T×N returns matrix has eigenvalues smeared by
// the Marchenko-Pastur distribution when q := N/T is not negligible.
// Cleaning preserves the eigenvectors and replaces the eigenvalues:
//
//   - "clip" (Laloux-Cizeau-Bouchaud-Potters 2000): keep eigenvalues
//     above λ₊ = σ²(1+√q)²; replace the rest with their mean so trace
//     is preserved.
//   - "rie"  (Ledoit-Péché / Bouchaud, Bun-Bouchaud-Potters 2017):
//     ξₖ = λₖ / |1 − q + q · zₖ · g(zₖ)|²,  zₖ = λₖ(1 − iη).
//
// Returns cleaned eigenvalues + the reconstructed matrix.

/// Center each column of an T×N matrix in place (subtract column mean).
fn center_columns(mut r: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    if r.is_empty() {
        return r;
    }
    let t = r.len();
    let n = r[0].len();
    let mut col_mean = vec![0.0f64; n];
    for i in 0..t {
        for j in 0..n {
            col_mean[j] += r[i][j];
        }
    }
    let tf = t as f64;
    for j in 0..n {
        col_mean[j] /= tf;
    }
    for i in 0..t {
        for j in 0..n {
            r[i][j] -= col_mean[j];
        }
    }
    r
}

/// Sample covariance (1/T) RᵀR  given a centered T×N returns matrix.
fn sample_cov_from_centered(r: &[Vec<f64>], t: usize) -> Vec<Vec<f64>> {
    if r.is_empty() {
        return Vec::new();
    }
    let n = r[0].len();
    let mut c = vec![vec![0.0f64; n]; n];
    let inv_t = 1.0 / t as f64;
    for ti in 0..t {
        for i in 0..n {
            let ri = r[ti][i];
            if ri == 0.0 {
                continue;
            }
            for j in 0..n {
                c[i][j] += ri * r[ti][j];
            }
        }
    }
    for i in 0..n {
        for j in 0..n {
            c[i][j] *= inv_t;
        }
    }
    c
}

/// Clipping: keep eigenvalues above the MP edge, replace the rest
/// with their mean so the trace is preserved.
fn clip_eigenvalues(eigvals: &[f64], t: usize, n: usize) -> Vec<f64> {
    let q = n as f64 / t as f64;
    // σ² from the average eigenvalue (= trace(C)/N).
    let mean = eigvals.iter().sum::<f64>() / n as f64;
    let lambda_plus = mean * (1.0 + q.sqrt()).powi(2);
    let (signal, noise): (Vec<&f64>, Vec<&f64>) =
        eigvals.iter().partition(|&&l| l >= lambda_plus);
    let noise_mean = if noise.is_empty() {
        mean
    } else {
        noise.iter().copied().sum::<f64>() / noise.len() as f64
    };
    let _ = signal;
    eigvals
        .iter()
        .map(|&l| if l >= lambda_plus { l } else { noise_mean })
        .collect()
}

/// Rotationally invariant estimator with a Lorentzian-regularised
/// Stieltjes transform.  z_k = λ_k (1 − iη)  with η small.
fn rie_eigenvalues(eigvals: &[f64], t: usize, n: usize, eta: f64) -> Vec<f64> {
    let q = n as f64 / t as f64;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let lk = eigvals[k];
        let z_re = lk;
        let z_im = -eta * lk.abs().max(1e-12);
        // g(z) = (1/N) Σ_j 1/(z − λ_j)  with z = z_re + i z_im
        let mut g_re = 0.0;
        let mut g_im = 0.0;
        for j in 0..n {
            let d_re = z_re - eigvals[j];
            let denom = d_re * d_re + z_im * z_im;
            if denom < 1e-300 {
                continue;
            }
            g_re += d_re / denom;
            g_im += -z_im / denom;
        }
        g_re /= n as f64;
        g_im /= n as f64;
        // factor = 1 − q + q · z · g(z)
        let zg_re = z_re * g_re - z_im * g_im;
        let zg_im = z_re * g_im + z_im * g_re;
        let factor_re = 1.0 - q + q * zg_re;
        let factor_im = q * zg_im;
        let mod_sq = factor_re * factor_re + factor_im * factor_im;
        out.push(lk / mod_sq.max(1e-12));
    }
    out
}

/// Reconstruct A from eigenvectors V and eigenvalues λ: A = V diag(λ) Vᵀ.
fn reconstruct_eigh(v: &[Vec<f64>], eigvals: &[f64]) -> Vec<Vec<f64>> {
    let n = v.len();
    let mut out = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            let mut acc = 0.0;
            for k in 0..n {
                acc += v[i][k] * eigvals[k] * v[j][k];
            }
            out[i][j] = acc;
        }
    }
    out
}

fn clean_covariance_impl(returns: &Value, opts: &Value) -> Result<Value, RuntimeError> {
    let r = to_matrix(returns)?;
    let t = r.len();
    let n = r[0].len();
    if t < 2 {
        return Err(RuntimeError::TypeError(
            "clean_covariance: need at least 2 rows (observations)".into(),
        ));
    }
    let method = match opt_get(opts, "method") {
        Some(Value::String(s)) => s,
        _ => "rie".to_string(),
    };
    let eta = opt_f64(opts, "eta", 0.05)?;
    let center = match opt_get(opts, "center") {
        Some(Value::Bool(b)) => b,
        _ => true,
    };
    let r_centered = if center { center_columns(r) } else { r };
    let c = sample_cov_from_centered(&r_centered, t);
    // Eigendecomposition: for symmetric PSD, jacobi_svd's V == U up to sign,
    // and Σ are the eigenvalues.
    let (_u, eigvals, v) = jacobi_svd(&c);
    let cleaned = match method.as_str() {
        "clip" => clip_eigenvalues(&eigvals, t, n),
        "rie" => rie_eigenvalues(&eigvals, t, n, eta),
        "raw" => eigvals.clone(),
        other => {
            return Err(RuntimeError::TypeError(format!(
                "clean_covariance: unknown method '{}'; expected 'clip', 'rie', or 'raw'",
                other
            )))
        }
    };
    let c_clean = reconstruct_eigh(&v, &cleaned);
    let mut out = IndexMap::new();
    out.insert("matrix".into(), matrix_to_value(&c_clean));
    out.insert("eigenvalues_clean".into(), vec_to_value(&cleaned));
    out.insert("eigenvalues_raw".into(), vec_to_value(&eigvals));
    out.insert("method".into(), Value::String(method));
    out.insert("q".into(), Value::Float(n as f64 / t as f64));
    out.insert("n_obs".into(), Value::Int(SomaInt::from_i64(t as i64)));
    out.insert("n_assets".into(), Value::Int(SomaInt::from_i64(n as i64)));
    Ok(Value::Map(out))
}

// ── Market impact (Bouchaud square-root law) ─────────────────────────
//
// Empirical impact of a metaorder of size Q against daily volume V on
// an asset with daily volatility σ:
//
//   impact = Y · σ · √(Q/V)            (fraction of price)
//   cost_bps = impact · 10_000
//
// Y is an asset-class constant — typically ≈ 1 for equities, larger
// for less liquid assets.  See Tóth et al. arXiv:1105.1694.

fn impact_sqrt_impl(args: &[Value], opts: &Value) -> Result<Value, RuntimeError> {
    let q = val_to_f64(&args[0])?.abs();
    let v = val_to_f64(&args[1])?.max(1e-12);
    let sigma = val_to_f64(&args[2])?.abs();
    let y = opt_f64(opts, "Y", 1.0)?;
    let impact = y * sigma * (q / v).sqrt();
    let cost_bps = impact * 10_000.0;
    let mut out = IndexMap::new();
    out.insert("impact".into(), Value::Float(impact));
    out.insert("bps".into(), Value::Float(cost_bps));
    out.insert("Y".into(), Value::Float(y));
    out.insert("q_over_v".into(), Value::Float(q / v));
    Ok(Value::Map(out))
}

// ── Risk metrics (historical + Gaussian) ─────────────────────────────
//
// All operate on a List<Float> of returns.  Historical estimators make
// no distributional assumption — they're the Bouchaud-Potters baseline
// for risk in markets that aren't Gaussian.

/// Empirical q-quantile of a sample (0 ≤ q ≤ 1).
fn quantile_impl(xs: &Value, q: &Value) -> Result<Value, RuntimeError> {
    let mut v = to_vector(xs)?;
    let q = val_to_f64(q)?.clamp(0.0, 1.0);
    if v.is_empty() {
        return Err(RuntimeError::TypeError(
            "quantile: empty input".into(),
        ));
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    let pos = q * (n - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    Ok(Value::Float(v[lo] * (1.0 - frac) + v[hi] * frac))
}

/// Historical VaR at confidence α (e.g. α=0.99): the (1-α) quantile of
/// the losses.  Convention: returns are positive for gains, so
/// VaR = -quantile(returns, 1-α).
fn var_historical_impl(returns: &Value, opts: &Value) -> Result<Value, RuntimeError> {
    let alpha = opt_f64(opts, "alpha", 0.95)?;
    let q = quantile_impl(returns, &Value::Float(1.0 - alpha))?;
    let q_val = val_to_f64(&q)?;
    Ok(Value::Float(-q_val))
}

/// Historical expected shortfall: mean of returns below the (1-α) quantile.
fn expected_shortfall_historical_impl(
    returns: &Value,
    opts: &Value,
) -> Result<Value, RuntimeError> {
    let v = to_vector(returns)?;
    let alpha = opt_f64(opts, "alpha", 0.95)?;
    let q = quantile_impl(returns, &Value::Float(1.0 - alpha))?;
    let cutoff = val_to_f64(&q)?;
    let below: Vec<f64> = v.iter().copied().filter(|&x| x <= cutoff).collect();
    if below.is_empty() {
        return Ok(Value::Float(-cutoff));
    }
    let mean = below.iter().sum::<f64>() / below.len() as f64;
    Ok(Value::Float(-mean))
}

/// Standard-normal inverse CDF via Acklam's rational approximation.
/// |error| < 1.15e-9.  Used by var_gaussian / expected_shortfall_gaussian.
fn inv_normal_cdf(p: f64) -> f64 {
    let p = p.clamp(1e-15, 1.0 - 1e-15);
    let a = [
        -3.969683028665376e+01,
         2.209460984245205e+02,
        -2.759285104469687e+02,
         1.383577518672690e+02,
        -3.066479806614716e+01,
         2.506628277459239e+00,
    ];
    let b = [
        -5.447609879822406e+01,
         1.615858368580409e+02,
        -1.556989798598866e+02,
         6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    let c = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
         4.374664141464968e+00,
         2.938163982698783e+00,
    ];
    let d = [
         7.784695709041462e-03,
         3.224671290700398e-01,
         2.445134137142996e+00,
         3.754408661907416e+00,
    ];
    let p_low = 0.02425;
    let p_high = 1.0 - p_low;
    if p < p_low {
        let q = (-2.0 * p.ln()).sqrt();
        return (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0);
    }
    if p > p_high {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        return -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0);
    }
    let q = p - 0.5;
    let r = q * q;
    (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
        / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
}

/// Gaussian VaR — assumes N(μ, σ²) and returns the analytical α-VaR.
/// Returns a Map for parity with var_historical, with both μ and σ
/// estimated from the sample if not provided.
fn var_gaussian_impl(returns: &Value, opts: &Value) -> Result<Value, RuntimeError> {
    let v = to_vector(returns)?;
    if v.is_empty() {
        return Err(RuntimeError::TypeError(
            "var_gaussian: empty input".into(),
        ));
    }
    let alpha = opt_f64(opts, "alpha", 0.95)?;
    let mu = opt_f64(opts, "mu", v.iter().sum::<f64>() / v.len() as f64)?;
    let sigma = opt_f64(opts, "sigma", {
        let m = v.iter().sum::<f64>() / v.len() as f64;
        let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64;
        var.sqrt()
    })?;
    // VaR_α = -(μ + σ · Φ⁻¹(1 − α))
    let var = -(mu + sigma * inv_normal_cdf(1.0 - alpha));
    Ok(Value::Float(var))
}

// Power iteration on AᵀA to estimate ||A||² (the squared spectral norm).
// 30 iterations is plenty for the moderate matrices we test on.
fn power_iter_spec_sq(a: &[Vec<f64>]) -> f64 {
    let m = a.len();
    let n = a[0].len();
    let mut x: Vec<f64> = (0..n).map(|_| rand_f64() - 0.5).collect();
    let mut norm = (x.iter().map(|v| v * v).sum::<f64>()).sqrt();
    if norm == 0.0 {
        return 0.0;
    }
    for v in x.iter_mut() {
        *v /= norm;
    }
    let mut lambda = 0.0;
    for _ in 0..30 {
        // y = A x;  z = Aᵀ y
        let mut y = vec![0.0; m];
        for i in 0..m {
            for j in 0..n {
                y[i] += a[i][j] * x[j];
            }
        }
        let mut z = vec![0.0; n];
        for j in 0..n {
            for i in 0..m {
                z[j] += a[i][j] * y[i];
            }
        }
        norm = (z.iter().map(|v| v * v).sum::<f64>()).sqrt();
        if norm < 1e-300 {
            return lambda;
        }
        for j in 0..n {
            x[j] = z[j] / norm;
        }
        lambda = norm;
    }
    lambda
}

// ── dispatch ─────────────────────────────────────────────────────────

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "importance_sample_rows" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "importance_sample_rows expects (matrix, opts)".into(),
                )));
            }
            Some(importance_sample_rows_impl(&args[0], &args[1]))
        }
        "svd_lowrank" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "svd_lowrank expects (matrix, opts)".into(),
                )));
            }
            Some(svd_lowrank_impl(&args[0], &args[1]))
        }
        "regress_sgd" => {
            if args.len() < 3 {
                return Some(Err(RuntimeError::TypeError(
                    "regress_sgd expects (matrix, b, opts)".into(),
                )));
            }
            Some(regress_sgd_impl(&args[0], &args[1], &args[2]))
        }
        "clean_covariance" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "clean_covariance expects (returns: List<List<Float>>, opts: Map)".into(),
                )));
            }
            Some(clean_covariance_impl(&args[0], &args[1]))
        }
        // Bouchaud square-root market impact.
        "impact_sqrt" => {
            if args.len() < 3 {
                return Some(Err(RuntimeError::TypeError(
                    "impact_sqrt expects (qty, daily_volume, sigma, opts?)".into(),
                )));
            }
            let opts = args.get(3).cloned().unwrap_or(Value::Map(IndexMap::new()));
            Some(impact_sqrt_impl(args, &opts))
        }
        // Empirical quantile of a sample.
        "quantile" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "quantile expects (values: List<Float>, q: Float)".into(),
                )));
            }
            Some(quantile_impl(&args[0], &args[1]))
        }
        // Historical VaR (no distributional assumption).
        "var_historical" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "var_historical expects (returns: List<Float>, opts?: Map)".into(),
                )));
            }
            let opts = args.get(1).cloned().unwrap_or(Value::Map(IndexMap::new()));
            Some(var_historical_impl(&args[0], &opts))
        }
        // Historical expected shortfall / CVaR.
        "expected_shortfall_historical" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "expected_shortfall_historical expects (returns, opts?)".into(),
                )));
            }
            let opts = args.get(1).cloned().unwrap_or(Value::Map(IndexMap::new()));
            Some(expected_shortfall_historical_impl(&args[0], &opts))
        }
        // Gaussian VaR — assumes N(μ, σ²).  μ / σ inferred from sample
        // unless overridden.
        "var_gaussian" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "var_gaussian expects (returns: List<Float>, opts?: Map)".into(),
                )));
            }
            let opts = args.get(1).cloned().unwrap_or(Value::Map(IndexMap::new()));
            Some(var_gaussian_impl(&args[0], &opts))
        }
        // Reshape a flat list of numbers into an r×c matrix.
        // mat(rows, cols, list(a, b, c, ...)).
        "mat" => {
            if args.len() < 3 {
                return Some(Err(RuntimeError::TypeError(
                    "mat expects (rows: Int, cols: Int, values: List<Float>)".into(),
                )));
            }
            let r = match val_to_usize(&args[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let c = match val_to_usize(&args[1]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let flat = match to_vector(&args[2]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if flat.len() != r * c {
                return Some(Err(RuntimeError::TypeError(format!(
                    "mat: expected {} values for {}×{} matrix, got {}",
                    r * c,
                    r,
                    c,
                    flat.len()
                ))));
            }
            let mut rows: Vec<Vec<f64>> = Vec::with_capacity(r);
            for i in 0..r {
                rows.push(flat[i * c..(i + 1) * c].to_vec());
            }
            Some(Ok(matrix_to_value(&rows)))
        }
        // rows(r1, r2, r3, ...) — build a matrix from variadic row vectors.
        // Each argument must itself be a List<Float> of the same length.
        // Unlike `list(...)`, this does NOT flatten when the first arg
        // is itself a list — every arg is treated as one row.
        "rows" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "rows() needs at least one row".into(),
                )));
            }
            let mut out: Vec<Vec<f64>> = Vec::with_capacity(args.len());
            let mut n = 0usize;
            for (i, a) in args.iter().enumerate() {
                let row = match to_vector(a) {
                    Ok(v) => v,
                    Err(_) => {
                        return Some(Err(RuntimeError::TypeError(format!(
                            "rows: argument {} must be a List<Float>",
                            i
                        ))))
                    }
                };
                if i == 0 {
                    n = row.len();
                } else if row.len() != n {
                    return Some(Err(RuntimeError::TypeError(format!(
                        "rows: row {} has length {}, expected {}",
                        i,
                        row.len(),
                        n
                    ))));
                }
                out.push(row);
            }
            Some(Ok(matrix_to_value(&out)))
        }
        // cols(c1, c2, c3, ...) — build a matrix from column vectors.
        // Each argument is one column; the result is m × k where m is
        // the column length and k is the number of arguments.
        "cols" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "cols() needs at least one column".into(),
                )));
            }
            let cols: Vec<Vec<f64>> = match args.iter().map(to_vector).collect() {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let m = cols[0].len();
            if cols.iter().any(|c| c.len() != m) {
                return Some(Err(RuntimeError::TypeError(
                    "cols: all columns must have the same length".into(),
                )));
            }
            let k = cols.len();
            let mut out: Vec<Vec<f64>> = vec![vec![0.0; k]; m];
            for j in 0..k {
                for i in 0..m {
                    out[i][j] = cols[j][i];
                }
            }
            Some(Ok(matrix_to_value(&out)))
        }
        // zeros(r, c) — r×c matrix of zeros.
        "zeros" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "zeros expects (r: Int, c: Int)".into(),
                )));
            }
            let r = match val_to_usize(&args[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let c = match val_to_usize(&args[1]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(matrix_to_value(&vec![vec![0.0; c]; r])))
        }
        // ones(r, c) — r×c matrix of ones.
        "ones" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "ones expects (r: Int, c: Int)".into(),
                )));
            }
            let r = match val_to_usize(&args[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let c = match val_to_usize(&args[1]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(matrix_to_value(&vec![vec![1.0; c]; r])))
        }
        // diag(list(d_1, ..., d_n)) — n×n diagonal matrix.
        "diag" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "diag expects (values: List<Float>)".into(),
                )));
            }
            let d = match to_vector(&args[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let n = d.len();
            let mut out = vec![vec![0.0f64; n]; n];
            for i in 0..n {
                out[i][i] = d[i];
            }
            Some(Ok(matrix_to_value(&out)))
        }
        // matrix("1 2 3; 4 5 6") — MATLAB-style literal.
        // Rows separated by ';', entries by whitespace or commas.
        "matrix" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "matrix expects (spec: String)".into(),
                )));
            }
            let s = match &args[0] {
                Value::String(s) => s.clone(),
                _ => {
                    return Some(Err(RuntimeError::TypeError(
                        "matrix: argument must be a String".into(),
                    )))
                }
            };
            let mut rows: Vec<Vec<f64>> = Vec::new();
            for (i, row_s) in s.split(';').enumerate() {
                let row_s = row_s.trim();
                if row_s.is_empty() {
                    continue;
                }
                let entries: Result<Vec<f64>, _> = row_s
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|t| !t.is_empty())
                    .map(|t| t.parse::<f64>())
                    .collect();
                match entries {
                    Ok(v) => rows.push(v),
                    Err(e) => {
                        return Some(Err(RuntimeError::TypeError(format!(
                            "matrix: failed to parse row {}: {}",
                            i, e
                        ))))
                    }
                }
            }
            if rows.is_empty() {
                return Some(Err(RuntimeError::TypeError("matrix: empty spec".into())));
            }
            let n = rows[0].len();
            if rows.iter().any(|r| r.len() != n) {
                return Some(Err(RuntimeError::TypeError(
                    "matrix: rows have inconsistent length".into(),
                )));
            }
            Some(Ok(matrix_to_value(&rows)))
        }
        // Identity matrix as a row-of-rows.
        "eye" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError("eye expects (n: Int)".into())));
            }
            let n = match val_to_usize(&args[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let mut rows: Vec<Vec<f64>> = vec![vec![0.0; n]; n];
            for i in 0..n {
                rows[i][i] = 1.0;
            }
            Some(Ok(matrix_to_value(&rows)))
        }
        // Sample one row index from A according to ℓ² row norms.
        // Returns Map { index, prob, weight, row }.
        "sample_row" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "sample_row expects (matrix)".into(),
                )));
            }
            let view = match to_matrix_view(&args[0]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let row_sq = view.row_norm_sq();
            let fro_sq = view.fro_sq();
            if fro_sq <= 0.0 {
                return Some(Err(RuntimeError::TypeError("sample_row: zero matrix".into())));
            }
            let i = view.sample_row(&row_sq, fro_sq);
            let p = row_sq[i] / fro_sq;
            let row = view.as_dense()[i].clone();
            Some(Ok(map_from_pairs(vec![
                ("index".into(), Value::Int(SomaInt::from_i64(i as i64))),
                ("prob".into(), Value::Float(p)),
                ("weight".into(), Value::Float(row_sq[i].sqrt())),
                ("row".into(), vec_to_value(&row)),
            ])))
        }
        // to_sampled(A)  or  to_sampled(A, map("max_rows", R, "max_cols", C))
        //
        // Builds the BST data structure once.  Returns a Map
        // { __sampled__: handle, rows, cols, fro_norm, kind:"sampled" }
        // that the linalg algorithms can consume.  All subsequent
        // sampling on this handle is O(log m + log n) instead of O(m).
        //
        // The optional opts argument is consumed purely by the budget
        // checker; the runtime ignores it.
        "to_sampled" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "to_sampled expects (matrix: List<List<Float>>) or (matrix, opts: Map)".into(),
                )));
            }
            // If the user already has a sampled handle, return it as-is.
            if as_sampled(&args[0]).is_some() {
                return Some(Ok(args[0].clone()));
            }
            let dense = match to_matrix(&args[0]) {
                Ok(m) => m,
                Err(e) => return Some(Err(e)),
            };
            let sampled = SampledMatrix::from_dense(dense);
            let id = register_sampled(sampled);
            let arc = lookup_sampled(id).expect("just registered");
            Some(Ok(sampled_handle_value(id, &arc)))
        }
        // Free a sampled-matrix registry entry.  Returns true if dropped.
        "drop_sampled" => {
            if args.is_empty() {
                return Some(Err(RuntimeError::TypeError(
                    "drop_sampled expects (handle: Map)".into(),
                )));
            }
            if let Value::Map(m) = &args[0] {
                if let Some(Value::Int(si)) = m.get("__sampled__") {
                    if let Some(id) = si.to_i64() {
                        let removed = SAMPLED_REGISTRY.with(|r| r.borrow_mut().remove(&id).is_some());
                        return Some(Ok(Value::Bool(removed)));
                    }
                }
            }
            Some(Ok(Value::Bool(false)))
        }
        _ => None,
    }
}

// ── tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fmat(rows: &[&[f64]]) -> Value {
        Value::List(
            rows.iter()
                .map(|r| Value::List(r.iter().map(|x| Value::Float(*x)).collect()))
                .collect(),
        )
    }

    fn fvec(xs: &[f64]) -> Value {
        Value::List(xs.iter().map(|x| Value::Float(*x)).collect())
    }

    fn mapof(pairs: &[(&str, Value)]) -> Value {
        let mut m = IndexMap::new();
        for (k, v) in pairs {
            m.insert((*k).into(), v.clone());
        }
        Value::Map(m)
    }

    fn extract_vec(v: &Value) -> Vec<f64> {
        match v {
            Value::List(xs) => xs.iter().map(|x| val_to_f64(x).unwrap()).collect(),
            _ => panic!("not a list"),
        }
    }

    #[test]
    fn impact_sqrt_matches_formula() {
        // Y=1, σ=0.02, Q=10_000, V=1_000_000 → impact = 1 · 0.02 · 0.1 = 0.002
        let r = call_builtin(
            "impact_sqrt",
            &[
                Value::Float(10_000.0),
                Value::Float(1_000_000.0),
                Value::Float(0.02),
                mapof(&[("Y", Value::Float(1.0))]),
            ],
        )
        .unwrap()
        .unwrap();
        if let Value::Map(m) = r {
            let impact = match m.get("impact").unwrap() {
                Value::Float(f) => *f,
                _ => panic!(),
            };
            let bps = match m.get("bps").unwrap() {
                Value::Float(f) => *f,
                _ => panic!(),
            };
            assert!((impact - 0.002).abs() < 1e-9, "impact = {}", impact);
            assert!((bps - 20.0).abs() < 1e-9, "bps = {}", bps);
        }
    }

    #[test]
    fn quantile_matches_sorted_position() {
        // Sample: 1..10, median = 5.5
        let xs = fvec(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
        let r = call_builtin("quantile", &[xs, Value::Float(0.5)]).unwrap().unwrap();
        let q = val_to_f64(&r).unwrap();
        assert!((q - 5.5).abs() < 1e-9, "q50 = {}", q);
    }

    #[test]
    fn var_historical_picks_left_tail() {
        // Returns: 1..100 (gains).  α=0.95 → VaR is the 5th percentile.
        // Sorted, the 5th percentile (q=0.05) is around 5.95.  VaR = -5.95.
        let xs: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let r = call_builtin(
            "var_historical",
            &[
                vec_to_value(&xs),
                mapof(&[("alpha", Value::Float(0.95))]),
            ],
        )
        .unwrap()
        .unwrap();
        let v = val_to_f64(&r).unwrap();
        // With linear interp: pos = 0.05 * 99 = 4.95 → between v[4]=5 and v[5]=6 → 5.95
        assert!((v + 5.95).abs() < 1e-9, "var = {}", v);
    }

    #[test]
    fn expected_shortfall_below_var() {
        // For a fat-tailed return distribution ES should exceed VaR.
        let xs: Vec<f64> = (-50..=50).map(|i| i as f64 * 0.01).collect();
        let var = call_builtin(
            "var_historical",
            &[
                vec_to_value(&xs),
                mapof(&[("alpha", Value::Float(0.99))]),
            ],
        )
        .unwrap()
        .unwrap();
        let es = call_builtin(
            "expected_shortfall_historical",
            &[
                vec_to_value(&xs),
                mapof(&[("alpha", Value::Float(0.99))]),
            ],
        )
        .unwrap()
        .unwrap();
        let var_v = val_to_f64(&var).unwrap();
        let es_v = val_to_f64(&es).unwrap();
        assert!(es_v >= var_v, "ES {} should be ≥ VaR {}", es_v, var_v);
    }

    #[test]
    fn var_gaussian_matches_z_score() {
        // Mean = 0, std = 0.01, α = 0.95 → z = 1.6449, VaR = 0.016449
        let xs: Vec<f64> = (0..1000).map(|_| 0.0).collect();
        // Force μ=0, σ=0.01 via opts.
        let r = call_builtin(
            "var_gaussian",
            &[
                vec_to_value(&xs),
                mapof(&[
                    ("alpha", Value::Float(0.95)),
                    ("mu", Value::Float(0.0)),
                    ("sigma", Value::Float(0.01)),
                ]),
            ],
        )
        .unwrap()
        .unwrap();
        let v = val_to_f64(&r).unwrap();
        assert!((v - 0.016449).abs() < 1e-4, "var = {} (expected ≈ 0.01645)", v);
    }

    #[test]
    fn clean_covariance_beats_sample_cov_on_noisy_setup() {
        // True covariance: rank-1 signal eigenvalue 5 + identity noise.
        // Σ_true = 5 v vᵀ + I  with v a unit vector.
        let n = 20usize;
        let t = 30usize; // q = N/T = 0.66 — well into the noisy regime
        let mut v_signal = vec![0.0; n];
        for i in 0..n {
            v_signal[i] = ((i as f64 + 1.0) * 0.31).sin();
        }
        let v_norm = (v_signal.iter().map(|x| x * x).sum::<f64>()).sqrt();
        for x in v_signal.iter_mut() {
            *x /= v_norm;
        }
        let mut sigma_true = vec![vec![0.0f64; n]; n];
        for i in 0..n {
            sigma_true[i][i] = 1.0;
            for j in 0..n {
                sigma_true[i][j] += 5.0 * v_signal[i] * v_signal[j];
            }
        }
        // Generate T samples from N(0, Σ_true) via Cholesky-free hack:
        // r = L · z  where L is the Jacobi-SVD factor √Σ.
        let (_u_s, s_s, vt_s) = jacobi_svd(&sigma_true);
        // L = V · diag(√s).  Each sample: r_t = L · z_t with z_t i.i.d. N(0,1).
        let mut returns = vec![vec![0.0f64; n]; t];
        for ti in 0..t {
            let mut z = vec![0.0f64; n];
            for j in 0..n {
                // Box-Muller from two rand_f64.
                let u1 = rand_f64().max(1e-15);
                let u2 = rand_f64();
                z[j] = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            }
            for i in 0..n {
                let mut acc = 0.0;
                for k in 0..n {
                    acc += vt_s[i][k] * s_s[k].sqrt() * z[k];
                }
                returns[ti][i] = acc;
            }
        }
        let returns_v = matrix_to_value(&returns);
        // Sample cov (method = "raw") vs cleaned (method = "rie").
        let raw = clean_covariance_impl(
            &returns_v,
            &mapof(&[("method", Value::String("raw".into()))]),
        )
        .unwrap();
        let rie = clean_covariance_impl(
            &returns_v,
            &mapof(&[
                ("method", Value::String("rie".into())),
                ("eta", Value::Float(0.1)),
            ]),
        )
        .unwrap();
        let raw_m = if let Value::Map(m) = &raw { m } else { panic!() };
        let rie_m = if let Value::Map(m) = &rie { m } else { panic!() };
        let raw_mat = to_matrix(raw_m.get("matrix").unwrap()).unwrap();
        let rie_mat = to_matrix(rie_m.get("matrix").unwrap()).unwrap();
        let raw_err = matrix_l2(&raw_mat, &sigma_true);
        let rie_err = matrix_l2(&rie_mat, &sigma_true);
        // RIE must improve over raw sample cov in this regime.
        assert!(
            rie_err < raw_err,
            "RIE error {} >= raw error {}",
            rie_err,
            raw_err
        );
    }

    fn matrix_l2(a: &[Vec<f64>], b: &[Vec<f64>]) -> f64 {
        let mut acc = 0.0;
        for i in 0..a.len() {
            for j in 0..a[0].len() {
                let d = a[i][j] - b[i][j];
                acc += d * d;
            }
        }
        acc.sqrt()
    }

    #[test]
    fn clean_covariance_clip_returns_pd_matrix() {
        // Quick sanity: clipping returns a PSD matrix whose eigenvalues
        // are all ≥ the lower noise mean.
        let n = 8usize;
        let t = 12usize;
        let mut returns = vec![vec![0.0f64; n]; t];
        for ti in 0..t {
            for j in 0..n {
                let u1 = rand_f64().max(1e-15);
                let u2 = rand_f64();
                returns[ti][j] =
                    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            }
        }
        let res = clean_covariance_impl(
            &matrix_to_value(&returns),
            &mapof(&[("method", Value::String("clip".into()))]),
        )
        .unwrap();
        let m = if let Value::Map(m) = res { m } else { panic!() };
        let cleaned = extract_vec(m.get("eigenvalues_clean").unwrap());
        let mn = cleaned.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(mn > 0.0, "cleaned eigenvalues should be positive, got min {}", mn);
    }

    #[test]
    fn sampled_matrix_bst_distribution() {
        // Row norms 9, 4, 1, 0 — row i should be sampled with prob
        // 9/14, 4/14, 1/14, 0/14 respectively.  Take many samples and
        // check the empirical distribution matches.
        let data = vec![
            vec![3.0, 0.0, 0.0],
            vec![0.0, 2.0, 0.0],
            vec![0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0],
        ];
        let m = SampledMatrix::from_dense(data);
        let mut counts = [0u32; 4];
        let trials = 20_000;
        for _ in 0..trials {
            counts[m.sample_row()] += 1;
        }
        let total = trials as f64;
        let p0 = counts[0] as f64 / total;
        let p1 = counts[1] as f64 / total;
        let p2 = counts[2] as f64 / total;
        let p3 = counts[3] as f64 / total;
        // Expected: 9/14, 4/14, 1/14, 0.
        assert!((p0 - 9.0/14.0).abs() < 0.05, "p0 = {}", p0);
        assert!((p1 - 4.0/14.0).abs() < 0.05, "p1 = {}", p1);
        assert!((p2 - 1.0/14.0).abs() < 0.05, "p2 = {}", p2);
        assert!(p3 < 0.02, "p3 = {} (row 3 has zero norm)", p3);
    }

    #[test]
    fn to_sampled_handle_roundtrip() {
        let a = fmat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let h = call_builtin("to_sampled", &[a]).unwrap().unwrap();
        // The handle exposes rows/cols/fro_norm.
        if let Value::Map(m) = &h {
            assert!(matches!(m.get("__sampled__"), Some(Value::Int(_))));
            assert_eq!(m.get("kind"), Some(&Value::String("sampled".into())));
            assert!(matches!(m.get("rows"), Some(Value::Int(_))));
        } else {
            panic!("to_sampled did not return a Map");
        }
        // Re-calling to_sampled on a handle is a no-op (returns same).
        let h2 = call_builtin("to_sampled", &[h.clone()]).unwrap().unwrap();
        if let (Value::Map(m1), Value::Map(m2)) = (&h, &h2) {
            assert_eq!(m1.get("__sampled__"), m2.get("__sampled__"));
        }
        // Algorithms accept the handle.
        let opts = mapof(&[("samples", Value::Int(SomaInt::from_i64(10)))]);
        let r = call_builtin("importance_sample_rows", &[h.clone(), opts])
            .unwrap()
            .unwrap();
        assert!(matches!(r, Value::Map(_)));
    }

    #[test]
    fn matrix_string_parses_basic() {
        let v = call_builtin("matrix", &[Value::String("1 2 3; 4 5 6".into())])
            .unwrap()
            .unwrap();
        let m = to_matrix(&v).unwrap();
        assert_eq!(m, vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    }

    #[test]
    fn matrix_string_handles_commas_and_negatives() {
        let v = call_builtin("matrix", &[Value::String("1.5, -2.3; 0.0, 4.7".into())])
            .unwrap()
            .unwrap();
        let m = to_matrix(&v).unwrap();
        assert_eq!(m, vec![vec![1.5, -2.3], vec![0.0, 4.7]]);
    }

    #[test]
    fn matrix_string_rejects_ragged() {
        let r = call_builtin("matrix", &[Value::String("1 2; 3 4 5".into())]).unwrap();
        assert!(r.is_err());
    }

    #[test]
    fn rows_variadic_builds_matrix() {
        let r1 = fvec(&[1.0, 2.0, 3.0]);
        let r2 = fvec(&[4.0, 5.0, 6.0]);
        let v = call_builtin("rows", &[r1, r2]).unwrap().unwrap();
        let m = to_matrix(&v).unwrap();
        assert_eq!(m, vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
    }

    #[test]
    fn cols_transposes() {
        let c1 = fvec(&[1.0, 2.0]);
        let c2 = fvec(&[3.0, 4.0]);
        let c3 = fvec(&[5.0, 6.0]);
        let v = call_builtin("cols", &[c1, c2, c3]).unwrap().unwrap();
        let m = to_matrix(&v).unwrap();
        assert_eq!(m, vec![vec![1.0, 3.0, 5.0], vec![2.0, 4.0, 6.0]]);
    }

    #[test]
    fn diag_builds_square_diagonal() {
        let v = call_builtin("diag", &[fvec(&[1.0, 2.0, 3.0])]).unwrap().unwrap();
        let m = to_matrix(&v).unwrap();
        assert_eq!(
            m,
            vec![
                vec![1.0, 0.0, 0.0],
                vec![0.0, 2.0, 0.0],
                vec![0.0, 0.0, 3.0],
            ]
        );
    }

    #[test]
    fn jacobi_svd_recovers_diagonal() {
        // A = diag(3, 2, 1) embedded in 4×3.
        let a = vec![
            vec![3.0, 0.0, 0.0],
            vec![0.0, 2.0, 0.0],
            vec![0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0],
        ];
        let (_u, s, _v) = jacobi_svd(&a);
        assert!((s[0] - 3.0).abs() < 1e-9, "σ₁ = {}", s[0]);
        assert!((s[1] - 2.0).abs() < 1e-9, "σ₂ = {}", s[1]);
        assert!((s[2] - 1.0).abs() < 1e-9, "σ₃ = {}", s[2]);
    }

    #[test]
    fn jacobi_svd_reconstructs() {
        let a = vec![
            vec![1.0, 2.0, 3.0],
            vec![4.0, 5.0, 6.0],
            vec![7.0, 8.0, 10.0],
        ];
        let (u, s, v) = jacobi_svd(&a);
        // A ≈ U Σ Vᵀ
        let n = a[0].len();
        let m = a.len();
        for i in 0..m {
            for j in 0..n {
                let mut acc = 0.0;
                for k in 0..n {
                    acc += u[i][k] * s[k] * v[j][k];
                }
                assert!((acc - a[i][j]).abs() < 1e-8, "({},{}) {} vs {}", i, j, acc, a[i][j]);
            }
        }
    }

    #[test]
    fn importance_sample_rows_returns_valid_indices() {
        let a = fmat(&[&[1.0, 0.0], &[0.0, 10.0], &[0.0, 0.0]]);
        let opts = mapof(&[("samples", Value::Int(SomaInt::from_i64(50)))]);
        let res = importance_sample_rows_impl(&a, &opts).unwrap();
        let m = match &res {
            Value::Map(m) => m,
            _ => panic!(),
        };
        // The all-zero row 2 should never be sampled (prob 0).
        let indices = match m.get("indices").unwrap() {
            Value::List(xs) => xs,
            _ => panic!(),
        };
        for v in indices {
            let i = match v {
                Value::Int(si) => si.to_i64().unwrap(),
                _ => panic!(),
            };
            assert!(i == 0 || i == 1, "got index {}", i);
        }
    }

    #[test]
    fn svd_lowrank_recovers_rank1() {
        // Rank-1 matrix u vᵀ with u, v unit vectors and σ = 5.
        let m = 30;
        let n = 20;
        let u: Vec<f64> = (0..m).map(|i| (i as f64 + 1.0)).collect();
        let v: Vec<f64> = (0..n).map(|j| (j as f64 + 1.0)).collect();
        let u_norm = (u.iter().map(|x| x * x).sum::<f64>()).sqrt();
        let v_norm = (v.iter().map(|x| x * x).sum::<f64>()).sqrt();
        let u_unit: Vec<f64> = u.iter().map(|x| x / u_norm).collect();
        let v_unit: Vec<f64> = v.iter().map(|x| x / v_norm).collect();
        let sigma_true = 5.0;
        let mut mat = vec![vec![0.0; n]; m];
        for i in 0..m {
            for j in 0..n {
                mat[i][j] = sigma_true * u_unit[i] * v_unit[j];
            }
        }
        let opts = mapof(&[
            ("row_samples", Value::Int(SomaInt::from_i64(20))),
            ("col_samples", Value::Int(SomaInt::from_i64(15))),
            ("rank", Value::Int(SomaInt::from_i64(1))),
        ]);
        let av = matrix_to_value(&mat);
        let res = svd_lowrank_impl(&av, &opts).unwrap();
        let m = match res {
            Value::Map(m) => m,
            _ => panic!(),
        };
        let s = extract_vec(m.get("S").unwrap());
        // The top singular value should be within ~5% of sigma_true.
        assert!(
            (s[0] - sigma_true).abs() < 0.3,
            "top singular value {} vs {}",
            s[0],
            sigma_true
        );
    }

    #[test]
    fn regress_sgd_converges_diagonal() {
        // Well-conditioned diagonal-dominant system with a known x*.
        // m = 20, n = 5.  A is 5 stacked identity blocks plus small noise.
        let m_dim = 20;
        let n_dim = 5;
        let mut a = vec![vec![0.0; n_dim]; m_dim];
        for i in 0..m_dim {
            a[i][i % n_dim] = 1.0;
            for j in 0..n_dim {
                if j != i % n_dim {
                    a[i][j] = 0.05 * (((i * 7 + j * 3) % 11) as f64 - 5.0) / 5.0;
                }
            }
        }
        let x_star = vec![1.0, -2.0, 3.0, -1.5, 0.5];
        let b: Vec<f64> = (0..m_dim)
            .map(|i| (0..n_dim).map(|j| a[i][j] * x_star[j]).sum())
            .collect();

        let av = matrix_to_value(&a);
        let bv = vec_to_value(&b);
        let opts = mapof(&[
            ("eps", Value::Float(0.05)),
            ("lambda", Value::Float(0.0)),
            ("samples_per_iter", Value::Int(SomaInt::from_i64(4))),
            ("max_iter", Value::Int(SomaInt::from_i64(50_000))),
        ]);
        let res = regress_sgd_impl(&av, &bv, &opts).unwrap();
        let m = match res {
            Value::Map(m) => m,
            _ => panic!(),
        };
        let x = extract_vec(m.get("x").unwrap());
        let mut residual_sq = 0.0;
        let mut b_sq = 0.0;
        for i in 0..m_dim {
            let mut ax_i = 0.0;
            for j in 0..n_dim {
                ax_i += a[i][j] * x[j];
            }
            residual_sq += (ax_i - b[i]).powi(2);
            b_sq += b[i].powi(2);
        }
        let rel = (residual_sq / b_sq).sqrt();
        assert!(
            rel < 0.25,
            "relative residual {} too large; x = {:?}",
            rel,
            x
        );
    }
}
