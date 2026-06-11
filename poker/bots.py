#!/usr/bin/env python3
"""Two poker bots that play heads-up Texas Hold'em against the FELT server.

    soma serve poker/app.cell -p 8080      # in one terminal
    python3 poker/bots.py 8                 # in another — plays 8 hands

Each bot is deliberately simple: raise once with a strong hole, otherwise
call cheap bets and check. The point isn't the strategy — it's that the
server runs the whole game, evaluates the hands, and the chip total never
moves off its starting value no matter how the bots bet.
"""
import json
import sys
import urllib.request

BASE = "http://localhost:8080"


def call(path):
    with urllib.request.urlopen(BASE + path) as r:
        return json.load(r)


RANK = {c: i for i, c in enumerate("23456789TJQKA")}


def hole_strength(hole):
    """0..1-ish: pairs and big cards score high."""
    a, b = hole.split()
    ra, rb = RANK[a[0]], RANK[b[0]]
    suited = a[1] == b[1]
    if ra == rb:
        return 0.6 + ra / 30          # any pair is strong
    hi, lo = max(ra, rb), min(ra, rb)
    return (hi + lo) / 40 + (0.1 if suited else 0)


def decide(st):
    """Return (move, amount) for the seat to act."""
    bet, mine = st["bet"], st["my_contrib"]
    to_call = bet - mine
    strength = hole_strength(st["hole"])
    if to_call > 0:                                   # facing a bet
        if to_call <= 40 or strength > 0.55:
            return ("call", 0)
        return ("fold", 0)
    # no bet to us: occasionally open, else check
    if strength > 0.62 and bet == 0:
        return ("raise", 30)
    return ("check", 0)


def play_hand(n):
    d = call("/deal")
    if not d.get("ok"):
        print(f"  deal refused: {d.get('reason')}")
        return
    guard = 0
    while guard < 200:
        guard += 1
        t = call("/table")
        if t["live"] == 0:
            break
        # whoever is to act
        seat = call("/state?seat=0")["to_act"]
        st = call(f"/state?seat={seat}")
        move, amt = decide(st)
        res = call(f"/act?seat={seat}&move={move}&amount={amt}")
        if not res.get("ok"):
            # illegal (e.g. can't check facing a bet) → just call
            res = call(f"/act?seat={seat}&move=call&amount=0")
        if res.get("showdown") or res.get("move") == "fold":
            w = res.get("winner", "?")
            print(f"  hand {n}: winner={w} pot={res.get('pot')}")
            return
    print(f"  hand {n}: (did not resolve)")


def main():
    hands = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    print(call("/join?name=alice"))
    print(call("/join?name=bob"))
    start = call("/table")
    print(f"start: alice={start['stack0']} bob={start['stack1']} "
          f"total={start['total_chips']}")
    for n in range(1, hands + 1):
        play_hand(n)
    end = call("/table")
    print(f"end:   alice={end['stack0']} bob={end['stack1']} "
          f"total={end['total_chips']}  (hands={end['hand_no']})")
    assert end["total_chips"] == start["total_chips"], "CHIPS LEAKED!"
    print(f"chips conserved across {end['hand_no']} hands: "
          f"{end['total_chips']} in, {end['total_chips']} out ✓")


if __name__ == "__main__":
    main()
