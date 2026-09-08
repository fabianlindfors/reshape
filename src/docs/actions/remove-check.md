# remove_check

Remove a check constraint from a table.

## Schema

```toml
[[actions]]
type = "remove_check"
table = "table_name"          # Required: table with the check
check = "constraint_name"     # Required: constraint name
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table containing the check |
| `check` | string | Yes | Name of the constraint to remove |

## Example

```toml
[[actions]]
type = "remove_check"
table = "users"
check = "users_age_check"
```

## Behavior

1. **Start phase**:
   - Validates that the check exists
   - Check remains enforced

2. **Complete phase**:
   - Drops the check constraint

## Notes

- The check remains enforced during the migration period, so the new application must keep writing rows which satisfy it until the migration is completed
- This ensures data consistency for the old schema
- The constraint is only removed during completion
- To replace a check, remove it and add a new one with the same name in the same migration (see `add_check`)
- Find constraint names using: `\d table_name` in psql
