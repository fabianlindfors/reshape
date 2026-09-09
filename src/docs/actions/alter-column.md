# alter_column

Modify an existing column's type, name, nullability, or default value.

## Schema

```toml
[[actions]]
type = "alter_column"
table = "table_name"          # Required: table containing the column
column = "column_name"        # Required: column to modify
up = "expression"             # Optional: SQL expression for new value
down = "expression"           # Optional: SQL expression for old value

    [actions.changes]         # At least one change required
    name = "new_name"         # Optional: rename column
    type = "NEW_TYPE"         # Optional: change data type
    nullable = true           # Optional: change nullability
    default = "expression"    # Optional: change default value
    comment = "description"   # Optional: change comment, empty removes it
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table containing the column |
| `column` | string | Yes | Current column name |
| `up` | string | No | SQL expression to compute new value from old |
| `down` | string | No | SQL expression to compute old value from new |
| `changes.name` | string | No | New column name |
| `changes.type` | string | No | New data type |
| `changes.nullable` | boolean | No | New nullability |
| `changes.default` | string | No | New default value expression |
| `changes.comment` | string | No | New comment on the column, an empty string removes it |

## Examples

### Rename Column

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "name"

    [actions.changes]
    name = "full_name"
```

### Change Type with Transformation

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "age"
up = "age::TEXT"
down = "age::INTEGER"

    [actions.changes]
    type = "TEXT"
```

### Make Column Non-Nullable

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "email"
up = "COALESCE(email, 'unknown@example.com')"
down = "email"

    [actions.changes]
    nullable = false
```

### Change Default Value

```toml
[[actions]]
type = "alter_column"
table = "posts"
column = "status"

    [actions.changes]
    default = "'draft'"
```

### Multiple Changes

```toml
[[actions]]
type = "alter_column"
table = "products"
column = "price"
up = "price * 100"
down = "price / 100"

    [actions.changes]
    name = "price_cents"
    type = "INTEGER"
```

## Behavior

1. **Start phase**:
   - Creates a temporary column with new properties
   - Sets up triggers to sync values between old and new columns
   - Backfills existing data using the `up` expression

2. **During migration**:
   - Old schema reads/writes the original column
   - New schema reads/writes the temporary column
   - Triggers keep both columns synchronized

3. **Complete phase**:
   - Drops the original column
   - Renames temporary column to final name
   - Removes triggers

## Notes

- If only renaming or changing the comment (no type/nullable/default changes), the column is changed directly without a temporary column
- `up` and `down` reference the altered column by its existing name from `column`, even when `changes.name` renames it
- The `up` expression can reference any column in the table
- The `down` expression enables backward compatibility during rollout
- Indices on the column are automatically duplicated to the new column
- Comments on the column, its indices and its check constraints are carried over to the new column, unless `changes.comment` declares a new one
- A new comment is visible through this migration's schema right away and only replaces the column's own comment once the migration completes, so aborting leaves the original comment
