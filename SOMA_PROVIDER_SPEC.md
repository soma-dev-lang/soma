# Soma Storage Provider Protocol — Implementation Spec

## Context

Soma is a declarative language where developers describe storage requirements as properties on memory fields:

```
memory {
    users:    Map<String, String> [persistent, consistent]
    cache:    Map<String, String> [ephemeral, local]
    secrets:  Map<String, String> [persistent, encrypted]
    logs:     Log<String>         [persistent, immutable]
    sessions: Map<String, String> [ephemeral, ttl(30min)]
}
```

Today, all persistent storage resolves to SQLite. The goal is to let cloud providers plug in their own backends (DynamoDB, Firestore, CosmosDB, D1, etc.) so the same Soma code runs on any cloud without changes.

The developer writes properties. The provider resolves them to services. The Soma runtime calls the provider through a standard protocol.

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│  Soma Source (.cell)                                │
│                                                     │
│  memory {                                           │
│      data: Map<String,String> [persistent,consistent]│
│  }                                                  │
└──────────────────┬──────────────────────────────────┘
                   │ properties extracted at compile time
                   ▼
┌─────────────────────────────────────────────────────┐
│  Soma Compiler (check phase)                        │
│                                                     │
│  1. Parse memory declarations                       │
│  2. Extract property sets per field                 │
│  3. Validate no contradictions (persistent+ephemeral)│
│  4. Emit StorageRequest per field                   │
└──────────────────┬──────────────────────────────────┘
                   │ StorageRequest[]
                   ▼
┌─────────────────────────────────────────────────────┐
│  Provider Resolver                                  │
│                                                     │
│  1. Read soma.toml → which provider                 │
│  2. Load provider manifest                          │
│  3. Match each StorageRequest to a backend          │
│  4. Fail if any property set is unsatisfied         │
└──────────────────┬──────────────────────────────────┘
                   │ ResolvedBackend per field
                   ▼
┌─────────────────────────────────────────────────────┐
│  Soma Runtime                                       │
│                                                     │
│  Calls get/set/keys/delete on each backend          │
│  through the StorageBackend trait                   │
└──────────────────┬──────────────────────────────────┘
                   │ StorageBackend trait calls
                   ▼
┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
│  SQLite  │ │ DynamoDB │ │Firestore │ │ D1 / KV  │
│ (default)│ │  (AWS)   │ │  (GCP)   │ │  (CF)    │
└──────────┘ └──────────┘ └──────────┘ └──────────┘
```

## Part 1 — The StorageBackend Trait

This is the interface every provider backend must implement. It is deliberately minimal: 6 methods, all synchronous from the caller's perspective.

### Rust trait definition

```rust
/// The core trait that every storage backend must implement.
/// One instance is created per memory field in a cell.
pub trait StorageBackend: Send + Sync {
    /// Retrieve a value by key. Returns None if the key does not exist.
    fn get(&self, key: &str) -> Result<Option<String>, StorageError>;

    /// Store a key-value pair. Overwrites if key exists.
    /// Passing value = None deletes the key (same as delete).
    fn set(&self, key: &str, value: Option<&str>) -> Result<(), StorageError>;

    /// Return all keys. Order is not guaranteed.
    fn keys(&self) -> Result<Vec<String>, StorageError>;

    /// Delete a key. No-op if key does not exist.
    fn delete(&self, key: &str) -> Result<(), StorageError>;

    /// Return the number of stored keys.
    fn len(&self) -> Result<usize, StorageError>;

    /// Called once when the runtime shuts down. Flush buffers, close connections.
    fn close(&self) -> Result<(), StorageError>;
}

