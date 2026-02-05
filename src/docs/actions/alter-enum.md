# alter_enum

Add new values to an existing PostgreSQL enum type.

## Schema

```toml
[[actions]]
type = "alter_enum"
name = "enum_name"            # Required: enum type name

    [[actions.add]]           # Required: values to add
    value = "new_value"
    down = "'fallback'"       # SQL expression for old schema
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Enum type name |
| `add[].value` | string | Yes | New enum value to add |
| `add[].down` | string | Yes | SQL expression for backward compatibility |

## Example

### Add Single Value

```toml
[[actions]]
type = "alter_enum"
name = "status"

    [[actions.add]]
    value = "cancelled"
    down = "'pending'"
```

### Add Multiple Values

```toml
[[actions]]
type = "alter_enum"
name = "priority"

    [[actions.add]]
    value = "urgent"
    down = "'high'"

    [[actions.add]]
    value = "trivial"
    down = "'low'"
```

## Behavior

1. **Start phase**:
   - Creates a new temporary enum with all values (old + new)
   - For each column using this enum:
     - Creates a temporary column with the new enum type
     - Sets up triggers to sync values
     - Maps new values to `down` expression for old schema

2. **During migration**:
   - Old schema uses original enum (new values mapped via `down`)
   - New schema uses new enum with all values

3. **Complete phase**:
   - Drops original columns using old enum
   - Renames temporary columns

## Notes

- The `down` expression must evaluate to a valid value in the original enum
- All columns using the enum are automatically updated
- PostgreSQL's built-in `ALTER TYPE ADD VALUE` isn't used because it can't be undone
- This action may take time for tables with many rows using the enum
