# Task: Implement HTTP Sidecar Protocol for Soma Storage Providers

## Goal

Enable Soma to use external storage providers (AWS DynamoDB, Cloudflare KV, etc.) by adding an HTTP sidecar protocol to the runtime. Today, the runtime only supports 3 hardcoded backends (memory, file, sqlite). After this work, any storage provider can be implemented as a standalone HTTP server that Soma calls at runtime.

## Context

Read these files first to understand the current system:

- `compiler/src/runtime/storage.rs` — the `StorageBackend` trait (10 methods) and existing implementations
- `compiler/src/provider/resolver.rs` — resolves memory properties to backends via manifest matching
- `compiler/src/provider/manifest.rs` — parses `soma-provider.toml` files
- `compiler/src/provider/types.rs` — `StorageRequest`, `Property`, `BackendConfig`, `StorageError`
- `stdlib/providers/local/soma-provider.toml` — example manifest for the built-in local provider

The provider resolution pipeline already works:
1. Compiler extracts `[persistent, consistent]` from memory declarations
2. Resolver loads provider manifest and matches properties to a backend
3. **Problem:** `instantiate_native_backend()` in `storage.rs` only knows `memory`, `file`, `sqlite`. Unknown backends silently fall back to memory.

## What to build

### Part 1 — HttpBackend in the Soma runtime

Add a new `StorageBackend` implementation in `compiler/src/runtime/storage.rs` that proxies all calls to an HTTP server.

```rust
pub struct HttpBackend {
    base_url: String,       // e.g. "http://localhost:9100"
    cell_name: String,
    field_name: String,
    client: reqwest::blocking::Client,
}
```

It implements `StorageBackend` by making HTTP calls:

| Trait method | HTTP call | Request body | Response body |
|---|---|---|---|
| `get(key)` | `POST /get` | `{"cell":"X","field":"Y","key":"k"}` | `{"value": <StoredValue or null>}` |
| `set(key, value)` | `POST /set` | `{"cell":"X","field":"Y","key":"k","value":<StoredValue>}` | `{"ok": true}` |
| `delete(key)` | `POST /delete` | `{"cell":"X","field":"Y","key":"k"}` | `{"deleted": true/false}` |
| `keys()` | `POST /keys` | `{"cell":"X","field":"Y"}` | `{"keys": ["k1","k2",...]}` |
| `values()` | `POST /values` | `{"cell":"X","field":"Y"}` | `{"values": [<StoredValue>, ...]}` |
| `has(key)` | `POST /has` | `{"cell":"X","field":"Y","key":"k"}` | `{"exists": true/false}` |
| `len()` | `POST /len` | `{"cell":"X","field":"Y"}` | `{"len": 42}` |
| `append(value)` | `POST /append` | `{"cell":"X","field":"Y","value":<StoredValue>}` | `{"ok": true}` |
| `list()` | `POST /list` | `{"cell":"X","field":"Y"}` | `{"items": [<StoredValue>, ...]}` |

StoredValue JSON encoding:
- `Int(42)` → `{"type":"int","value":42}`
- `Float(3.14)` → `{"type":"float","value":3.14}`
- `String("hello")` → `{"type":"string","value":"hello"}`
- `Bool(true)` → `{"type":"bool","value":true}`
- `Null` → `{"type":"null"}`
- `List([...])` → `{"type":"list","value":[...]}`
- `Map({...})` → `{"type":"map","value":{...}}`

Error handling: if the HTTP call fails or returns a non-200 status, log a warning and return the appropriate empty/default value (None for get, empty vec for keys, etc.). Do not panic.

### Part 2 — Wire HttpBackend into the resolver

In `compiler/src/provider/resolver.rs`, modify the `instantiate` function:

```rust
fn instantiate(native_name: &str, cell_name: &str, field_name: &str) -> Arc<dyn StorageBackend> {
    match native_name {
        "sqlite" => Arc::new(SqliteBackend::new(cell_name, field_name)),
        "memory" => Arc::new(MemoryBackend::new()),
        "file" => Arc::new(FileBackend::new(cell_name, field_name)),
        _ => {
            // Try HTTP sidecar: check if a sidecar is running
            // The URL comes from provider config or defaults to http://localhost:9100
            Arc::new(HttpBackend::new(
                "http://localhost:9100",  // TODO: read from config
                cell_name,
                field_name,
            ))
        }
    }
}
```

Also modify `instantiate_native_backend()` in `storage.rs` with the same logic — both functions need updating because the codebase has two code paths (resolver-based and fallback).

