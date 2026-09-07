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
    where = "col1 IS NOT NULL"  # Optional: predicate for a partial index
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table to add index to |
| `index.name` | string | Yes | Index name |
| `index.columns` | array | Yes | Columns to index |
| `index.unique` | boolean | No | Create unique index (default: false) |
| `index.type` | string | No | Index type (btree, hash, gist, etc.) |
| `index.where` | string | No | Predicate for a partial index, without the `WHERE` keyword |

## Index Columns

Each entry in `index.columns` is either a plain column name or a table with more detail.
The order of the entries is the order of the columns in the index.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `column` | string | Yes\* | Column to index |
| `expression` | string | Yes\* | Expression to index, instead of a column |
| `direction` | string | No | `ASC` (default) or `DESC` |
| `nulls` | string | No | `FIRST` or `LAST`. Defaults to `LAST` for `ASC` and `FIRST` for `DESC` |

\* Exactly one of `column` and `expression` must be set.

```toml
columns = [
    "audience",
    { column = "content_updated_at", direction = "DESC", nulls = "LAST" },
    { expression = "lower(email)" },
]
```

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

### Index with Sort Order

Useful for keyset pagination, where the index has to match the query's `ORDER BY`:

```toml
[[actions]]
type = "add_index"
table = "posts"

    [actions.index]
    name = "posts_keyset_idx"
    columns = [
        "audience",
        { column = "content_updated_at", direction = "DESC" },
        "id",
    ]
```

### Partial Index

```toml
[[actions]]
type = "add_index"
table = "posts"

    [actions.index]
    name = "posts_community_idx"
    columns = ["audience", "id"]
    where = "is_public AND shared_to_community"
```

### Expression Index

```toml
[[actions]]
type = "add_index"
table = "users"

    [actions.index]
    name = "users_email_idx"
    unique = true
    columns = [{ expression = "lower(email)" }]
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
- Expressions and `where` predicates are passed to Postgres as written rather than being
  resolved by Reshape, so they reference the table's real columns. Avoid referencing a
  column which is being altered or renamed in the same migration: `alter_column` replaces
  the column rather than modifying it in place, so creating the index can fail, and an
  index that is created may be dropped again when the migration is completed
