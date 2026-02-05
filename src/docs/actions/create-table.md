# create_table

Create a new table in the database.

## Schema

```toml
[[actions]]
type = "create_table"
name = "table_name"           # Required: name of the table
primary_key = ["id"]          # Required: primary key column(s)

    [[actions.columns]]       # Required: at least one column
    name = "column_name"
    type = "DATA_TYPE"
    nullable = true           # Optional: default true
    default = "expression"    # Optional: SQL expression
    generated = "clause"      # Optional: generation clause

    [[actions.foreign_keys]]  # Optional: foreign key constraints
    columns = ["col"]
    referenced_table = "other"
    referenced_columns = ["id"]

    [actions.up]              # Optional: populate from existing table
    table = "source_table"
    values = { col1 = "expr1", col2 = "expr2" }
    upsert_constraint = "constraint_name"  # Optional
```

## Column Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Column name |
| `type` | string | Yes | PostgreSQL data type |
| `nullable` | boolean | No | Allow NULL values (default: true) |
| `default` | string | No | SQL expression for default value |
| `generated` | string | No | Generation clause (e.g., "ALWAYS AS IDENTITY") |

## Examples

### Basic Table

```toml
[[actions]]
type = "create_table"
name = "users"
primary_key = ["id"]

    [[actions.columns]]
    name = "id"
    type = "INTEGER"
    generated = "ALWAYS AS IDENTITY"

    [[actions.columns]]
    name = "email"
    type = "TEXT"
    nullable = false

    [[actions.columns]]
    name = "created_at"
    type = "TIMESTAMPTZ"
    default = "NOW()"
```

### Table with Foreign Key

```toml
[[actions]]
type = "create_table"
name = "posts"
primary_key = ["id"]

    [[actions.columns]]
    name = "id"
    type = "INTEGER"
    generated = "ALWAYS AS IDENTITY"

    [[actions.columns]]
    name = "user_id"
    type = "INTEGER"
    nullable = false

    [[actions.columns]]
    name = "title"
    type = "TEXT"

    [[actions.foreign_keys]]
    columns = ["user_id"]
    referenced_table = "users"
    referenced_columns = ["id"]
```

### Table with Data Migration

Populate new table from existing data:

```toml
[[actions]]
type = "create_table"
name = "user_profiles"
primary_key = ["user_id"]

    [[actions.columns]]
    name = "user_id"
    type = "INTEGER"

    [[actions.columns]]
    name = "display_name"
    type = "TEXT"

    [actions.up]
    table = "users"
    values = { user_id = "id", display_name = "COALESCE(name, email)" }
```

## Behavior

1. **Start phase**: Creates the table with all columns and constraints
2. **If `up` is specified**: Sets up triggers to sync data from source table
3. **Complete phase**: Removes sync triggers

## Notes

- The table is created immediately during the start phase
- Foreign keys reference the current state of referenced tables
- Use `up` transformation to populate from existing tables during migration