#[derive(Debug)]
pub enum StorageError {
    /// Network or I/O failure (retryable)
    ConnectionError(String),
    /// Authentication or permission failure
    AuthError(String),
    /// Key too large, value too large, quota exceeded
    LimitError(String),
    /// Any other error
    Other(String),
}
```

### How the runtime uses it

When Soma encounters `data.set("key", "value")` in a signal handler:
1. Runtime looks up which `StorageBackend` instance is bound to the `data` field
2. Calls `backend.set("key", Some("value"))`
3. If `Result::Err`, the runtime converts it to a Soma error (catchable with `try {}`)

When Soma encounters `data.get("key")`:
1. Calls `backend.get("key")`
2. `Some(value)` → returns the string to Soma
3. `None` → returns `()` (unit/null) to Soma

When Soma encounters `for key in data.keys()`:
1. Calls `backend.keys()`
2. Iterates over the returned Vec

### Important constraints

- All values are strings. Soma serializes with `to_json()` before storing and deserializes with `from_json()` after reading. The backend never interprets values.
- Keys are UTF-8 strings, max 1024 bytes.
- Values are UTF-8 strings, max 1MB.
- The backend must handle concurrent reads from multiple signal handlers (the `Send + Sync` bound).
- The backend is created once at startup and reused for the lifetime of the process.

## Part 2 — StorageRequest and Property Resolution

### StorageRequest

When the compiler processes a memory field, it emits a `StorageRequest`:

```rust
pub struct StorageRequest {
    /// Name of the cell containing this field
    pub cell_name: String,
    /// Name of the memory field
    pub field_name: String,
    /// The declared type (e.g., "Map<String, String>")
    pub field_type: String,
    /// The set of properties declared on this field
    pub properties: Vec<Property>,
}

pub enum Property {
    /// Simple flag: persistent, ephemeral, consistent, encrypted, immutable, local
    Flag(String),
    /// Parameterized: ttl(30min), retain(7years), replicas(3)
    Parameterized { name: String, value: String },
}
```

Example: for `sessions: Map<String, String> [ephemeral, ttl(30min)]`, the compiler emits:

```json
{
    "cell_name": "TradingDesk",
    "field_name": "sessions",
    "field_type": "Map<String, String>",
    "properties": [
        { "Flag": "ephemeral" },
        { "Parameterized": { "name": "ttl", "value": "30min" } }
    ]
}
```

### Standard properties

These are the properties that Soma defines. Providers must understand all of them.

| Property | Type | Meaning |
|----------|------|---------|
| `persistent` | flag | Data survives process restart |
| `ephemeral` | flag | Data lost on process restart |
| `consistent` | flag | Reads reflect the latest write (strong consistency) |
| `local` | flag | Data stays on the local machine, not replicated |
| `encrypted` | flag | Data encrypted at rest |
| `immutable` | flag | Once written, cannot be overwritten or deleted |
| `ttl(duration)` | param | Auto-delete after duration (e.g., 30min, 24h, 7d) |
| `retain(duration)` | param | Minimum retention period (e.g., 7years) |
| `replicas(n)` | param | Minimum number of copies |

### Contradiction rules (enforced by compiler)

These combinations are compile errors — the provider never sees them:

- `persistent` + `ephemeral`
- `immutable` + `ttl(…)` (can't delete immutable data)
- `retain(X)` + `ttl(Y)` where Y < X

### Resolution algorithm

The provider resolver takes a `StorageRequest` and finds the best backend:

```
for each StorageRequest:
    1. Load the provider manifest (see Part 3)
    2. Find all backends whose "requires" is a subset of the request properties
    3. Among those, find the one whose "requires" is the largest subset
       (most specific match wins)
    4. If no backend matches all properties → error:
       "provider 'aws' cannot satisfy [persistent, encrypted, ttl(30min)]
        on field 'sessions'. Supported combinations: ..."
    5. Instantiate the matched backend with the request properties
```

## Part 3 — Provider Manifest

Each provider ships a `soma-provider.toml` that declares what it supports.

### File location

```
soma-provider-aws/
├── soma-provider.toml      ← manifest
├── src/
│   ├── lib.rs              ← implements StorageBackend for each backend
│   ├── dynamodb.rs
│   ├── elasticache.rs
│   └── s3.rs
└── Cargo.toml
```

### Manifest format

```toml
[provider]
name = "aws"
version = "0.1.0"
description = "AWS storage backends for Soma"

# Authentication: how the provider gets credentials
[provider.auth]
# Environment variables the provider reads
env = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_REGION"]
# Or: config file path
# config = "~/.aws/credentials"

# Each [[backend]] block declares one storage implementation.
# The resolver picks the best match based on properties.

[[backend]]
name = "dynamodb"
description = "DynamoDB table with strong consistency"
# This backend satisfies these properties:
requires = ["persistent", "consistent"]
# It can also handle these optional properties:
optional = ["encrypted", "ttl", "replicas"]
# Rust struct that implements StorageBackend
impl = "soma_provider_aws::DynamoDbBackend"

[[backend]]
name = "dynamodb-encrypted"
description = "DynamoDB with KMS encryption"
requires = ["persistent", "consistent", "encrypted"]
optional = ["ttl", "replicas"]
impl = "soma_provider_aws::DynamoDbEncryptedBackend"