### Part 3 — Read sidecar URL from soma.toml

In the `[storage]` section of soma.toml, add support for a `url` field:

```toml
[storage]
provider = "aws"

[storage.config]
url = "http://localhost:9100"
region = "eu-west-1"
table_prefix = "myapp_"
```

Pass `storage.config` through to `BackendConfig.provider_config` so the HttpBackend can read the URL from config instead of hardcoding localhost:9100.

### Part 4 — Build a reference sidecar in Python

Create `tools/sidecar/` with a minimal Python HTTP server that implements the sidecar protocol using an in-memory dict. This is for testing the protocol, not for production use.

File: `tools/sidecar/server.py`

```python
from http.server import HTTPServer, BaseHTTPRequestHandler
import json

# Storage: dict of "cell.field" -> dict of key -> value
storage = {}
logs = {}

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        cell = body.get('cell', '')
        field = body.get('field', '')
        ns = f"{cell}.{field}"

        if ns not in storage:
            storage[ns] = {}
        if ns not in logs:
            logs[ns] = []

        path = self.path

        if path == '/get':
            key = body['key']
            value = storage[ns].get(key)
            self.respond({'value': value})

        elif path == '/set':
            key = body['key']
            storage[ns][key] = body['value']
            self.respond({'ok': True})

        elif path == '/delete':
            key = body['key']
            deleted = key in storage[ns]
            storage[ns].pop(key, None)
            self.respond({'deleted': deleted})

        elif path == '/keys':
            keys = [k for k in storage[ns].keys() if not k.startswith('__')]
            self.respond({'keys': keys})

        elif path == '/values':
            self.respond({'values': list(storage[ns].values())})

        elif path == '/has':
            self.respond({'exists': body['key'] in storage[ns]})

        elif path == '/len':
            self.respond({'len': len(storage[ns]) + len(logs[ns])})

        elif path == '/append':
            logs[ns].append(body['value'])
            self.respond({'ok': True})

        elif path == '/list':
            self.respond({'items': logs[ns]})

        elif path == '/health':
            self.respond({'status': 'ok', 'provider': 'memory-sidecar'})

        else:
            self.respond({'error': f'unknown endpoint: {path}'}, status=404)

    def respond(self, data, status=200):
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, format, *args):
        print(f"[sidecar] {args[0]}")

if __name__ == '__main__':
    port = 9100
    print(f"soma storage sidecar listening on http://localhost:{port}")
    HTTPServer(('0.0.0.0', port), Handler).serve_forever()
```

### Part 5 — Build an AWS DynamoDB sidecar

Create `providers/aws/` with a sidecar that implements the same HTTP protocol but uses DynamoDB as the backend.

File: `providers/aws/server.py`

