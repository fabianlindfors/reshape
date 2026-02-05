# custom

Run custom SQL statements at different migration phases.

## Schema

```toml
[[actions]]
type = "custom"
start = "SQL statement"       # Optional: run during start phase
complete = "SQL statement"    # Optional: run during complete phase
abort = "SQL statement"       # Optional: run during abort
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `start` | string | No | SQL to run during migration start |
| `complete` | string | No | SQL to run during completion |
| `abort` | string | No | SQL to run if migration is aborted |

## Examples

### Run During Start

```toml
[[actions]]
type = "custom"
start = "CREATE EXTENSION IF NOT EXISTS pg_trgm"
```

### Run During Complete

```toml
[[actions]]
type = "custom"
complete = "VACUUM ANALYZE users"
```

### With Abort Handling

```toml
[[actions]]
type = "custom"
start = "CREATE EXTENSION my_extension"
abort = "DROP EXTENSION IF EXISTS my_extension"
```

### Multiple Statements

```toml
[[actions]]
type = "custom"
start = """
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS btree_gin;
"""
```

### Data Migration

```toml
[[actions]]
type = "custom"
complete = """
UPDATE users
SET normalized_email = LOWER(email)
WHERE normalized_email IS NULL
"""
```

## Behavior

1. **Start phase**: Executes `start` SQL if provided
2. **Complete phase**: Executes `complete` SQL if provided
3. **Abort**: Executes `abort` SQL if provided

## Notes

- SQL statements are executed directly without wrapping
- Multiple statements can be separated by semicolons
- Use for operations not covered by built-in actions:
  - Installing extensions
  - Creating functions/procedures
  - Complex data migrations
  - Running VACUUM or ANALYZE
- The `abort` statement should undo what `start` did
- No automatic rollback - ensure `abort` handles cleanup