[[backend]]
name = "elasticache"
description = "ElastiCache Redis for ephemeral data"
requires = ["ephemeral"]
optional = ["ttl", "local"]
impl = "soma_provider_aws::ElastiCacheBackend"

[[backend]]
name = "s3-immutable"
description = "S3 with Object Lock for append-only data"
requires = ["persistent", "immutable"]
optional = ["retain", "encrypted"]
impl = "soma_provider_aws::S3ImmutableBackend"
```

### How resolution works with this manifest

Given `memory { data: Map<String,String> [persistent, consistent, encrypted] }`:

1. Check `dynamodb`: requires `[persistent, consistent]` — ✓ subset. `encrypted` is in optional — ✓
2. Check `dynamodb-encrypted`: requires `[persistent, consistent, encrypted]` — ✓ exact match
3. Check `elasticache`: requires `[ephemeral]` — ✗ not a subset
4. Winner: `dynamodb-encrypted` (largest matching requires set)

### Provider configuration in soma.toml

The user's project `soma.toml` specifies which provider to use:

```toml
[package]
name = "trading-desk"
version = "0.1.0"
entry = "trading_desk.cell"

[storage]
# Which provider to use. "local" = built-in SQLite (default).
provider = "aws"

# Provider-specific config passed to backend constructors
[storage.config]
region = "eu-west-1"
table_prefix = "trading_desk_"

# Override for specific fields (optional)
[storage.overrides.rank_cache]
provider = "local"  # keep cache in SQLite even on AWS
```

## Part 4 — Backend Constructor

Each backend struct receives configuration at construction time:

```rust
pub struct BackendConfig {
    /// From [storage.config] in soma.toml
    pub provider_config: HashMap<String, String>,
    /// The StorageRequest for this specific field
    pub request: StorageRequest,
    /// Resolved authentication credentials
    pub credentials: HashMap<String, String>,
}

/// Every backend must implement this constructor.
pub trait StorageBackendFactory: Send + Sync {
    /// Create a new backend instance for the given field.
    /// Called once per memory field at runtime startup.
    fn create(&self, config: BackendConfig) -> Result<Box<dyn StorageBackend>, StorageError>;
}
```

### Example: DynamoDB implementation

```rust
pub struct DynamoDbBackend {
    client: aws_sdk_dynamodb::Client,
    table_name: String,
}

impl StorageBackendFactory for DynamoDbBackendFactory {
    fn create(&self, config: BackendConfig) -> Result<Box<dyn StorageBackend>, StorageError> {
        let region = config.provider_config.get("region")
            .ok_or(StorageError::Other("missing 'region' in storage config".into()))?;

        let prefix = config.provider_config.get("table_prefix")
            .map(|s| s.as_str())
            .unwrap_or("");

        // Table name: {prefix}{cell}_{field}
        let table_name = format!("{}{}_{}",
            prefix,
            config.request.cell_name.to_lowercase(),
            config.request.field_name
        );

        // Build AWS client
        let sdk_config = aws_config::from_env()
            .region(Region::new(region.clone()))
            .load()
            .await
            .map_err(|e| StorageError::AuthError(e.to_string()))?;

        let client = aws_sdk_dynamodb::Client::new(&sdk_config);

        // Auto-create table if it doesn't exist
        ensure_table_exists(&client, &table_name).await?;

        Ok(Box::new(DynamoDbBackend { client, table_name }))
    }
}

