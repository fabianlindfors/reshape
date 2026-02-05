# rename_table

Rename an existing table.

## Schema

```toml
[[actions]]
type = "rename_table"
table = "old_name"            # Required: current table name
new_name = "new_name"         # Required: new table name
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Current table name |
| `new_name` | string | Yes | New table name |

## Example

```toml
[[actions]]
type = "rename_table"
table = "users"
new_name = "customers"
```

## Behavior

1. **Start phase**:
   - Table is NOT renamed yet
   - Old schema's view uses original name
   - New schema's view uses new name (pointing to original table)

2. **During migration**:
   - Old schema uses original table name
   - New schema uses new table name
   - Both point to the same underlying table

3. **Complete phase**:
   - Renames the actual table

## Notes

- The rename is done atomically during completion
- All foreign keys and indices are preserved
- Both schemas can operate normally during the migration
