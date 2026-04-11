# Adding Builtins to Soma

## Pattern

Every builtin follows the same pattern. Pick the right file, add a match arm, done.

### Files

| File | Category | Examples |
|------|----------|---------|
| `builtins/math.rs` | Math functions | abs, sqrt, log, pow, clamp, random |
| `builtins/string.rs` | String operations | len, contains, replace, split, trim |
| `builtins/collection.rs` | List/Map operations | list, map, push, nth, sort, reverse, range |
| `builtins/pipeline.rs` | Pipeline operators | filter_by, sort_by, top, pluck, group_by, distinct |
| `builtins/io.rs` | I/O and HTTP responses | print, read_file, html, response |
| `builtins/time.rs` | Time functions | now, now_ms, today |
| `builtins/http.rs` | HTTP client | http_get, http_post |
| `builtins/record.rs` | Record operations | is_type |
| `builtins/storage.rs` | Storage methods | needs &Interpreter |

All files are in `compiler/src/interpreter/builtins/`.

## Step 1: Add the function

Open the right file. Find the `match name {` block. Add your arm before `_ => None`.

### Template: simple function (1 arg → 1 result)

```rust
"my_func" => {
    args.first().map(|arg| match arg {
        Value::Int(n) => Ok(Value::Int(/* your logic */)),
        Value::Float(n) => Ok(Value::Float(/* your logic */)),
        _ => Err(RuntimeError::TypeError("my_func expects a number".to_string())),
    })
}
```

### Template: function with N args

```rust
"my_func" => {
    if args.len() >= 2 {
        let a = &args[0];
        let b = &args[1];
        // your logic
        Some(Ok(Value::Int(result)))
    } else {
        Some(Err(RuntimeError::TypeError("my_func(a, b)".to_string())))
    }
}
```

### Template: list transform (pipeline operator)

```rust
"my_transform" => {
    if args.len() >= 2 {
        if let Value::List(items) = &args[0] {
            let field = format!("{}", args[1]);
            let result: Vec<Value> = items.iter().map(|item| {
                if let Value::Map(entries) = item {
                    let val = map_field_f64(item, &field);
                    let mut new_entries = entries.clone();
                    new_entries.push(("new_field".to_string(), Value::Float(val * 2.0)));
                    Value::Map(new_entries)
                } else {
                    item.clone()
                }
            }).collect();
            Some(Ok(Value::List(result)))
        } else {
            Some(Ok(Value::List(vec![])))
        }
    } else {
        Some(Err(RuntimeError::TypeError("my_transform(list, field)".to_string())))
    }
}
```

### Template: terminal aggregation

```rust
"my_agg" => {
    if args.len() >= 2 {
        if let Value::List(items) = &args[0] {
            let field = format!("{}", args[1]);
            let vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
            // your aggregation logic
            let result = vals.iter().sum::<f64>() / vals.len() as f64;
            Some(Ok(Value::Float(result)))
        } else {
            Some(Ok(Value::Unit))
        }
    } else {
        Some(Err(RuntimeError::TypeError("my_agg(list, field)".to_string())))
    }
}
```

## Step 2: Build and test

```bash
cd compiler
cargo build --release

echo 'cell App { on run() { print(my_func(42)) } }' > /tmp/test.cell
./target/release/soma run /tmp/test.cell
```

## Step 3: Add to "did you mean?" list

In `compiler/src/interpreter/mod.rs`, search for `let builtins = vec![` and add your function name to the list. This enables typo suggestions.

## Helpers available

```rust
use super::{val_to_i64, val_to_f64, map_field_i64, map_field_f64};

val_to_i64(&value)          // extract i64 from any Value
val_to_f64(&value)          // extract f64 from any Value
map_field_i64(&map, "key")  // get field from Map as i64
map_field_f64(&map, "key")  // get field from Map as f64
```

## Value types

```rust
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
    Unit,                    // null
    // Lambda, LambdaBlock, Big — less common
}
```

## Return convention

- `Some(Ok(value))` — success
- `Some(Err(RuntimeError::TypeError("message")))` — error
- `None` — "not my function" (dispatch continues to next module)

## Example: adding `sign(x)`

In `builtins/math.rs`:

```rust
"sign" | "signum" => {
    args.first().map(|arg| match arg {
        Value::Int(n) => Ok(Value::Int(if *n > 0 { 1 } else if *n < 0 { -1 } else { 0 })),
        Value::Float(n) => Ok(Value::Float(n.signum())),
        _ => Err(RuntimeError::TypeError("sign expects a number".to_string())),
    })
}
```

That's it. Build, test, done.
