#!/bin/bash
# Build the reference binary the verdict gate compares with: soma as it is at
# <git-ref> (default: origin/main), in a scratch worktree, copied to
# tools/harness/.ref/soma. Done once at the start of a piece of work:
#
#     tools/harness/make_ref.sh                 # origin/main
#     python3 tools/harness/run.py --ref tools/harness/.ref/soma
set -euo pipefail
ref="${1:-origin/main}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
sha="$(git -C "$root" rev-parse --short "$ref")"
work="$(mktemp -d)/soma-ref"
git -C "$root" worktree add --detach "$work" "$ref" >/dev/null
trap 'git -C "$root" worktree remove --force "$work"' EXIT
# the same target directory as the working tree would rebuild both on every switch
(cd "$work/compiler" && cargo build --release --locked)
mkdir -p "$root/tools/harness/.ref"
cp "$work/compiler/target/release/soma" "$root/tools/harness/.ref/soma"
# (a copied binary loses its signature on macOS and is killed at launch)
[ "$(uname)" = Darwin ] && codesign --force --sign - "$root/tools/harness/.ref/soma" >/dev/null 2>&1 || true
echo "$sha" > "$root/tools/harness/.ref/sha"
echo "reference: soma at $ref ($sha) -> tools/harness/.ref/soma"
