# soma (JavaScript / TypeScript)

Use a Soma program as the verified core of a Node program: load a `.cell`
file, call its handlers, get typed values back and errors by kind.

```ts
import { load, SomaError } from "soma-lang";

const app = await load("treasury/app.cell");
await app.seed(1000);                                  // a handler, by name
try {
  await app.call("Ledger.debit", { amount: 5000 });    // by parameter name, cell named
} catch (e) {
  if (e instanceof SomaError) console.log(e.kind);     // "invariant", "requires", …
}
await app.close();
```

It needs the `soma` binary (3.0 or later): on `PATH`, at `~/.soma/bin/soma`,
or named by `SOMA_BIN`. No dependency.

What crosses the boundary: `null` ↔ `()`, booleans, numbers, strings, arrays,
plain objects; an Int past 2^53 comes back as a `bigint` (and a `bigint` goes
in as an Int); a value of one of the program's sum types is a `Variant`. Each
call is one atomic handler invocation, with the program's data in
`.soma_data/` beside the `.cell` file. A program that fails `soma check` does
not load (`LoadError`, with the diagnostics).
