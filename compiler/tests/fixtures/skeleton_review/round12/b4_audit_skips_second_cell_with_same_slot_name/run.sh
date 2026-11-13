#!/bin/sh
S=${SOMA:-/private/tmp/claude-501/-Users-antoine-soma/42606278-6030-4a1b-bdf3-5eabb6b81862/scratchpad/r12/bin/soma}
cd "$(dirname "$0")"; rm -rf .soma_data
$S run v1.cell B.put k 500          # version 1 : pas d'invariant
$S verify v2.cell 2>&1 | grep "invariant c <= 100"
echo "--- v2 (A.c et B.c) : l'audit de demarrage se tait"; $S run v2.cell B.look k
echo "--- ctl (meme programme, A renommee Z) : l'audit parle"; $S run ctl.cell B.look k
rm -rf .soma_data
