"""Run: SOMA_BIN=…/soma python3 packages/python/tests/test_embed.py <a copy of packages/fixture/app.cell>"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import soma

program = sys.argv[1]

with soma.load(program) as app:
    assert app.version.startswith("3."), app.version
    info = app.handlers()["Ledger"]["debit"]
    assert info["params"] == [{"name": "amount", "type": "Int"}] and info["returns"] == "Int", info

    # typed values in and out
    assert app.seed(1000) == 1000
    assert app.debit(300) == 700
    assert app.call("Ledger.debit", amount=200) == 500
    assert app.balance() == 500
    assert app.big() == 2 ** 70, app.big()
    assert app.echo(2 ** 70 + 1) == 2 ** 70 + 1
    assert app.echo(2 ** 60) == 2 ** 60
    for value in [None, True, 1.5, "é", [1, [2, None]], {"k": {"n": 1}}]:
        assert app.echo(value) == value, value
    assert app.find("r1") == {"id": "r1", "tags": ["a", "b"]}
    assert app.shout("hi") == "HI"            # its print() went to stderr, not into the protocol

    # the program's own sum types
    assert app.classify(40) == soma.Variant("Decision", "Pay", fields={"amount": 40})
    assert app.classify(900).variant == "Hold" and app.classify(0) == soma.Variant("Decision", "Skip")
    assert app.label(app.classify(40)) == "pay 40"
    assert app.label(soma.Variant("Decision", "Hold", fields={"reason": "manual"})) == "hold: manual"

    # errors by kind — and a failed call leaves nothing behind
    def kind_of(f):
        try:
            f()
        except soma.SomaError as e:
            return e.kind
        return None
    assert kind_of(lambda: app.debit(5000)) == "invariant"
    assert app.balance() == 500
    assert kind_of(lambda: app.debit(-1)) == "requires"
    assert kind_of(lambda: app.find("")) == "BadId"
    assert kind_of(lambda: app.find("ghost")) == "not_found"
    assert kind_of(lambda: app.debit("ten")) == "type"
    assert kind_of(lambda: app.call("nope")) == "no_handler"
    assert kind_of(lambda: app.call("debit", sum=1)) == "type"
    assert kind_of(lambda: app.label(soma.Variant("Decision", "Refund"))) == "type"
    try:
        app.nope
        raise AssertionError("an unknown handler is no attribute")
    except AttributeError:
        pass

# the data is the program's: it is there for the next process
with soma.load(program) as again:
    assert again.balance() == 500

# a program that fails `soma check` does not load
broken = os.path.join(os.path.dirname(program), "broken.cell")
open(broken, "w").write("cell B {\n    on f() { return undefined_name }\n}\n")
try:
    soma.load(broken)
    raise AssertionError("a broken program loaded")
except soma.LoadError as e:
    assert e.errors and "undefined" in e.errors[0]["message"], e.errors

print("python: ok")
