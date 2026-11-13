#!/bin/bash
# Every tracked .cell file: does `soma check` answer the same with $2 as with $1?
# usage: tools/verdicts.sh <reference soma> <new soma> [<git ref>]     (prints the files whose verdict changed)
# The files are those of <git ref> (default: main) — the programs that existed
# for the reference compiler; what a branch adds since is not its to judge.
ref=$1; new=$2; base=${3:-main}; n=0; changed=0
for f in $(git ls-tree -r --name-only "$base" | grep '\.cell$'); do
  [ -f "$f" ] || continue
  n=$((n+1))
  "$ref" check "$f" >/dev/null 2>&1; a=$?
  "$new" check "$f" >/dev/null 2>&1; b=$?
  if [ $a -ne $b ]; then changed=$((changed+1)); echo "CHANGED $f: $a -> $b  $("$new" check "$f" 2>&1 | grep -m1 '^error' | cut -c1-200)"; fi
done
echo "verdicts: $n files, $changed changed"
