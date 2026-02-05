# remove_foreign_key

Remove a foreign key constraint from a table.

## Schema

```toml
[[actions]]
type = "remove_foreign_key"
table = "table_name"          # Required: table with the FK
foreign_key = "constraint_name"  # Required: constraint name
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table containing the foreign key |
| `foreign_key` | string | Yes | Name of the constraint to remove |

## Example

```toml
[[actions]]
type = "remove_foreign_key"
table = "posts"
foreign_key = "posts_user_id_fkey"
```

## Behavior

1. **Start phase**:
   - Validates that the foreign key exists
   - Foreign key remains enforced

2. **Complete phase**:
   - Drops the foreign key constraint

## Notes

- The foreign key remains enforced during the migration period
- This ensures data consistency for the old schema
- The constraint is only removed during completion
- Find constraint names using: `\d table_name` in psql