```python
"""
Soma storage provider for AWS DynamoDB.

Setup:
  pip install boto3
  export AWS_ACCESS_KEY_ID=...
  export AWS_SECRET_ACCESS_KEY=...
  export AWS_DEFAULT_REGION=eu-west-1

Run:
  python server.py
  # or: python server.py --port 9100 --prefix myapp_

The sidecar creates one DynamoDB table per cell.field combination.
Table naming: {prefix}{cell}_{field}
Each table has: pk (String, partition key), value (String, JSON-encoded StoredValue)
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import sys
import boto3
from botocore.exceptions import ClientError

# Config
PORT = 9100
PREFIX = ""
for i, arg in enumerate(sys.argv):
    if arg == '--port' and i + 1 < len(sys.argv):
        PORT = int(sys.argv[i + 1])
    if arg == '--prefix' and i + 1 < len(sys.argv):
        PREFIX = sys.argv[i + 1]

dynamodb = boto3.resource('dynamodb')

def table_name(cell, field):
    return f"{PREFIX}{cell}_{field}"

def ensure_table(name):
    """Create table if it doesn't exist."""
    try:
        table = dynamodb.Table(name)
        table.load()
        return table
    except ClientError as e:
        if e.response['Error']['Code'] == 'ResourceNotFoundException':
            table = dynamodb.create_table(
                TableName=name,
                KeySchema=[{'AttributeName': 'pk', 'KeyType': 'HASH'}],
                AttributeDefinitions=[{'AttributeName': 'pk', 'AttributeType': 'S'}],
                BillingMode='PAY_PER_REQUEST'
            )
            table.wait_until_exists()
            print(f"[aws] created table: {name}")
            return table
        raise

# Cache table references
tables = {}

def get_table(cell, field):
    name = table_name(cell, field)
    if name not in tables:
        tables[name] = ensure_table(name)
    return tables[name]

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        cell = body.get('cell', '')
        field = body.get('field', '')
        path = self.path

        try:
            if path == '/health':
                self.respond({'status': 'ok', 'provider': 'aws-dynamodb'})
                return

            table = get_table(cell, field)

            if path == '/get':
                resp = table.get_item(Key={'pk': body['key']}, ConsistentRead=True)
                item = resp.get('Item')
                if item and 'value' in item:
                    self.respond({'value': json.loads(item['value'])})
                else:
                    self.respond({'value': None})

            elif path == '/set':
                table.put_item(Item={
                    'pk': body['key'],
                    'value': json.dumps(body['value'])
                })
                self.respond({'ok': True})

            elif path == '/delete':
                try:
                    table.delete_item(
                        Key={'pk': body['key']},
                        ConditionExpression='attribute_exists(pk)'
                    )
                    self.respond({'deleted': True})
                except ClientError as e:
                    if e.response['Error']['Code'] == 'ConditionalCheckFailedException':
                        self.respond({'deleted': False})
                    else:
                        raise

            elif path == '/keys':
                items = []
                resp = table.scan(ProjectionExpression='pk')
                items.extend([i['pk'] for i in resp['Items'] if not i['pk'].startswith('__')])
                while 'LastEvaluatedKey' in resp:
                    resp = table.scan(
                        ProjectionExpression='pk',
                        ExclusiveStartKey=resp['LastEvaluatedKey']
                    )
                    items.extend([i['pk'] for i in resp['Items'] if not i['pk'].startswith('__')])
                self.respond({'keys': items})

            elif path == '/values':
                items = []
                resp = table.scan()
                items.extend([json.loads(i['value']) for i in resp['Items'] if 'value' in i])
                while 'LastEvaluatedKey' in resp:
                    resp = table.scan(ExclusiveStartKey=resp['LastEvaluatedKey'])
                    items.extend([json.loads(i['value']) for i in resp['Items'] if 'value' in i])
                self.respond({'values': items})

            elif path == '/has':
                resp = table.get_item(Key={'pk': body['key']}, ProjectionExpression='pk')
                self.respond({'exists': 'Item' in resp})

            elif path == '/len':
                count = 0
                resp = table.scan(Select='COUNT')
                count += resp['Count']
                while 'LastEvaluatedKey' in resp:
                    resp = table.scan(Select='COUNT', ExclusiveStartKey=resp['LastEvaluatedKey'])
                    count += resp['Count']
                self.respond({'len': count})

            elif path == '/append':
                # For append-only logs, use auto-incrementing key
                import time
                key = f"__log_{int(time.time() * 1000000)}"
                table.put_item(Item={
                    'pk': key,
                    'value': json.dumps(body['value']),
                    '__is_log': True
                })
                self.respond({'ok': True})

            elif path == '/list':
                items = []
                resp = table.scan(FilterExpression='attribute_exists(#log)',
                                  ExpressionAttributeNames={'#log': '__is_log'})
                items.extend([json.loads(i['value']) for i in resp['Items'] if 'value' in i])
                self.respond({'items': items})

            else:
                self.respond({'error': f'unknown endpoint: {path}'}, status=404)

        except Exception as e:
            print(f"[aws] error: {e}")
            self.respond({'error': str(e)}, status=500)

    def respond(self, data, status=200):
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, format, *args):
        print(f"[aws] {args[0]}")

if __name__ == '__main__':
    print(f"soma AWS provider (DynamoDB) listening on http://localhost:{PORT}")
    print(f"table prefix: '{PREFIX}'")
    HTTPServer(('0.0.0.0', PORT), Handler).serve_forever()
```

Provider manifest file: `providers/aws/soma-provider.toml`

```toml
[provider]
name = "aws"
version = "0.1.0"
description = "AWS DynamoDB storage provider for Soma"

[provider.auth]
env = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_DEFAULT_REGION"]

[[backend]]
name = "dynamodb"
description = "DynamoDB with strong consistency"
requires = ["persistent", "consistent"]
optional = ["encrypted", "ttl"]
native = "http"

[[backend]]
name = "dynamodb-default"
description = "DynamoDB for any persistent storage"
requires = ["persistent"]
optional = ["consistent", "encrypted", "ttl"]
native = "http"
```

Note: `native = "http"` tells the resolver to use `HttpBackend` instead of a local implementation.

### Part 6 — Add `soma add-provider` implementation

The `soma add-provider aws` command should:

