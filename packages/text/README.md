# text

Words, sentences, n-grams, edit distance, and a line diff. Pure.

```soma
use text

words_of("Hello, World — hello!")              // ["hello", "world", "hello"]
edit_distance("kitten", "sitting")             // 3
diff_lines(old, new)                            // [{op: "=" | "-" | "+", line}]
similarity(a, b)  ngrams(tokens, 2)  term_counts(tokens)  slug(s)  truncate_words(s, 20)
```

`edit_distance` and `diff_lines` are quadratic in the size of their inputs.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/text/text.cell
```
