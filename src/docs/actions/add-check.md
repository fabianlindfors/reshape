# add_check

Add a check constraint to a table.

## Schema

```toml
[[actions]]
type = "add_check"
table = "table_name"          # Required: table to add the check to

    [actions.check]           # Required: check definition
    name = "constraint_name"  # Required: name of the constraint
    expression = "col > 0"    # Required: SQL expression every row must satisfy
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table to add the check to |
| `check.name` | string | Yes | Name of the constraint |
| `check.expression` | string | Yes | SQL expression which every row must satisfy. May reference any column of the table by its current name. |

## Examples

### Check on a Single Column

```toml
[[actions]]
type = "add_check"
table = "users"

    [actions.check]
    name = "users_age_check"
    expression = "age >= 0"
```

### Check Spanning Several Columns

```toml
[[actions]]
type = "add_check"
table = "bookings"

    [actions.check]
    name = "bookings_period_check"
    expression = "ends_at > starts_at"
```

### Replacing an Existing Check

A check is replaced by removing it and adding a new one with the same name in the same migration.

```toml
[[actions]]
type = "remove_check"
table = "users"
check = "users_age_check"

[[actions]]
type = "add_check"
table = "users"

    [actions.check]
    name = "users_age_check"
    expression = "age >= -1"
```

## Behavior

1. **Start phase**:
   - Creates the check with `NOT VALID` (doesn't lock for validation)
   - Validates the constraint (scans table but doesn't block writes). If existing rows violate the check, the check is dropped again and the migration fails.

2. **Complete phase**:
   - Renames constraint to its final name

## Notes

- The check is enforced immediately for new inserts and updates, from both the old and the new schema. Reshape assumes the existing application already writes rows which satisfy it.
- Existing data is validated after creation, as for foreign keys
- The check fails if a constraint with the same name already exists, unless the same migration removes it first.
- A `NULL` result satisfies a check, following Postgres semantics.
