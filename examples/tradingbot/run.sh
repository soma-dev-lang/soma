#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════
# Trading Bot — 4 cells communicating via signal bus
# ═══════════════════════════════════════════════════════════════
set -e

SOMA="${SOMA:-soma}"
DIR="$(cd "$(dirname "$0")" && pwd)"
G='\033[32m'; C='\033[36m'; B='\033[1m'; N='\033[0m'

cleanup() { kill $P1 $P2 $P3 $P4 2>/dev/null; }
trap cleanup EXIT

echo -e "${B}${C}═══ TRADING BOT ═══${N}"
echo -e "  4 cells, 4 processes, 1 signal bus\n"

# Start in order: Market → Trading → Booking → Broker
# Each connects to the previous via signal bus

echo -e "${G}[1]${N} Market  → :8080 (prices every 2s)"
(cd "$DIR" && $SOMA serve market.cell -p 8080) &>/tmp/tb_market.log &
P1=$!; sleep 2

echo -e "${G}[2]${N} Broker  → :8110 (executes orders)"
(cd "$DIR" && $SOMA serve broker.cell -p 8110) &>/tmp/tb_broker.log &
P2=$!; sleep 1

echo -e "${G}[3]${N} Booking → :8100 (computes deltas, connects to Broker)"
cat > /tmp/tb_booking_soma.toml << 'EOF'
[package]
name = "booking"
[peers]
broker = "localhost:8112"
[verify]
deadlock_free = true
[verify.after.sent]
eventually = ["filled", "rejected", "cancelled"]
EOF
cp "$DIR/booking.cell" /tmp/tb_booking.cell
cp /tmp/tb_booking_soma.toml /tmp/tb_booking_soma_dir_soma.toml
mkdir -p /tmp/tb_booking_dir && cp "$DIR/booking.cell" /tmp/tb_booking_dir/app.cell && cp /tmp/tb_booking_soma.toml /tmp/tb_booking_dir/soma.toml
(cd /tmp/tb_booking_dir && $SOMA serve app.cell -p 8100) &>/tmp/tb_booking.log &
P3=$!; sleep 1

echo -e "${G}[4]${N} Trading → :8090 (strategy every 10s, connects to Market + Booking)"
cat > /tmp/tb_trading_soma.toml << 'EOF'
[package]
name = "trading"
[peers]
market = "localhost:8082"
booking = "localhost:8102"
EOF
mkdir -p /tmp/tb_trading_dir && cp "$DIR/trading.cell" /tmp/tb_trading_dir/app.cell && cp /tmp/tb_trading_soma.toml /tmp/tb_trading_dir/soma.toml
(cd /tmp/tb_trading_dir && $SOMA serve app.cell -p 8090) &>/tmp/tb_trading.log &
P4=$!; sleep 2

echo -e "\n${B}All 4 cells running. Signal bus connected.${N}\n"
echo "  Market  → http://localhost:8080/prices"
echo "  Trading → http://localhost:8090/targets"
echo "  Booking → http://localhost:8100/orders"
echo "  Broker  → http://localhost:8110/fills"
echo ""

# Wait for first price cycle + strategy computation
echo "Waiting 12s for market ticks + strategy computation..."
sleep 12

echo -e "\n${C}═══ MARKET PRICES ═══${N}"
curl -s http://localhost:8080/prices | python3 -c "
import sys,json
prices = json.load(sys.stdin)
for p in sorted(prices, key=lambda x: x.get('symbol','')):
    if p.get('symbol'):
        print(f'  {p[\"symbol\"]:6} bid={p[\"bid\"]:>8} ask={p[\"ask\"]:>8} mid={p[\"mid\"]:>8}')
" 2>/dev/null

echo -e "\n${C}═══ TRADING TARGETS ═══${N}"
curl -s http://localhost:8090/targets | python3 -c "
import sys,json
targets = json.load(sys.stdin)
for t in sorted(targets, key=lambda x: x.get('symbol','')):
    if t.get('symbol'):
        qty = t.get('qty','0')
        print(f'  {t[\"symbol\"]:6} qty={qty:>6} side={t.get(\"side\",\"?\"):4} notional={t.get(\"notional\",\"0\"):>8}')
" 2>/dev/null

echo -e "\n${C}═══ BOOKING ORDERS ═══${N}"
curl -s http://localhost:8100/orders | python3 -c "
import sys,json
orders = json.load(sys.stdin)
if not orders:
    print('  (no orders yet — waiting for target_portfolio signal)')
else:
    for o in orders:
        if o.get('id'):
            print(f'  {o[\"symbol\"]:6} {o[\"side\"]:4} qty={o.get(\"qty\",\"0\"):>6} status={o.get(\"status\",\"?\"):10}')
" 2>/dev/null

echo -e "\n${C}═══ BROKER FILLS ═══${N}"
curl -s http://localhost:8110/fills | python3 -c "
import sys,json
fills = json.load(sys.stdin)
if not fills:
    print('  (no fills yet)')
else:
    for f in fills:
        if f.get('id'):
            print(f'  {f[\"symbol\"]:6} {f[\"side\"]:4} qty={f.get(\"qty\",\"0\"):>6} @ {f.get(\"price\",\"0\"):>8}')
" 2>/dev/null

echo -e "\n${C}═══ POSITIONS ═══${N}"
curl -s http://localhost:8100/positions | python3 -c "
import sys,json
pos = json.load(sys.stdin)
if not pos:
    print('  (no positions yet)')
else:
    for p in sorted(pos, key=lambda x: x.get('symbol','')):
        if p.get('symbol'):
            print(f'  {p[\"symbol\"]:6} qty={p.get(\"qty\",\"0\"):>6}')
" 2>/dev/null

echo -e "\n${B}Ctrl+C to stop all cells${N}"
wait
