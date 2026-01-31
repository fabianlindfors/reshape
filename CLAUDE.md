# Reshape

Reshape is a zero-downtime schema migration tool for PostgreSQL (12+). It works by creating views that encapsulate tables, with triggers to translate between old and new schemas during migrations. This allows gradual application rollouts without downtime.

## Build and Test Commands

```bash
# Build
cargo build

# Run all tests (must use single thread due to shared database)
cargo test -- --test-threads=1

# Run a single test
cargo test test_name -- --test-threads=1

# Lint
cargo clippy
```

**Test database setup:** Tests require a Postgres database.

## Architecture

### Core Components

- **`src/lib.rs`**: Main `Reshape` struct with public API (`migrate`, `complete`, `abort`, `remove`)
- **`src/db.rs`**: Database connection with `DbLocker` for advisory locking to prevent concurrent runs
- **`src/state.rs`**: State machine tracking migration lifecycle (`Idle` → `Applying` → `InProgress` → `Completing`)
- **`src/schema.rs`**: Schema introspection and change tracking during migrations
- **`src/main.rs`**: CLI interface using clap

### Migration Actions (`src/migrations/`)

All migration types implement the `Action` trait:

- `run()`: Apply the migration (create views, triggers, temporary columns)
- `complete()`: Finalize the migration (drop old columns, rename, cleanup)
- `abort()`: Roll back changes
- `update_schema()`: Track schema changes for subsequent actions

Supported actions: `create_table`, `alter_column`, `add_column`, `remove_column`, `rename_table`, `remove_table`, `add_index`, `remove_index`, `create_enum`, `remove_enum`, `add_foreign_key`, `remove_foreign_key`, `custom`

### Key Patterns

- **State persistence**: Migration state stored in `reshape.data` table as JSON; completed migrations tracked in `reshape.migrations`
- **Naming conventions**: Objects created during migration use prefix `__reshape_{migration_index}_{action_index}`
- **Schema naming**: Views created in schema named `migration_{migration_name}`
- **Retry logic**: Database queries use exponential backoff (100ms to 3.2s, up to 10 attempts)
- **Trait serialization**: Migration actions use `typetag` for trait object serialization in TOML/JSON files

### Migration File Format

Migrations are TOML or JSON files in `migrations/` directory with `[[actions]]` arrays:

```toml
[[actions]]
type = "create_table"
name = "users"
primary_key = ["id"]

    [[actions.columns]]
    name = "id"
    type = "INTEGER"
```
