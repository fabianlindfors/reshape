# remove_index

Remove an index from a table.

## Schema

```toml
[[actions]]
type = "remove_index"
index = "index_name"          # Required: index to remove
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `index` | string | Yes | Name of the index to remove |

## Example

```toml
[[actions]]
type = "remove_index"
index = "users_legacy_idx"
```

## Behavior

1. **Start phase**:
   - Index is NOT removed yet

2. **Complete phase**:
   - Drops index using `DROP INDEX CONCURRENTLY`
   - Does not block reads or writes

## Notes

- Indexes are dropped concurrently to avoid blocking
- The index remains available during the migration period
- Primary key and unique constraint indexes should be removed by modifying the constraint instead
