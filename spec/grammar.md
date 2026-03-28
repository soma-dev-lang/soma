# Soma Language Specification

## 1. Overview

Soma is a declarative, fractal programming language designed for agent-constructed software.
The fundamental unit is the **cell** — a self-similar construct that declares what it is,
what it remembers, and what it contains.

Programs are not sequences of instructions. They are declarations of entities, their
contracts, their state requirements, and their relationships.

## 2. Lexical Structure

### 2.1 Keywords

```
cell    face    memory    interior
given   promise signal   await
on      where   else     require
true    false
```

### 2.2 Built-in Types

```
Int     Float   String   Bool
Map     List    Set      Log
Option  Result
```

### 2.3 Memory Properties

```
persistent  ephemeral
consistent  eventual    local
immutable   versioned
replicated  encrypted
```

### 2.4 Property Functions

```
capacity(n)     ttl(duration)   retain(duration)
replicated(n)   evict(policy)
```

### 2.5 Identifiers

Identifiers start with a letter or underscore, followed by letters, digits, or underscores.
Type names start with an uppercase letter. Value names start with a lowercase letter.

```
identifier     = [a-z_][a-zA-Z0-9_]*
type_name      = [A-Z][a-zA-Z0-9_]*
```

### 2.6 Literals

```
integer_lit    = [0-9]+
float_lit      = [0-9]+ "." [0-9]+
string_lit     = '"' [^"]* '"'
duration_lit   = [0-9]+ ("ms" | "s" | "min" | "h" | "d" | "years")
percentage_lit = [0-9]+ "%"
```

### 2.7 Operators

```
<  >  <=  >=  ==  !=
+  -  *  /
&&  ||  !
->  =>
```

### 2.8 Delimiters

```
{  }  (  )  [  ]  ,  :  .
```

### 2.9 Comments

```
// single line comment
/* multi-line comment */
```

## 3. Grammar

### 3.1 Program

```ebnf
program         = cell_def* ;
```

### 3.2 Cell Definition

```ebnf
cell_def        = "cell" IDENT "{" cell_body "}" ;
cell_body       = section* ;
section         = face_section
                | memory_section
                | interior_section
                | on_section ;
```

### 3.3 Face Section (Contract)

```ebnf
face_section    = "face" "{" face_decl* "}" ;
face_decl       = given_decl
                | promise_decl
                | signal_decl
                | await_decl ;

given_decl      = "given" IDENT ":" type_expr where_clause? ;
promise_decl    = "promise" constraint ;
signal_decl     = "signal" IDENT "(" param_list ")" return_type? ;
await_decl      = "await" IDENT "(" param_list ")" return_type? ;

where_clause    = "where" "{" constraint ("," constraint)* "}" ;
return_type     = "->" type_expr ;
param_list      = (param ("," param)*)? ;
param           = IDENT ":" type_expr ;
```

### 3.4 Memory Section (State)

```ebnf
memory_section  = "memory" "{" slot_decl* "}" ;
slot_decl       = IDENT ":" type_expr "[" property_list "]" ;
property_list   = property ("," property)* ;
property        = IDENT                          // flag property
                | IDENT "(" literal ")" ;        // parameterized property
```

### 3.5 Interior Section (Children)

```ebnf
interior_section = "interior" "{" cell_def* "}" ;
```

### 3.6 Signal Handlers

```ebnf
on_section      = "on" IDENT "(" param_list ")" "{" handler_body "}" ;
handler_body    = statement* ;
statement       = let_stmt
                | emit_stmt
                | require_stmt
                | transform_stmt ;

let_stmt        = "let" IDENT "=" expr ;
emit_stmt       = "signal" IDENT "(" arg_list ")" ;
require_stmt    = "require" constraint "else" IDENT ;
transform_stmt  = IDENT "." IDENT "(" arg_list ")" ;
```

### 3.7 Types

```ebnf
type_expr       = base_type
                | generic_type
                | cell_ref ;

base_type       = "Int" | "Float" | "String" | "Bool" ;
generic_type    = TYPE_IDENT "<" type_expr ("," type_expr)* ">" ;
cell_ref        = IDENT "." IDENT ;              // reference to another cell's type
```

### 3.8 Constraints

```ebnf
constraint      = expr comparator expr           // comparison
                | IDENT "(" arg_list ")"          // predicate call
                | constraint "&&" constraint      // conjunction
                | constraint "||" constraint      // disjunction
                | "!" constraint ;                // negation

comparator      = "<" | ">" | "<=" | ">=" | "==" | "!=" ;
```

