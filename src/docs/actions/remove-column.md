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
   - With a cross-table `down`, writes to `down.table` in the new schema populate the
     column for the matching rows

3. **Complete phase**:
   - Drops the column
   - Removes any indices on the column
   - Removes triggers

## Notes

- The column is only removed during the complete phase
- If the column is NOT NULL and you need backward compatibility, provide a `down` expression
- For non-nullable columns with a cross-table `down`, the NOT NULL constraint is temporarily
  replaced by triggers. Writes in the old schema are still rejected immediately. In the new
  schema, the column may be left empty within a transaction, so that a row can be inserted
  before the row in `down.table` it takes its value from, but the transaction fails at
  commit if it is still empty. This requires the table to have a primary key
- In a cross-table `down`, every column in `value` and `where` must be qualified with its
  table name, as the expressions run in triggers on both tables
- Data in the column is permanently lost after completion
