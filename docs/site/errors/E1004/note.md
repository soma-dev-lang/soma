In Soma, `match` arms are written `pattern -> value`; `=>` is reserved for lambdas (`x => x * 2`). The parser reports one E1004 for each arm that uses `=>`. Replace `=>` with `->` in every arm.