1. Check if `providers/aws/soma-provider.toml` exists in the repo (or a registry URL)
2. Copy the manifest to `.soma_env/providers/aws/soma-provider.toml`
3. Copy the sidecar server to `.soma_env/providers/aws/server.py`
4. Update `soma.toml` with:
   ```toml
   [storage]
   provider = "aws"

   [storage.config]
   url = "http://localhost:9100"
   ```
5. Print instructions:
   ```
   ✓ provider 'aws' installed

   setup:
     pip install boto3
     export AWS_ACCESS_KEY_ID=...
     export AWS_SECRET_ACCESS_KEY=...
     export AWS_DEFAULT_REGION=eu-west-1

   start the provider sidecar:
     python .soma_env/providers/aws/server.py

   then run your app:
     soma serve app.cell
   ```

### Part 7 — Add `soma provider start` command

Convenience command that starts the sidecar for the configured provider:

```bash
soma provider start          # reads soma.toml, starts the right sidecar
soma provider start --port 9100
soma provider status         # health check: GET /health
soma provider stop           # kills the sidecar
```

This runs `python .soma_env/providers/{name}/server.py` as a background process, waits for `/health` to return 200, then prints ready.

## Testing plan

### Test 1 — Memory sidecar (no cloud needed)

```bash
# Terminal 1: start the reference sidecar
python tools/sidecar/server.py

# Terminal 2: run the todo app with sidecar
# (soma.toml has provider = "sidecar", url = "http://localhost:9100")
soma serve todo.cell -p 8080

# Terminal 3: test
curl -s -X POST http://localhost:8080/add -H "Content-Type: application/json" -d '{"title":"Test sidecar"}'
curl -s http://localhost:8080/list
```

Expected: same behavior as with SQLite, but storage goes through HTTP to the sidecar.

### Test 2 — AWS DynamoDB (requires AWS credentials)

```bash
# Terminal 1: start AWS sidecar
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_DEFAULT_REGION=eu-west-1
python providers/aws/server.py --prefix test_

# Terminal 2: run the trading desk
soma serve trading_desk.cell -p 8080

# Terminal 3: test full pipeline
curl -s http://localhost:8080/api/universe | python3 -c "import sys,json; print(len(json.load(sys.stdin)), 'stocks')"
```

Expected: 20 stocks, stored in DynamoDB tables `test_TradingDesk_stocks`, `test_TradingDesk_trades`, etc.

### Test 3 — Provider switching (no code changes)

```bash
# Run with local SQLite
soma serve trading_desk.cell -p 8080

# Stop. Change soma.toml to provider = "aws". Start sidecar. Run again.
soma serve trading_desk.cell -p 8080

# Same app, same code, different backend.
```

### Test 4 — Conformance test

Run the same test suite against both providers:

```bash
# Against local
soma test conformance.cell

# Against sidecar
python tools/sidecar/server.py &
soma test conformance.cell  # with provider = "sidecar"
```

Both must pass identically.

## File summary

| File | Action | Description |
|---|---|---|
| `compiler/src/runtime/storage.rs` | Modify | Add `HttpBackend` struct implementing `StorageBackend` via HTTP |
| `compiler/src/provider/resolver.rs` | Modify | Route `native = "http"` to `HttpBackend`, read URL from config |
| `compiler/src/provider/types.rs` | No change | Already has everything needed |
| `compiler/src/provider/manifest.rs` | No change | Already parses manifests correctly |
| `tools/sidecar/server.py` | Create | Reference sidecar for testing (in-memory dict) |
| `providers/aws/server.py` | Create | AWS DynamoDB sidecar |
| `providers/aws/soma-provider.toml` | Create | AWS provider manifest |
| `compiler/src/main.rs` | Modify | Implement `soma provider start/stop/status` subcommands |
| `compiler/Cargo.toml` | Modify | Add `reqwest = { version = "0.11", features = ["blocking", "json"] }` |

## Constraints

- Do not break existing behavior. When `provider = "local"` (the default), everything must work exactly as before.
- The HttpBackend must be resilient: if the sidecar is down, log a clear error ("storage provider at http://localhost:9100 is not reachable — is the sidecar running?") rather than panicking.
- All HTTP calls are synchronous (blocking). The Soma runtime is single-threaded.
- The sidecar protocol is the contract. Any language can implement a provider: Python, Node, Rust, Go. The protocol is HTTP + JSON.
- StoredValue encoding must round-trip perfectly: `Int(42)` → JSON → `Int(42)`, never `Float(42.0)` or `String("42")`.
