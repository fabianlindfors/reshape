# add_index

Add an index to a table.

## Schema

```toml
[[actions]]
type = "add_index"
table = "table_name"          # Required: table to index

    [actions.index]           # Required: index definition
    name = "index_name"
    columns = ["col1", "col2"]
    unique = false            # Optional: default false
    type = "btree"            # Optional: index type
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table to add index to |
| `index.name` | string | Yes | Index name |
| `index.columns` | array | Yes | Columns to index |
| `index.unique` | boolean | No | Create unique index (default: false) |
| `index.type` | string | No | Index type (btree, hash, gist, etc.) |

## Examples

### Simple Index

```toml
[[actions]]
type = "add_index"
table = "users"

    [actions.index]
    name = "users_email_idx"
    columns = ["email"]
```

### Unique Index

```toml
[[actions]]
type = "add_index"
table = "users"

    [actions.index]
    name = "users_email_unique"
    columns = ["email"]
    unique = true
```

### Composite Index

```toml
[[actions]]
type = "add_index"
table = "orders"

    [actions.index]
    name = "orders_user_date_idx"
    columns = ["user_id", "created_at"]
```

### GIN Index

```toml
[[actions]]
type = "add_index"
table = "documents"

    [actions.index]
    name = "documents_tags_idx"
    columns = ["tags"]
    type = "gin"
```

## Behavior

1. **Start phase**:
   - Creates index using `CREATE INDEX CONCURRENTLY`
   - Does not block reads or writes

2. **Complete phase**:
   - No action needed

## Notes

- Indexes are created concurrently to avoid blocking
- The index is immediately available after the start phase
- For unique indexes, existing data must not have duplicates
