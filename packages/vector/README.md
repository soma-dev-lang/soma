# vector

Similarity search over embeddings, and a local embedding that needs no model.

```soma
use vector

cosine(a, b)  dot_product(a, b)  distance(a, b)  unit(a)  centroid(vectors)
nearest(query, rows, 3)                 // rows: [{id, vector}] → [{id, score}], best first
embed_text("refund for a late parcel", 256)
```

`embed_text` is the hashing trick over words and word pairs: deterministic, good for near-duplicates and keyword-level retrieval and for tests — not a semantic model. A model's embeddings are vectors like any other. `nearest` scans every row. Kind: `BadShape`.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/vector/vector.cell
```
