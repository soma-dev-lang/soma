# Learn Soma in six lessons

You are probably a language model. This course is written for you: six
short lessons, each a program, the commands to run on it, and what the
toolchain answered. **Every output below is real**: the site build writes
each program to an empty folder, runs each command with the released
`soma` binary and pastes what it printed. If the toolchain changes, the
course changes with it, and the build fails if a lesson stops behaving as
it says.

Why learn it? Because Soma is built around the way you work:

- **You guess, it corrects.** You will write `null`, `{}` and other
  habits of other languages. `soma check` answers before anything runs,
  and each error names the Soma form to write.
- **You can show the user evidence.** `soma verify` proves lifecycles and
  limits for every input. It also says which ones it only checks at
  runtime, so your report can quote the verdict instead of saying "I
  believe it is correct".
- **You never need the whole program in context.** Past one file, you
  write the structure first (`skeleton`). The compiler then gives you a
  one-screen brief per function to write, and it refuses a body that does
  more than its brief allows.
- **The models you call are kept on a leash.** `think()` runs inside a
  state machine with a token bound the compiler proves; tests mock it, so
  they run offline.

Install: `curl -fsSL https://soma-lang.dev/install.sh | sh`. Then type
along, or just read: the outputs are here.

## Lesson 1 — A cell is a whole program

A `cell` holds its contract (`face`), its storage (`memory`) and its
handlers (`on`). No framework, no database setup: a `[persistent]` slot
is stored for you, and each handler call is one transaction.

```soma file=counter.cell
cell Counter {
    face {
        signal bump(name: String) -> Int
        signal top(n: Int) -> List
    }
    memory { hits: Map<String, Int> [persistent] }
    on bump(name: String) {
        let n = (hits.get(name) ?? 0) + 1      // `??`: a default for a missing key
        hits.set(name, n)
        return n
    }
    on top(n: Int) {
        return entries(hits) |> sort_by(e => 0 - e.value) |> top(n) |> map(e => e.key)
    }
}
```

```run
soma check counter.cell
soma run counter.cell bump ada
soma run counter.cell bump ada
soma run counter.cell bump alan
soma run counter.cell top 1
```

The slot kept its values between runs (in `.soma_data/` beside the file).
`soma serve counter.cell` would answer the same handlers over HTTP:
`POST /bump/ada`.

## Lesson 2 — Guess wrong, read the fix

Here is what a model writes on its first try, carrying the habits of
Python and JavaScript:

```soma file=cart.cell
cell Shop {
    memory { carts: Map<String, Map> [persistent] }
    on add(user: String, sku: String) {
        let cart = carts.get(user) ?? {}
        cart[sku] = (cart[sku] ?? 0) + 1
        carts.set(user, cart)
        return cart
    }
}
```

```run
!soma check cart.cell
```

The error says what to write. After that fix, the next mistakes show up:

```soma file=cart.cell
cell Shop {
    memory { carts: Map<String, Map> [persistent] }
    on add(user: String, sku: String) {
        let cart = carts.get(user) ?? map()
        if cart[sku] == null { cart[sku] = 0 }
        cart[sku] = cart[sku] + 1
        carts.set(user, cart)
        return cart
    }
    on count(user: String) {
        return len(cart)
    }
}
```

```run
!soma check cart.cell
```

`soma check` reports every error it finds in one pass, each with a stable
code (`soma explain E0091`). Fix them and run the program:

```soma file=cart.cell
cell Shop {
    memory { carts: Map<String, Map> [persistent] }
    on add(user: String, sku: String) {
        let cart = carts.get(user) ?? map()
        cart[sku] = (cart[sku] ?? 0) + 1
        carts.set(user, cart)
        return cart
    }
    on count(user: String) {
        return len(carts.get(user) ?? map())
    }
}
```

```run
soma check cart.cell
soma run cart.cell add ada book
soma run cart.cell add ada book
soma run cart.cell count ada
```

That is the loop: write, `soma check`, apply the fix it names. It
converges because the messages are written for the reader who acts on
them, and that reader is you.

## Lesson 3 — A rule next to the data, proven

A quota of 100 units per user. The rule is one line in `memory`. The
runtime refuses any write that would break it, and `soma verify` tries to
prove that no write ever can.

```soma file=quota.cell
cell Quota {
    memory {
        used: Map<String, Int> [persistent]
        invariant used >= 0 && used <= 100
    }
    on consume(user: String, n: Int) {
        require n > 0 else BadAmount
        let before = used.get(user) ?? 0
        require before + n <= 100 else QuotaExceeded
        used.set(user, before + n)
        return 100 - (before + n)
    }
}
```

```run
soma verify quota.cell
soma run quota.cell consume ana 60
!soma run quota.cell consume ana 60
soma run quota.cell consume ana 40
```

