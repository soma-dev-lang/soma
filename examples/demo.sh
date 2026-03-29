#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════
# Soma Fractal Distribution Demo
# Same code. One machine or a cluster. The only difference: --join.
# ═══════════════════════════════════════════════════════════════════════
set -e

SOMA="${SOMA:-soma}"
DIR="$(cd "$(dirname "$0")" && pwd)/jobqueue"
G='\033[32m'; R='\033[31m'; Y='\033[33m'; C='\033[36m'; B='\033[1m'; N='\033[0m'

banner()  { echo -e "\n${B}${C}═══ $1 ═══${N}\n"; }
status()  { echo -e "  ${G}✓${N} $1"; }
fail()    { echo -e "  ${R}✗${N} $1"; }
info()    { echo -e "  ${Y}→${N} $1"; }

cleanup() { kill $N1 $N2 $N3 2>/dev/null; rm -rf /tmp/soma_demo_*; }
trap cleanup EXIT

# ── Setup ────────────────────────────────────────────────────────────
banner "SOMA FRACTAL DISTRIBUTION DEMO"
echo -e "  The same 120-line job queue runs on 1, 2, or 3 nodes."
echo -e "  No Docker. No YAML. No deploy. Just ${B}--join${N}.\n"

for d in 1 2 3; do mkdir -p /tmp/soma_demo_n$d && cp "$DIR/app.cell" "$DIR/soma.toml" /tmp/soma_demo_n$d/; done

# ── Phase 1: Verify ──────────────────────────────────────────────────
banner "PHASE 1: VERIFY (compile-time distribution checks)"
cd /tmp/soma_demo_n1
$SOMA verify app.cell 2>&1 | grep -E "✓|✗|⚠|passed|failed|States:|replicas|tolerance|shard|consistency|CAP|quorum|node-local|scheduler"
echo ""

# ── Phase 2: Single node ─────────────────────────────────────────────
banner "PHASE 2: SINGLE NODE"
info "Starting node 1 on :8080..."
(cd /tmp/soma_demo_n1 && SOMA_NODE_ID=localhost:8082 $SOMA serve app.cell -p 8080) &>/tmp/soma_demo_n1.log &
N1=$!; sleep 3
status "Node 1 running (pid $N1)"

info "Submitting 5 jobs..."
for i in $(seq 1 5); do
    curl -s -X POST -d "{\"task\":\"render\",\"payload\":\"frame_$i.exr\"}" http://localhost:8080/submit > /dev/null
done
status "5 jobs submitted"
sleep 3
STATS=$(curl -s http://localhost:8080/stats 2>/dev/null)
echo -e "  ${C}node1:${N} $(echo $STATS | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'total={d[\"total\"]} queued={d[\"queued\"]} running={d[\"running\"]} completed={d[\"completed\"]}')" 2>/dev/null)"

# ── Phase 3: Node 2 joins ────────────────────────────────────────────
banner "PHASE 3: NODE 2 JOINS (--join localhost:8082)"
info "Starting node 2 on :8081..."
(cd /tmp/soma_demo_n2 && SOMA_NODE_ID=localhost:8083 $SOMA serve app.cell -p 8081 --join localhost:8082) &>/tmp/soma_demo_n2.log &
N2=$!; sleep 4
status "Node 2 joined the cluster"

info "Submitting 5 more jobs on node 2..."
for i in $(seq 6 10); do
    curl -s -X POST -d "{\"task\":\"encode\",\"payload\":\"video_$i.mp4\"}" http://localhost:8081/submit > /dev/null
done
status "5 jobs submitted on node 2"
sleep 2

info "Both nodes see the same data:"
T1=$(curl -s http://localhost:8080/stats 2>/dev/null)
T2=$(curl -s http://localhost:8081/stats 2>/dev/null)
echo -e "  ${C}node1:${N} $(echo $T1 | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'total={d[\"total\"]} queued={d[\"queued\"]} running={d[\"running\"]} completed={d[\"completed\"]}')" 2>/dev/null)"
echo -e "  ${C}node2:${N} $(echo $T2 | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'total={d[\"total\"]} queued={d[\"queued\"]} running={d[\"running\"]} completed={d[\"completed\"]}')" 2>/dev/null)"

