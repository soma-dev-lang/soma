#!/usr/bin/env bash
# Multi-user simulator for the Soma messenger.
#
#   1. Start the server in another terminal:
#        soma serve examples/messenger/app.cell -p 8080
#
#   2. (Optional) In a third terminal, watch every event live:
#        curl -N http://localhost:8080/events
#
#   3. Run this script:
#        ./examples/messenger/sim.sh
#
# Drives three users (alice, bob, carol) through a realistic conversation:
# 1:1 + group chat, typing indicators, read receipts, presence updates.

set -euo pipefail
HOST=${HOST:-http://localhost:8080}

post() {  # post <path> <json>
    curl -fsS -X POST "$HOST$1" \
         -H 'Content-Type: application/json' \
         -d "$2" >/dev/null
    sleep 0.4
}

step() {
    echo
    echo "── $1 ──"
}

step "1. register three users"
post /register '{"username":"alice","display_name":"Alice"}'
post /register '{"username":"bob","display_name":"Bob"}'
post /register '{"username":"carol","display_name":"Carol"}'

step "2. everyone comes online"
post /presence '{"user":"alice","status":"online"}'
post /presence '{"user":"bob","status":"online"}'
post /presence '{"user":"carol","status":"online"}'

step "3. alice opens a 1:1 with bob and starts typing"
post /typing '{"user":"alice","thread":"alice:bob","is_typing":true}'
sleep 1
post /send '{"from":"alice","thread":"alice:bob","text":"hey bob, you free for dinner tonight?"}'
post /typing '{"user":"alice","thread":"alice:bob","is_typing":false}'

step "4. bob replies"
post /typing '{"user":"bob","thread":"alice:bob","is_typing":true}'
sleep 1
post /send '{"from":"bob","thread":"alice:bob","text":"sure! 8pm at the usual place?"}'
post /typing '{"user":"bob","thread":"alice:bob","is_typing":false}'

step "5. alice acknowledges (read receipt + reply)"
LAST_BOB=$(curl -fsS "$HOST/thread/alice:bob" | \
           python3 -c 'import json,sys;m=[x for x in json.load(sys.stdin)["messages"] if x["from"]=="bob"];print(m[-1]["id"])')
post /read "{\"message_id\":\"$LAST_BOB\",\"by\":\"alice\"}"
post /send '{"from":"alice","thread":"alice:bob","text":"perfect, see you there"}'

step "6. carol creates a group thread for the three of them"
GROUP=$(curl -fsS -X POST "$HOST/thread/new" \
        -H 'Content-Type: application/json' \
        -d '{"creator":"carol","members":["alice","bob","carol"]}' | \
        python3 -c 'import json,sys;print(json.load(sys.stdin)["thread"])')
echo "  group thread id: $GROUP"

step "7. group conversation"
post /send "{\"from\":\"carol\",\"thread\":\"$GROUP\",\"text\":\"hey both, can i join you for dinner?\"}"
post /send "{\"from\":\"alice\",\"thread\":\"$GROUP\",\"text\":\"of course! 8pm\"}"
post /send "{\"from\":\"bob\",\"thread\":\"$GROUP\",\"text\":\"see you both then\"}"

step "8. everyone reads the group messages (read receipts)"
curl -fsS "$HOST/thread/$GROUP" | \
    python3 -c '
import json,sys,subprocess
d = json.load(sys.stdin)
for m in d["messages"]:
    for reader in ["alice","bob","carol"]:
        if reader != m["from"]:
            subprocess.run(["curl","-fsS","-X","POST",
                "'"$HOST"'/read","-H","Content-Type: application/json",
                "-d", json.dumps({"message_id": m["id"], "by": reader})],
                check=False, stdout=subprocess.DEVNULL)
'

step "9. summary"
echo
echo "  alice's threads:"
curl -fsS "$HOST/threads/alice" | python3 -m json.tool | sed 's/^/    /'
echo
echo "  group history:"
curl -fsS "$HOST/thread/$GROUP" | python3 -m json.tool | sed 's/^/    /'

step "10. bob goes offline"
post /presence '{"user":"bob","status":"offline"}'

echo
echo "✓ done. open http://localhost:8080 in two browsers and sign in as"
echo "  alice and bob to see the resulting state, or curl -N $HOST/events"
echo "  before re-running this script to watch every event live."
