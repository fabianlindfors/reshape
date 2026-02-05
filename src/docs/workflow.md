# Reshape Workflow

Reshape uses a three-phase deployment workflow to achieve zero-downtime schema migrations.

## Phase 1: Start Migration

```bash
reshape migration start
```

This phase:
1. Creates temporary columns/triggers for schema changes
2. Sets up views in a new schema (`migration_{name}`)
3. Backfills data to temporary columns
4. Both old and new schemas are now available

**After this phase:** Both old and new application versions can run simultaneously.

## Phase 2: Application Rollout

Update your application to use the new schema:

```sql
SET search_path TO migration_{migration_name}
```

Get the exact query using:

```bash
reshape schema-query
```

**During this phase:**
- Old application instances use the previous schema
- New application instances use the new schema
- Triggers keep data synchronized between schemas

## Phase 3: Complete Migration

```bash
reshape migration complete
```

This phase:
1. Removes the old schema
2. Renames temporary columns to final names
3. Drops triggers and helper functions
4. Updates migration state

**After this phase:** Only the new schema is available.

## Abort Migration

If issues are discovered during Phase 2:

```bash
reshape migration abort
```

This safely rolls back:
1. Removes new schema and views
2. Drops temporary columns
3. Removes triggers
4. Restores previous state

**Note:** Abort is only available before `complete` is called.

## Quick Migration

For non-production environments, you can apply and complete in one step:

```bash
reshape migration start --complete
```

## State Diagram

```
Idle → Applying → InProgress → Completing → Idle
          ↓            ↓
       Aborting ← ← ← ←
          ↓
        Idle
```

- `Idle`: No migration in progress
- `Applying`: Migration being applied (automatic rollback on failure)
- `InProgress`: Migration applied, waiting for completion
- `Completing`: Migration being finalized (cannot abort)
- `Aborting`: Migration being rolled back
