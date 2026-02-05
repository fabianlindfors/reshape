# remove_column

Remove a column from a table.

## Schema

```toml
[[actions]]
type = "remove_column"
table = "table_name"          # Required: table containing the column
column = "column_name"        # Required: column to remove
down = "expression"           # Optional: SQL expression or update config
```

### Complex Down Transformation

```toml
[actions.down]
table = "other_table"
value = "expression"
where = "join_condition"
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table containing the column |
| `column` | string | Yes | Column to remove |
| `down` | string/object | No | Value transformation for old schema |

## Examples

### Simple Removal

```toml
[[actions]]
type = "remove_column"
table = "users"
column = "legacy_field"
```

### With Down Transformation

Provide values for old schema during migration:

```toml
[[actions]]
type = "remove_column"
table = "users"
column = "full_name"
down = "CONCAT(first_name, ' ', last_name)"
```

### Cross-Table Down

Populate from another table for old schema:

```toml
[[actions]]
type = "remove_column"
table = "orders"
column = "customer_name"

    [actions.down]
    table = "customers"
    value = "customers.name"
    where = "customers.id = orders.customer_id"
```

## Behavior

1. **Start phase**:
   - Column is NOT removed yet
   - If `down` is specified, creates triggers to populate the column
   - New schema's view excludes the column

2. **During migration**:
   - Old schema can still read/write the column
   - New schema doesn't see the column
   - `down` transformation populates values for old schema writes

3. **Complete phase**:
   - Drops the column
   - Removes any indices on the column
   - Removes triggers

## Notes

- The column is only removed during the complete phase
- If the column is NOT NULL and you need backward compatibility, provide a `down` expression
- For non-nullable columns with complex `down`, the NOT NULL constraint is temporarily converted to a trigger
- Data in the column is permanently lost after completion