✓ means proven for every input, not just tested. Now forget the
`QuotaExceeded` line, the kind of omission that ships to production in
other languages:

```soma file=quota.cell
cell Quota {
    memory {
        used: Map<String, Int> [persistent]
        invariant used >= 0 && used <= 100
    }
    on consume(user: String, n: Int) {
        require n > 0 else BadAmount
        let before = used.get(user) ?? 0
        used.set(user, before + n)
        return 100 - (before + n)
    }
}
```

```run
soma verify quota.cell
soma run --fresh quota.cell consume ana 60
!soma run quota.cell consume ana 60
```

The bound is no longer proven. The ⚠ line says why and which `require`
would prove it, and the runtime still refuses the bad write and leaves the
slot unchanged. `soma verify --strict` turns every ⚠ into a failure: use
it in CI.

## Lesson 4 — A lifecycle the compiler checks

Most bugs in business software are a step taken out of order: paid
before approved, shipped after cancelled. In Soma the lifecycle is a
`state` machine next to the handlers, and the properties you care about
go in `soma.toml`:

```soma file=expense.cell
cell Expense {
    state flow {
        initial: submitted
        submitted -> approved
        submitted -> rejected
        approved -> paid
        submitted -> paid          // "small amounts skip approval"
    }
    on sign_off(id: String) { transition(id, "submitted", "approved") }
    on reject(id: String)   { transition(id, "submitted", "rejected") }
    on pay(id: String)      { transition(id, "paid") }
}
```

```toml file=soma.toml
[verify]
eventually = ["paid", "rejected"]    # every expense ends one way or the other

[verify.before.paid]
requires = ["approved"]              # nothing is paid without approval
```

```run
!soma verify expense.cell
```

The verifier found the shortcut and printed the path that breaks the
rule. Remove the edge, and name the source state in `pay` so the proof
sees where it starts:

```soma file=expense.cell
cell Expense {
    state flow {
        initial: submitted
        submitted -> approved
        submitted -> rejected
        approved -> paid
    }
    on sign_off(id: String) { transition(id, "submitted", "approved") }
    on reject(id: String)   { transition(id, "submitted", "rejected") }
    on pay(id: String)      { transition(id, "approved", "paid") }
}
```

```run
soma verify expense.cell
soma run expense.cell reject e1
!soma run expense.cell pay e1
```

At run time a transition the machine forbids is refused too, and the
error says which state the instance is really in. Handlers are atomic: one that fails leaves
no write and no transition behind.

## Lesson 5 — An agent in a cage, tested offline

`think()` asks a model. In Soma the answer cannot decide what happens
next on its own. `think_json(…, map("as", "Route"))` only accepts one of
the declared variants, `match` must handle every variant, the state
machine bounds where the ticket can go, and `cost` is a token bound
`soma check` proves. Tests mock the model, so they run without a key and
without a network.

```soma file=triage.cell
cell type Route { variants { Billing  Tech  Human { reason: String } } }

cell agent Triage {
    face { signal route(ticket: String, text: String) -> String }
    cost { tokens: 300 }
    memory { routed: Map<String, String> [persistent] }
    state ticket {
        initial: new
        new -> routed
        new -> escalated
    }
    on route(ticket: String, text: String) {
        set_budget(2000)
        let r = try { think_json("Route this ticket: {text}", map("as", "Route", "max_tokens", 300)) }
        if r.error != () {
            transition(ticket, "new", "escalated")
            return "human: the model failed ({r.kind})"
        }
        match r.value {
            Billing          -> { routed.set(ticket, "billing")  transition(ticket, "new", "routed") }
            Tech             -> { routed.set(ticket, "tech")     transition(ticket, "new", "routed") }
            Human { reason } -> { transition(ticket, "new", "escalated")  return "human: {reason}" }
        }
        return routed.get(ticket)
    }
}

cell test TriageTests {
    rules {
        mock think "{\"variant\": \"Billing\"}"
        assert route("t1", "I was charged twice") == "billing"
        mock think "{\"variant\": \"Refund\"}"          // a variant that does not exist
        assert route("t2", "?") == "human: the model failed (json)"
        mock think error "timeout"
        assert route("t3", "help") == "human: the model failed (llm)"
        assert get_status("t3") == "escalated"
    }
}
```

```run
soma check triage.cell
soma test triage.cell
```

The model invented a `Refund` variant; the program sent the ticket to a
human. `soma verify` adds `think-isolated`: the machine's safety
properties hold whatever the model answers.

## Lesson 6 — A large program, skeleton first

Past one file, the risk is no longer a wrong line but a wrong *shape*:
a function that writes what it should not, an edge nobody takes, a body
that drifts from what the design meant. And no model can keep 10,000
lines in context.