impl StorageBackend for DynamoDbBackend {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let result = self.client.get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(key.to_string()))
            .consistent_read(true)  // ← because [consistent] property
            .send()
            .await
            .map_err(|e| StorageError::ConnectionError(e.to_string()))?;

        Ok(result.item()
            .and_then(|item| item.get("value"))
            .and_then(|v| v.as_s().ok())
            .map(|s| s.to_string()))
    }

    fn set(&self, key: &str, value: Option<&str>) -> Result<(), StorageError> {
        match value {
            Some(v) => {
                self.client.put_item()
                    .table_name(&self.table_name)
                    .item("pk", AttributeValue::S(key.to_string()))
                    .item("value", AttributeValue::S(v.to_string()))
                    .send()
                    .await
                    .map_err(|e| StorageError::ConnectionError(e.to_string()))?;
            }
            None => {
                self.delete(key)?;
            }
        }
        Ok(())
    }

    fn keys(&self) -> Result<Vec<String>, StorageError> {
        let mut keys = Vec::new();
        let mut last_key = None;

        loop {
            let mut scan = self.client.scan()
                .table_name(&self.table_name)
                .projection_expression("pk");

            if let Some(key) = last_key {
                scan = scan.exclusive_start_key("pk", key);
            }

            let result = scan.send().await
                .map_err(|e| StorageError::ConnectionError(e.to_string()))?;

            for item in result.items() {
                if let Some(pk) = item.get("pk").and_then(|v| v.as_s().ok()) {
                    keys.push(pk.to_string());
                }
            }

            last_key = result.last_evaluated_key().map(|k| k.clone());
            if last_key.is_none() { break; }
        }

        Ok(keys)
    }

    fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.client.delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(key.to_string()))
            .send()
            .await
            .map_err(|e| StorageError::ConnectionError(e.to_string()))?;
        Ok(())
    }

    fn len(&self) -> Result<usize, StorageError> {
        // DynamoDB doesn't have an efficient count — use scan count
        let result = self.client.scan()
            .table_name(&self.table_name)
            .select(Select::Count)
            .send()
            .await
            .map_err(|e| StorageError::ConnectionError(e.to_string()))?;
        Ok(result.count() as usize)
    }

    fn close(&self) -> Result<(), StorageError> {
        // No cleanup needed for DynamoDB client
        Ok(())
    }
}
```

### Example: TTL handling

When a backend receives a request with `ttl(30min)`:

```rust
fn set(&self, key: &str, value: Option<&str>) -> Result<(), StorageError> {
    let ttl_seconds = self.parse_ttl(&self.config.request.properties);
    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() + ttl_seconds;

    self.client.put_item()
        .table_name(&self.table_name)
        .item("pk", AttributeValue::S(key.to_string()))
        .item("value", AttributeValue::S(value.unwrap().to_string()))
        .item("ttl", AttributeValue::N(expires_at.to_string()))  // DynamoDB TTL
        .send().await?;
    Ok(())
}
```

## Part 5 — Provider Lifecycle

### Installation

```bash
# Add a provider to a project
soma add-provider aws --version 0.1.0

# This adds to soma.toml:
# [storage]
# provider = "aws"

# And downloads to:
# .soma_env/providers/aws/
#   soma-provider.toml
#   libsoma_provider_aws.dylib (or .so)
```

### Startup sequence

When `soma serve app.cell` or `soma run app.cell` starts:

```
1. Parse .cell file → extract all memory fields + properties
2. Read soma.toml → determine provider (default: "local")
3. Load provider manifest from .soma_env/providers/{name}/soma-provider.toml
4. For each memory field:
   a. Build StorageRequest
   b. Check [storage.overrides] for field-specific provider
   c. Resolve properties → backend using manifest
   d. Call BackendFactory::create(config) → StorageBackend instance
   e. Register backend instance in runtime's storage registry
5. Start serving (all backends are ready)
```

### Runtime storage registry

```rust
pub struct StorageRegistry {
    /// Maps "CellName.field_name" → backend instance
    backends: HashMap<String, Box<dyn StorageBackend>>,
}

impl StorageRegistry {
    pub fn get_backend(&self, cell: &str, field: &str) -> &dyn StorageBackend {
        let key = format!("{}.{}", cell, field);
        self.backends.get(&key).expect("backend not registered")
    }
}
```

When the interpreter executes `stocks.get("MC")` inside cell `TradingDesk`:
1. Runtime calls `registry.get_backend("TradingDesk", "stocks")`
2. Calls `.get("MC")` on the returned backend
3. Returns result to the Soma program

## Part 6 — The Built-in "local" Provider

The default provider ships with Soma and requires no configuration:

```toml
# Built-in, not a separate package

[provider]
name = "local"

[[backend]]
name = "sqlite"
requires = ["persistent"]
optional = ["consistent", "encrypted", "retain"]

[[backend]]
name = "memory"
requires = ["ephemeral"]
optional = ["local", "ttl"]
```

This is what runs today. It stays the default so `soma serve app.cell` works with zero config.

## Part 7 — Testing a Provider

A provider must pass the Soma conformance test suite. Soma ships a test harness:

```bash
# Run conformance tests against a provider
soma test-provider aws

