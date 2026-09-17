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
    comment = "description"   # Optional: comment on the index
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
| `index.comment` | string | No | Comment stored on the index |

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
   - Uses `CREATE INDEX CONCURRENTLY`
   - Builds under an action-specific temporary name in Reshape's reserved namespace
   - Retries lock timeouts up to ten attempts with exponential backoff and jitter,
     removing the action's incomplete index before each retry

2. **Complete phase**:
   - Renames the temporary index to the requested name

3. **Abort phase**:
   - Removes only the action's temporary index; never drops the requested name

## Notes

- Concurrent creation avoids blocking application reads and writes on live tables
- An existing relation with the requested index name causes a conflict and is left intact
- If cleanup fails, the temporary index remains available for a later abort. Release the
  blocker and run `reshape migration abort` again. Application queries are never
  automatically cancelled
- Connection loss can leave the result of index creation unknown. A subsequent migrate
  (if still applying) or abort inspects the temporary index in PostgreSQL's catalog.
  A valid build is reused; an invalid build is removed before retrying. No action-specific
  records are stored in `reshape.data`
- Errors retain the original PostgreSQL failure, the cleanup outcome, and available
  information about possible blockers
- Indexes started by older Reshape versions under their final names are left untouched
  on abort; inspect any leftover index manually rather than assuming ownership
- The index is usable immediately after the start phase, including uniqueness enforcement.
  Its requested name is assigned during completion, so SQL referring to that name must
  wait until completion
- For unique indexes, existing data must not have duplicates
- Expressions and `where` predicates reference columns by their current names, just like
  plain column entries. Columns which are added, altered or renamed earlier in the same
  migration can be referenced by their new names and the index follows them when the
  migration completes. Referencing a column which doesn't exist fails the migration