So you write the **skeleton** first. It is cells without bodies: memory,
machines, and for each signal a **frame** saying what its body may read,
write, call and which edges it may move. Comments say what each signal is
for.

```soma file=app.cell
use lib::library

// A lending library: members borrow copies; the stock never goes negative.
skeleton Stock {
    memory {
        copies: Map<String, Int> [persistent]
        invariant copies >= 0
    }
    face {
        // Copies of `book` on the shelf; 0 for an unknown book.
        signal available(book: String) -> Int   reads copies
        // Takes one copy off the shelf; false when there is none.
        signal take(book: String) -> Bool       writes copies
        signal give_back(book: String)          writes copies
        signal restock(book: String, n: Int) -> Int
            requires n > 0
            writes copies
    }
}

skeleton Loans {
    memory { holder: Map<String, String> [persistent] }   // loan id → member
    state loan {
        initial: requested
        requested -> out
        requested -> refused
        out -> returned
    }
    face {
        // Lends `book` to `member` under loan `id`: out when a copy was taken.
        signal lend(id: String, member: String, book: String) -> String
            writes holder
            moves requested -> out, requested -> refused
            calls Stock.take
        signal close(id: String, book: String) -> String
            moves out -> returned
            calls Stock.give_back
    }
}
```

```soma file=lib/library.cell
flesh Stock {
    on available(book) { return copies.get(book) ?? 0 }
    on restock(book, n) {
        let have = copies.get(book) ?? 0
        copies.set(book, have + n)
        return have + n
    }
}
```

```toml file=soma.toml
[verify.before.returned]
requires = ["out"]
```

```run
soma check app.cell
soma holes app.cell
soma verify app.cell
```

With two bodies written out of six, the lifecycle and the property in
`soma.toml` are already proven. The
verifier treats each **hole** (a signal with no body yet) as anything its
frame allows, so a ✓ here holds for every body that fits. Now ask for the
**brief** of one hole: it is everything needed to write that one body,
and nothing else.

```run
soma skeleton app.cell Loans.lend
```

Write two bodies from their briefs. The first try reuses a helper it was
not given:

```soma file=lib/library.cell
flesh Stock {
    on available(book) { return copies.get(book) ?? 0 }
    on restock(book, n) {
        let have = copies.get(book) ?? 0
        copies.set(book, have + n)
        return have + n
    }
    on take(book) {
        let n = available(book)
        if n < 1 { return false }
        copies.set(book, n - 1)
        return true
    }
}

flesh Loans {
    on lend(id, member, book) {
        if !Stock.take(book) {
            transition(id, "requested", "refused")
            return "refused"
        }
        holder.set(id, member)
        transition(id, "requested", "out")
        return "out"
    }
}
```

```run
!soma check app.cell
```

The frame of `take` says `writes copies`, which allows reading `copies`
but no calls. Either the body changes, or the design does: the error
names the clause to add if the design means it. Here the body changes:

```soma file=lib/library.cell
flesh Stock {
    on available(book) { return copies.get(book) ?? 0 }
    on restock(book, n) {
        let have = copies.get(book) ?? 0
        copies.set(book, have + n)
        return have + n
    }
    on take(book) {
        let n = copies.get(book) ?? 0
        if n < 1 { return false }
        copies.set(book, n - 1)
        return true
    }
}

flesh Loans {
    on lend(id, member, book) {
        if !Stock.take(book) {
            transition(id, "requested", "refused")
            return "refused"
        }
        holder.set(id, member)
        transition(id, "requested", "out")
        return "out"
    }
}
```

```run
soma check app.cell
soma verify app.cell --summary
soma run app.cell restock dune 1
soma run app.cell lend L1 ana dune
soma run app.cell lend L2 bob dune
!soma run app.cell close L1 dune
```

The writes in `restock` and `take` are proven (`requires n > 0`; `n < 1`
returned first). The second loan found the shelf empty. Two holes are
left, and calling one fails cleanly. `soma skeleton app.cell --freeze`
locks the structure: from then on a body can only fill it, and a change
to the design is a deliberate `--amend`. In a 100-signal program you keep
the same loop: `soma holes`, one brief, one body, `soma check`.

## Where to go next

- `soma docs agent`, or https://soma-lang.dev/llms.txt: the language on
  one page. `soma docs skeleton`: everything about frames, holes and
  locks. `soma docs flesh`: the syntax a body needs, to hand to whoever
  fills a hole.
- `soma example invariant state_machine` lists verified programs with
  those features; `soma example <id>` prints one. Start from a working
  program rather than from nothing.
- https://soma-lang.dev/docs/guarantees.md lists what is proven, what is
  only enforced at runtime, and what is not covered at all. Read it
  before you tell a user that something is safe.
- `soma init myapp`, or `soma init --skeleton myapp` for a large one.
