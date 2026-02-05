# remove_table

Remove an existing table.

## Schema

```toml
[[actions]]
type = "remove_table"
table = "table_name"          # Required: table to remove
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table to remove |

## Example

```toml
[[actions]]
type = "remove_table"
table = "legacy_data"
```

## Behavior

1. **Start phase**:
   - Table is NOT removed yet
   - New schema's view excludes the table

2. **During migration**:
   - Old schema can still access the table
   - New schema doesn't see the table

3. **Complete phase**:
   - Drops the table with CASCADE

## Notes

- The table is only removed during the complete phase
- All data in the table is permanently lost
- Foreign keys referencing this table should be removed first
- Use with caution - ensure no application code depends on this table