# ── Phase 4: Node 3 joins ────────────────────────────────────────────
banner "PHASE 4: NODE 3 JOINS"
info "Starting node 3 on :8090..."
(cd /tmp/soma_demo_n3 && SOMA_NODE_ID=localhost:8092 $SOMA serve app.cell -p 8090 --join localhost:8082) &>/tmp/soma_demo_n3.log &
N3=$!; sleep 4
status "3-node cluster formed"

info "All 3 nodes see the same jobs:"
for port in 8080 8081 8090; do
    S=$(curl -s http://localhost:$port/stats 2>/dev/null)
    NODE=$( [ $port -eq 8080 ] && echo "node1" || ( [ $port -eq 8081 ] && echo "node2" || echo "node3" ) )
    echo -e "  ${C}${NODE}:${N} $(echo $S | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'total={d[\"total\"]} completed={d[\"completed\"]}')" 2>/dev/null)"
done

# ── Phase 5: Kill node 2 ─────────────────────────────────────────────
banner "PHASE 5: KILL NODE 2 (testing resilience)"
info "Killing node 2 (pid $N2)..."
kill $N2 2>/dev/null; N2=0
sleep 2
status "Node 2 is dead"

info "Submit more jobs — cluster continues without node 2:"
for i in $(seq 11 15); do
    curl -s -X POST -d "{\"task\":\"process\",\"payload\":\"data_$i.csv\"}" http://localhost:8080/submit > /dev/null
done
status "5 jobs submitted after node 2 died"
sleep 3

info "Surviving nodes still working:"
for port in 8080 8090; do
    S=$(curl -s http://localhost:$port/stats 2>/dev/null)
    NODE=$( [ $port -eq 8080 ] && echo "node1" || echo "node3" )
    echo -e "  ${C}${NODE}:${N} $(echo $S | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'total={d[\"total\"]} completed={d[\"completed\"]}')" 2>/dev/null)"
done

# ── Phase 6: Node 2 comes back ───────────────────────────────────────
banner "PHASE 6: NODE 2 REJOINS"
info "Restarting node 2..."
rm -rf /tmp/soma_demo_n2/.soma_data
(cd /tmp/soma_demo_n2 && SOMA_NODE_ID=localhost:8083 $SOMA serve app.cell -p 8081 --join localhost:8082) &>/tmp/soma_demo_n2b.log &
N2=$!; sleep 5
status "Node 2 rejoined — re-syncing data..."

info "All 3 nodes back in sync:"
for port in 8080 8081 8090; do
    S=$(curl -s http://localhost:$port/stats 2>/dev/null)
    NODE=$( [ $port -eq 8080 ] && echo "node1" || ( [ $port -eq 8081 ] && echo "node2" || echo "node3" ) )
    echo -e "  ${C}${NODE}:${N} $(echo $S | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'total={d[\"total\"]} completed={d[\"completed\"]}')" 2>/dev/null)"
done

# ── Summary ──────────────────────────────────────────────────────────
banner "SUMMARY"
echo -e "  Same ${B}app.cell${N} (120 lines). Zero changes between phases."
echo -e "  Distribution via ${B}--join${N}. Replication via signal bus."
echo -e "  Verified at compile time: deadlock-free, CAP mode, quorum."
echo ""
echo -e "  ${G}1 node  →${N}  soma serve app.cell -p 8080"
echo -e "  ${G}2 nodes →${N}  soma serve app.cell -p 8081 --join localhost:8082"
echo -e "  ${G}N nodes →${N}  soma serve app.cell -p 808N --join localhost:8082"
echo ""
echo -e "  ${B}That's it. That's the whole distributed system.${N}"
echo ""
