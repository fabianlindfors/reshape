# add_foreign_key

Add a foreign key constraint to a table.

## Schema

```toml
[[actions]]
type = "add_foreign_key"
table = "table_name"          # Required: table to add FK to

    [actions.foreign_key]     # Required: foreign key definition
    columns = ["col1"]
    referenced_table = "other_table"
    referenced_columns = ["id"]
```

## Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `table` | string | Yes | Table to add foreign key to |
| `foreign_key.columns` | array | Yes | Columns in this table |
| `foreign_key.referenced_table` | string | Yes | Referenced table |
| `foreign_key.referenced_columns` | array | Yes | Referenced columns |

## Examples

### Simple Foreign Key

```toml
[[actions]]
type = "add_foreign_key"
table = "posts"

    [actions.foreign_key]
    columns = ["user_id"]
    referenced_table = "users"
    referenced_columns = ["id"]
```

### Composite Foreign Key

```toml
[[actions]]
type = "add_foreign_key"
table = "order_items"

    [actions.foreign_key]
    columns = ["order_id", "product_id"]
    referenced_table = "order_products"
    referenced_columns = ["order_id", "product_id"]
```

## Behavior

1. **Start phase**:
   - Creates foreign key with `NOT VALID` (doesn't lock for validation)
   - Validates the constraint (scans table but doesn't block writes)

2. **Complete phase**:
   - Renames constraint to final name: `{table}_{columns}_fkey`

## Notes

- The constraint is created as `NOT VALID` first to avoid long locks
- Existing data is validated after creation
- The foreign key is enforced immediately for new inserts/updates
- Constraint name follows pattern: `{table}_{columns}_fkey`
