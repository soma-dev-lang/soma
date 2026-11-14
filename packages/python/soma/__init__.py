"""Call a Soma program from Python.

    import soma
    with soma.load("app.cell") as app:
        app.refund("o1", 40)
        app.call("Ledger.debit", amount=900)   # raises soma.SomaError(kind="requires")

The program runs in a `soma embed` process beside this one; each call is one
atomic handler invocation.
"""
import json
import os
import shutil
import subprocess
import threading

__all__ = ["load", "Program", "SomaError", "LoadError", "Variant"]
__version__ = "3.2.0"

_INT53 = (1 << 53) - 1


class SomaError(Exception):
    """A handler raised: `kind` is the error's kind (`invariant`, `requires`,
    `not_found`, a `require … else Tag` tag, …), `message` the full text."""

    def __init__(self, kind, message):
        super().__init__(message)
        self.kind = kind
        self.message = message


class LoadError(Exception):
    """The program did not load: `errors` holds `soma check`'s diagnostics."""

    def __init__(self, message, errors=None):
        super().__init__(message)
        self.errors = errors or []


class Variant:
    """A value of one of the program's sum types: `Paid { amount: 5 }` is
    Variant("Decision", "Paid", fields={"amount": 5}); `Up(3)` has values=[3]."""

    def __init__(self, type, variant, fields=None, values=None):
        self.type = type
        self.variant = variant
        self.fields = dict(fields or {})
        self.values = list(values or [])

    def __eq__(self, other):
        return (isinstance(other, Variant) and (self.type, self.variant, self.fields, self.values)
                == (other.type, other.variant, other.fields, other.values))

    def __repr__(self):
        inner = ", ".join([repr(v) for v in self.values] + ["%s=%r" % kv for kv in self.fields.items()])
        return "%s.%s(%s)" % (self.type, self.variant, inner)


def _encode(v):
    if isinstance(v, bool) or v is None or isinstance(v, (float, str)):
        return v
    if isinstance(v, int):
        return v if -_INT53 <= v <= _INT53 else {"__soma_int__": str(v)}
    if isinstance(v, Variant):
        out = {"_type": v.type, "_variant": v.variant}
        if v.values:
            out["_values"] = [_encode(x) for x in v.values]
        out.update({k: _encode(x) for k, x in v.fields.items()})
        return out
    if isinstance(v, dict):
        return {str(k): _encode(x) for k, x in v.items()}
    if isinstance(v, (list, tuple)):
        return [_encode(x) for x in v]
    raise TypeError("soma: a %s cannot be passed to a handler (None, bool, int, float, str, list, dict, soma.Variant)" % type(v).__name__)


def _decode(v):
    if isinstance(v, list):
        return [_decode(x) for x in v]
    if isinstance(v, dict):
        if len(v) == 1 and isinstance(v.get("__soma_int__"), str):
            return int(v["__soma_int__"])
        if "_type" in v and "_variant" in v:
            return Variant(v["_type"], v["_variant"],
                           fields={k: _decode(x) for k, x in v.items() if k not in ("_type", "_variant", "_values")},
                           values=[_decode(x) for x in v.get("_values", [])])
        return {k: _decode(x) for k, x in v.items()}
    return v


def _binary():
    named = os.environ.get("SOMA_BIN")
    if named:
        return named
    found = shutil.which("soma")
    if found:
        return found
    home = os.path.expanduser("~/.soma/bin/soma")
    if os.path.exists(home):
        return home
    raise LoadError("soma: the `soma` binary was not found — install it (https://soma-lang.dev) or set SOMA_BIN")


class Program:
    """A loaded Soma program. Handlers are methods: `app.refund("o1", 40)`."""

    def __init__(self, path, env=None):
        self._lock = threading.Lock()
        self._next = 0
        merged = dict(os.environ)
        merged.update(env or {})
        try:
            self._proc = subprocess.Popen([_binary(), "embed", str(path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                          env=merged, text=True, encoding="utf-8", bufsize=1)
        except OSError as e:
            raise LoadError("soma: cannot start `soma embed`: %s" % e)
        line = self._proc.stdout.readline()
        try:
            ready = json.loads(line)
        except ValueError:
            self.close()
            raise LoadError("soma: `soma embed %s` did not start (is the binary 3.0 or later?): %r" % (path, line.strip()))
        if not ready.get("ready"):
            self.close()
            errors = ready.get("errors", [])
            first = errors[0].get("message", "") if errors else ""
            raise LoadError("soma: %s fails `soma check` (%d error(s)): %s" % (path, len(errors), first), errors)
        self._cells = ready["cells"]
        self.version = ready.get("soma")

    def handlers(self):
        """cell → handler → {"params": [{"name", "type"}], "returns": type or None}"""
        return self._cells

    def call(self, handler, *args, **named):
        """Call `handler` (or `Cell.handler`) by position or by parameter name."""
        if args and named:
            raise TypeError("soma: pass arguments by position or by name, not both")
        payload = {k: _encode(v) for k, v in named.items()} if named else [_encode(a) for a in args]
        with self._lock:
            if self._proc.poll() is not None:
                raise SomaError("closed", "soma: the program is closed")
            self._next += 1
            self._proc.stdin.write(json.dumps({"id": self._next, "call": handler, "args": payload}) + "\n")
            self._proc.stdin.flush()
            line = self._proc.stdout.readline()
        if not line:
            raise SomaError("internal", "soma: the `soma embed` process ended during the call")
        reply = json.loads(line)
        if reply.get("ok"):
            return _decode(reply.get("value"))
        error = reply.get("error", {})
        raise SomaError(error.get("kind", "internal"), error.get("message", "soma: the call failed"))

    def __getattr__(self, name):
        if name.startswith("_"):
            raise AttributeError(name)
        if not any(name in handlers for handlers in self._cells.values()):
            raise AttributeError("soma: the program has no handler `%s`" % name)
        return lambda *args, **named: self.call(name, *args, **named)

    def close(self):
        proc = getattr(self, "_proc", None)
        if proc is None or proc.poll() is not None:
            return
        try:
            proc.stdin.close()
            proc.wait(timeout=5)
        except Exception:
            proc.kill()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        self.close()


def load(path, env=None):
    """Load a `.cell` program. `env`: extra environment for it (SOMA_LLM_MOCK, API keys…)."""
    return Program(path, env)
