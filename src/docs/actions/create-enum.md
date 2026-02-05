# create_enum

Create a new PostgreSQL enum type.

## Schema

```toml
[[actions]]
type = "create_enum"
name = "enum_name"            # Required: enum type name
values = ["val1", "val2"]     # Required: enum values
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Enum type name |
| `values` | array | Yes | List of enum values |

## Examples

### Basic Enum

```toml
[[actions]]
type = "create_enum"
name = "status"
values = ["pending", "active", "completed"]
```

### Enum for Column

Create enum and use it in a column:

```toml
# First, create the enum
[[actions]]
type = "create_enum"
name = "priority"
values = ["low", "medium", "high", "critical"]

# Then use it in a table
[[actions]]
type = "add_column"
table = "tasks"

    [actions.column]
    name = "priority"
    type = "priority"
    default = "'medium'"
```

## Behavior

1. **Start phase**:
   - Creates the enum type with specified values

2. **Complete phase**:
   - No action needed

3. **Abort**:
   - Drops the enum type

## Notes

- Enum values are case-sensitive
- Use `alter_enum` to add new values after creation
- Removing values from an enum requires recreating it
- Values in defaults must be quoted (e.g., `"'value'"`)