# This runs:
# 1. Basic CRUD: set, get, delete, keys, len
# 2. Overwrite: set same key twice, get returns latest
# 3. Missing key: get returns None
# 4. Delete idempotent: delete non-existent key is OK
# 5. Keys consistency: keys() reflects all set/delete ops
# 6. TTL (if supported): set with ttl, wait, verify expired
# 7. Encryption (if supported): verify value is not plaintext on disk
# 8. Immutable (if supported): set then set again → error
# 9. Concurrent access: 10 threads, 100 ops each, no corruption
# 10. Large values: 1MB value round-trips correctly
```

### Conformance test cell

```
cell test ProviderConformance {
    memory {
        basic: Map<String, String> [persistent, consistent]
    }

    rules {
        // CRUD
        assert basic.get("missing") == ()
        assert basic.set("k1", "v1") == ()
        assert basic.get("k1") == "v1"
        assert basic.set("k1", "v2") == ()
        assert basic.get("k1") == "v2"
        assert basic.delete("k1") == ()
        assert basic.get("k1") == ()

        // Keys
        assert basic.set("a", "1") == ()
        assert basic.set("b", "2") == ()
        assert basic.keys().length == 2
    }
}
```

## Part 8 — Provider Examples for Major Clouds

### AWS

| Properties | Backend | AWS Service |
|---|---|---|
| `persistent, consistent` | dynamodb | DynamoDB (ConsistentRead=true) |
| `persistent, consistent, encrypted` | dynamodb-encrypted | DynamoDB + KMS |
| `persistent, immutable` | s3-immutable | S3 + Object Lock |
| `ephemeral` | elasticache | ElastiCache Redis |
| `ephemeral, ttl(…)` | elasticache-ttl | ElastiCache + EXPIRE |

### GCP

| Properties | Backend | GCP Service |
|---|---|---|
| `persistent, consistent` | firestore | Firestore (native mode) |
| `persistent, consistent, encrypted` | firestore-cmek | Firestore + CMEK |
| `persistent, immutable` | gcs-locked | GCS + Retention Policy |
| `ephemeral` | memorystore | Memorystore Redis |
| `ephemeral, ttl(…)` | memorystore-ttl | Memorystore + EXPIRE |

### Cloudflare

| Properties | Backend | CF Service |
|---|---|---|
| `persistent, consistent` | d1 | D1 (SQLite at edge) |
| `ephemeral` | kv | Workers KV |
| `ephemeral, ttl(…)` | kv-ttl | Workers KV + expirationTtl |
| `persistent, immutable` | r2-locked | R2 + Object Lock |

### Supabase

| Properties | Backend | Service |
|---|---|---|
| `persistent, consistent` | postgres | Supabase Postgres |
| `ephemeral, ttl(…)` | redis | Supabase Redis (upcoming) |

## Part 9 — Migration Between Providers

When a user switches from `provider = "local"` to `provider = "aws"`:

```bash
soma migrate --from local --to aws

# This:
# 1. Connects to source provider (local SQLite)
# 2. Connects to target provider (AWS DynamoDB)
# 3. For each memory field:
#    a. Calls source.keys()
#    b. For each key: source.get(key) → target.set(key, value)
#    c. Verifies: target.len() == source.len()
# 4. Updates soma.toml: provider = "aws"
```

The migration tool only uses the StorageBackend trait — it works between any two providers without special code.

## Part 10 — Summary of What to Build

### Phase 1: Core protocol (in the Soma compiler/runtime)

1. **Define the `StorageBackend` trait** in a new crate: `soma-storage`
2. **Define `StorageRequest`, `Property`, `BackendConfig`, `StorageError`**
3. **Implement the provider resolver** (manifest parser + matching algorithm)
4. **Refactor the current SQLite backend** to implement `StorageBackend`
5. **Refactor the current in-memory backend** to implement `StorageBackend`
6. **Add `StorageRegistry`** to the runtime, replace direct SQLite calls
7. **Add `[storage]` section parsing** to soma.toml
8. **Ship the conformance test suite**

### Phase 2: Provider SDK

9. **Publish `soma-storage` crate** with trait + types + test harness
10. **Write `soma-provider-example`** — minimal reference implementation
11. **Document the manifest format**
12. **Add `soma add-provider` and `soma test-provider` CLI commands**

### Phase 3: First cloud providers

13. **`soma-provider-aws`** — DynamoDB + ElastiCache + S3
14. **`soma-provider-gcp`** — Firestore + Memorystore + GCS
15. **`soma-provider-cloudflare`** — D1 + KV + R2

### Phase 4: Migration

16. **`soma migrate`** CLI command
17. **Dry-run mode** that reports what would be migrated without writing
