# add_column

Add a new column to an existing table.

## Schema

```toml
[[actions]]
type = "add_column"
table = "table_name"          # Required: table to add column to
up = "expression"             # Optional: SQL expression or update config

    [actions.column]          # Required: column definition
    name = "column_name"
    type = "DATA_TYPE"
    nullable = true           # Optional: default true
    default = "expression"    # Optional: SQL default
    generated = "clause"      # Optional: generation clause
```

### Complex Up Transformation

```toml
[actions.up]
table = "other_table"
value = "expression"
where = "join_condition"
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Target table |
| `column.name` | string | Yes | New column name |
| `column.type` | string | Yes | PostgreSQL data type |
| `column.nullable` | boolean | No | Allow NULL (default: true) |
| `column.default` | string | No | SQL default expression |
| `column.generated` | string | No | Generation clause |
| `up` | string/object | No | Value transformation |

## Examples

### Simple Column

```toml
[[actions]]
type = "add_column"
table = "users"

    [actions.column]
    name = "phone"
    type = "TEXT"
```

### Non-Nullable with Default

```toml
[[actions]]
type = "add_column"
table = "posts"

    [actions.column]
    name = "view_count"
    type = "INTEGER"
    nullable = false
    default = "0"
```

### With Up Transformation

Populate based on existing columns:

```toml
[[actions]]
type = "add_column"
table = "users"
up = "LOWER(email)"

    [actions.column]
    name = "normalized_email"
    type = "TEXT"
```

### Cross-Table Transformation

Populate from another table:

```toml
[[actions]]
type = "add_column"
table = "orders"

    [actions.column]
    name = "customer_name"
    type = "TEXT"

    [actions.up]
    table = "customers"
    value = "customers.name"
    where = "customers.id = orders.customer_id"
```

### Generated Column

```toml
[[actions]]
type = "add_column"
table = "products"

    [actions.column]
    name = "id"
    type = "INTEGER"
    generated = "ALWAYS AS IDENTITY"
```

## Behavior

1. **Start phase**:
   - Adds column with temporary name
   - If `up` is specified, creates triggers and backfills data
   - Adds NOT NULL constraint if column is non-nullable

2. **During migration**:
   - Old schema doesn't see the new column
   - New schema can read/write the column
   - Triggers populate values for old schema writes

3. **Complete phase**:
   - Renames column to final name
   - Validates and applies NOT NULL constraint
   - Removes triggers

## Notes

- For non-nullable columns, provide either `default` or `up` to populate existing rows
- The `up` expression is evaluated for each row during backfill
- Cross-table `up` requires a previous migration schema to exist
