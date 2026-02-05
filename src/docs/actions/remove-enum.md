# remove_enum

Remove a PostgreSQL enum type.

## Schema

```toml
[[actions]]
type = "remove_enum"
name = "enum_name"            # Required: enum type name
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Enum type name to remove |

## Example

```toml
[[actions]]
type = "remove_enum"
name = "legacy_status"
```

## Behavior

1. **Start phase**:
   - Validates that the enum exists

2. **Complete phase**:
   - Drops the enum type

## Notes

- Ensure no columns are using this enum before removing it
- Remove or alter columns using this enum first
- The enum is only dropped during the complete phase
