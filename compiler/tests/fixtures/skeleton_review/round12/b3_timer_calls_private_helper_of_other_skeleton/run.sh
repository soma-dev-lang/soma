#!/bin/sh
# usage: sh run.sh   (port 18720)
S=${SOMA:-/private/tmp/claude-501/-Users-antoine-soma/42606278-6030-4a1b-bdf3-5eabb6b81862/scratchpad/r12/bin/soma}
cd "$(dirname "$0")"; rm -rf .soma_data trace.log
$S check app.cell | tail -1
SOMA_EFFECT_TRACE=$PWD/trace.log $S serve app.cell -p 18720 > serve.log 2>&1 &
P=$!; sleep 3; curl -s -m 3 http://127.0.0.1:18720/look/x; echo; kill $P
sort trace.log | uniq -c
