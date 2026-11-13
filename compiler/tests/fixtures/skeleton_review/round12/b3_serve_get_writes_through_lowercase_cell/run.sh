#!/bin/sh
# port 18729
S=${SOMA:-/private/tmp/claude-501/-Users-antoine-soma/42606278-6030-4a1b-bdf3-5eabb6b81862/scratchpad/r12/bin/soma}
cd "$(dirname "$0")"; rm -rf .soma_data
$S check app.cell | tail -1
$S serve app.cell -p 18729 > serve.log 2>&1 &
P=$!; sleep 2
for r in up/k g5/k g6/k g5/k; do echo "GET /$r -> $(curl -s -m 3 http://127.0.0.1:18729/$r)"; done
echo "POST /peek/k -> $(curl -s -m 3 -X POST http://127.0.0.1:18729/peek/k)"
kill $P
