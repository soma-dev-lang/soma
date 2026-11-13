#!/bin/sh
# port 18723
S=${SOMA:-/private/tmp/claude-501/-Users-antoine-soma/42606278-6030-4a1b-bdf3-5eabb6b81862/scratchpad/r12/bin/soma}
cd "$(dirname "$0")"; rm -rf .soma_data
python3 -c "import json; open('big.json','w').write(json.dumps(list(range(6000000))))"
echo "--- handler.cell (les memes corps dans des handlers)"; $S check handler.cell 2>&1 | grep -v "^$" | head -5 | cut -c1-200
echo "--- app.cell (dans every / after)"; $S check app.cell 2>&1 | grep -v "^$" | head -5 | cut -c1-200
SOMA_LLM_MOCK=echo $S serve app.cell -p 18723 > serve.log 2>&1 &
P=$!; sleep 4; echo "RSS du serveur (KiB) : $(ps -o rss= -p $P)  — budget declare 65536"; curl -s -m 3 http://127.0.0.1:18723/look; echo; kill $P; grep "after" serve.log; rm -f big.json