### 3.9 Expressions

```ebnf
expr            = literal
                | IDENT
                | expr "." IDENT                  // field access
                | expr "." IDENT "(" arg_list ")" // method call
                | expr bin_op expr                // binary op
                | "!" expr                        // negation
                | "(" expr ")" ;

arg_list        = (expr ("," expr)*)? ;

literal         = integer_lit
                | float_lit
                | string_lit
                | duration_lit
                | percentage_lit
                | "true"
                | "false" ;
```

## 4. Memory Property Algebra

### 4.1 Durability Axis (mutually exclusive)

- `persistent` — survives cell restart, backed by durable storage
- `ephemeral` — lost on restart, in-memory only

Default: `ephemeral`

### 4.2 Consistency Axis (mutually exclusive)

- `consistent` — all readers see the latest write (linearizable)
- `eventual` — readers may see stale data
- `local` — per-instance, no cross-instance guarantees

Default: `local`

### 4.3 Mutability Axis

- `immutable` — append-only / write-once
- `versioned` — writes create new versions, history preserved

Default: mutable (neither flag set)

### 4.4 Redundancy

- `replicated(n)` — maintain n copies across failure domains

### 4.5 Lifecycle

- `ttl(duration)` — entries expire after duration
- `retain(duration)` — entries must be kept for at least duration
- `capacity(n)` — maximum number of entries
- `evict(policy)` — eviction policy when at capacity: `lru`, `lfo`, `fifo`, `random`

### 4.6 Security

- `encrypted` — data encrypted at rest

### 4.7 Contradiction Rules

The compiler MUST reject these combinations:

| Combination | Reason |
|-------------|--------|
| `persistent` + `ephemeral` | mutually exclusive durability |
| `consistent` + `eventual` | mutually exclusive consistency |
| `consistent` + `local` | mutually exclusive consistency |
| `eventual` + `local` | mutually exclusive consistency |
| `immutable` + `evict(*)` | can't evict from immutable store |
| `ttl(*)` + `retain(*)` where ttl < retain | would delete before retention period |
| `ephemeral` + `retain(*)` | can't guarantee retention without persistence |
| `ephemeral` + `replicated(*)` where n > 1 | replication implies durability need |

### 4.8 Implication Rules

The compiler SHOULD apply these:

| If | Then |
|----|------|
| `immutable` | implies `consistent` (no mutation = no staleness) |
| `replicated(n)` where n > 1 | implies `persistent` |
| `retain(*)` | implies `persistent` |

## 5. Signal Matching Rules

### 5.1 Signal Emission

A cell may declare `signal name(params)` in its face. This means the cell emits
this signal. Sibling cells or the parent may handle it.

### 5.2 Signal Awaiting

A cell may declare `await name(params)` in its face. The compiler MUST verify
that some sibling cell emits a matching signal (same name, compatible types).

### 5.3 Signal Handling

A cell may declare `on name(params) { ... }` to handle a signal emitted by a
sibling. The compiler MUST verify that some sibling declares this signal.

### 5.4 Unmatched Signal Rules

- A signal with no handler: WARNING (signal is lost)
- An await with no matching signal: ERROR (cell will block forever)
- An on-handler with no matching signal: ERROR (dead code)

## 6. Promise Composition Rules

### 6.1 Upward Composition

A parent cell's promises must be satisfiable by the combination of its children's
promises and its own signal handlers. The compiler checks this structurally:

- If parent promises "latency < Xms", at least one execution path through
  children must have total promised latency < X.
- If parent promises "exactly_once", the relevant child must also promise
  "exactly_once" and the signal path must not duplicate.
- If parent promises a durability guarantee, the relevant memory slots
  (in self or children) must have compatible properties.

### 6.2 Constraint Propagation

Parent constraints propagate downward to children:

- If parent declares `promise all state encrypted`, every memory slot in
  every descendant must have `encrypted` property.
- If parent declares a latency bound, children's latency promises must
  compose to satisfy it.

## 7. Compilation Phases

```
Source (.cell)
    │
    ▼
[1. Lexing]        → Token stream
    │
    ▼
[2. Parsing]       → AST
    │
    ▼
[3. Resolution]    → Resolved AST (names linked, types inferred)
    │
    ▼
[4. Checking]      → Verified AST
    │                  ├─ Property algebra (contradictions, implications)
    │                  ├─ Signal matching (all signals wired)
    │                  └─ Promise composition (parents satisfied by children)
    │
    ▼
[5. Code Generation] → Target language (Rust / WASM / containers)
```
