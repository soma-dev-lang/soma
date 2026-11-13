#!/bin/sh
S=${SOMA:-/private/tmp/claude-501/-Users-antoine-soma/42606278-6030-4a1b-bdf3-5eabb6b81862/scratchpad/r12/bin/soma}
cd "$(dirname "$0")"; rm -rf .soma_data top/.soma_data
echo "=== cellule interieure (runtime { signal iput(\"k\", 500) })"
$S run v1.cell >/dev/null 2>&1; $S run v1.cell >/dev/null 2>&1     # v1 : c[k]=500, instance k -> b -> old
$S verify v2.cell 2>&1 | grep "invariant c"
echo "v2 (invariant c <= 100, etat old retire) :"; $S run v2.cell 2>&1 | grep "stored\|error"
echo "v3 (slot retype String) :"; $S run v3.cell 2>&1 | grep "stored\|error"
echo "=== la meme cellule au niveau superieur"
cd top; $S run v1.cell put k 500 >/dev/null; $S run v1.cell go k >/dev/null; $S run v1.cell go2 k >/dev/null
echo "v2 :"; $S run v2.cell look k 2>&1 | grep "stored data" | cut -c1-150
echo "v3 :"; $S run v3.cell look k 2>&1 | grep "stored data" | cut -c1-150
cd ..; rm -rf .soma_data top/.soma_data
