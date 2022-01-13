use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    migrations::common,
    schema::Schema,
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AlterColumn {
    pub table: String,
    pub column: String,
    pub up: Option<String>,
    pub down: Option<String>,
    pub changes: ColumnChanges,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ColumnChanges {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub data_type: Option<String>,
    pub nullable: Option<bool>,
    pub default: Option<String>,
}

#[typetag::serde(name = "alter_column")]
impl Action for AlterColumn {
    fn describe(&self) -> String {
        format!("Altering column \"{}\" on \"{}\"", self.column, self.table)
    }

    fn run(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        // If we are only changing the name of a column, we don't have to do anything at this stage
        // We'll set the new schema to point to the old column. When the migration is completed,
        // we rename the actual column.
        if self.can_short_circuit() {
            return Ok(());
        }

        let table = schema.get_table(db, &self.table)?;

        let column = table
            .columns
            .iter()
            .find(|column| column.name == self.column)
            .ok_or_else(|| anyhow!("no such column {} exists", self.column))?;

        let temporary_column_name = self.temporary_column_name(ctx);
        let temporary_column_type = self.changes.data_type.as_ref().unwrap_or(&column.data_type);

        // Add temporary, nullable column
        let mut temp_column_definition_parts: Vec<&str> =
            vec![&temporary_column_name, temporary_column_type];

        // Use either new default value or existing one if one exists
        let default_value = self
            .changes
            .default
            .as_ref()
            .or_else(|| column.default.as_ref());
        if let Some(default) = default_value {
            temp_column_definition_parts.push("DEFAULT");
            temp_column_definition_parts.push(default);
        }

        let query = format!(
            r#"
			ALTER TABLE "{table}"
            ADD COLUMN IF NOT EXISTS {temp_column_definition}
			"#,
            table = self.table,
            temp_column_definition = temp_column_definition_parts.join(" "),
        );
        db.run(&query).context("failed to add temporary column")?;

        // If up or down wasn't provided, we default to simply moving the value over.
        // This is the correct behaviour for example when only changing the default value.
        let up = self.up.as_ref().unwrap_or(&self.column);
        let down = self.down.as_ref().unwrap_or(&self.column);

        let declarations: Vec<String> = table
            .columns
            .iter()
            .filter(|column| column.name != self.column)
            .map(|column| {
                format!(
                    "{alias} public.{table}.{real_name}%TYPE := NEW.{real_name};",
                    table = table.real_name,
                    alias = column.name,
                    real_name = column.real_name,
                )
            })
            .collect();

        let query = format!(
            r#"
                CREATE OR REPLACE FUNCTION {up_trigger}()
                RETURNS TRIGGER AS $$
                BEGIN
                    IF reshape.is_old_schema() THEN
                        DECLARE
                            {declarations}
                            {existing_column} public.{table}.{existing_column_real}%TYPE := NEW.{existing_column_real};
                        BEGIN
                            NEW.{temp_column} = {up};
                        END;
                    END IF;
                    RETURN NEW;
                END
                $$ language 'plpgsql';

                DROP TRIGGER IF EXISTS "{up_trigger}" ON "{table}";
                CREATE TRIGGER "{up_trigger}" BEFORE INSERT OR UPDATE ON "{table}" FOR EACH ROW EXECUTE PROCEDURE {up_trigger}();

                CREATE OR REPLACE FUNCTION {down_trigger}()
                RETURNS TRIGGER AS $$
                BEGIN
                    IF NOT reshape.is_old_schema() THEN
                        DECLARE
                            {declarations}
                            {existing_column} public.{table}.{temp_column}%TYPE := NEW.{temp_column};
                        BEGIN
                            NEW.{existing_column_real} = {down};
                        END;
                    END IF;
                    RETURN NEW;
                END
                $$ language 'plpgsql';

                DROP TRIGGER IF EXISTS "{down_trigger}" ON "{table}";
                CREATE TRIGGER "{down_trigger}" BEFORE INSERT OR UPDATE ON "{table}" FOR EACH ROW EXECUTE PROCEDURE {down_trigger}();
                "#,
            existing_column = &self.column,
            existing_column_real = column.real_name,
            temp_column = self.temporary_column_name(ctx),
            up = up,
            down = down,
            table = self.table,
            up_trigger = self.up_trigger_name(ctx),
            down_trigger = self.down_trigger_name(ctx),
            declarations = declarations.join("\n"),
        );
        db.run(&query)
            .context("failed to create up and down triggers")?;

        // Backfill values in batches by touching the previous column
        common::batch_touch_rows(db, &table.real_name, &column.real_name)
            .context("failed to batch update existing rows")?;

        // Duplicate any indices to the temporary column
        let indices = common::get_indices_for_column(db, &table.real_name, &column.real_name)?;
        for (index_name, index_oid) in indices {
            let index_columns: Vec<String> = common::get_index_columns(db, &index_name)?
                .into_iter()
                .map(|idx_column| {
                    // Replace column with temporary column for new index
                    if idx_column == column.real_name {
                        temporary_column_name.to_string()
                    } else {
                        idx_column
                    }
                })
                .collect();
            let temp_index_name = self.temp_index_name(ctx, index_oid);

            db.query(&format!(
                r#"
                CREATE INDEX CONCURRENTLY IF NOT EXISTS "{new_index_name}" ON "{table}" ({columns})
                "#,
                new_index_name = temp_index_name,
                table = table.real_name,
                columns = index_columns.join(", "),
            ))
            .context("failed to create temporary index")?;
        }

        // Add a temporary NOT NULL constraint if the column shouldn't be nullable.
        // This constraint is set as NOT VALID so it doesn't apply to existing rows and
        // the existing rows don't need to be scanned under an exclusive lock.
        // Thanks to this, we can set the full column as NOT NULL later with minimal locking.
        if !column.nullable {
            let query = format!(
                r#"
                ALTER TABLE "{table}"
                ADD CONSTRAINT "{constraint_name}"
                CHECK ("{column}" IS NOT NULL) NOT VALID
                "#,
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
                column = self.temporary_column_name(ctx),
            );
            db.run(&query)
                .context("failed to add NOT NULL constraint")?;
        }

        Ok(())
    }

    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        if self.can_short_circuit() {
            if let Some(new_name) = &self.changes.name {
                let query = format!(
                    r#"
			        ALTER TABLE "{table}"
			        RENAME COLUMN "{existing_name}" TO "{new_name}"
			        "#,
                    table = self.table,
                    existing_name = self.column,
                    new_name = new_name,
                );
                db.run(&query).context("failed to rename column")?;
            }
            return Ok(None);
        }

        // Update column to be NOT NULL if necessary
        let has_not_null_constraint = !db
            .query_with_params(
                "
                SELECT constraint_name
                FROM information_schema.constraint_column_usage
                WHERE constraint_name = $1
                ",
                &[&self.not_null_constraint_name(ctx)],
            )
            .context("failed to get any NOT NULL constraint")?
            .is_empty();
        if has_not_null_constraint {
            // Validate the temporary constraint (should always be valid).
            // This performs a sequential scan but does not take an exclusive lock.
            let query = format!(
                r#"
                ALTER TABLE "{table}"
                VALIDATE CONSTRAINT "{constraint_name}"
                "#,
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
            );
            db.run(&query)
                .context("failed to validate NOT NULL constraint")?;

            // Update the column to be NOT NULL.
            // This requires an exclusive lock but since PG 12 it can check
            // the existing constraint for correctness which makes the lock short-lived.
            // Source: https://dba.stackexchange.com/a/268128
            let query = format!(
                r#"
                ALTER TABLE "{table}"
                ALTER COLUMN "{column}" SET NOT NULL
                "#,
                table = self.table,
                column = self.temporary_column_name(ctx),
            );
            db.run(&query).context("failed to set column as NOT NULL")?;

            // Drop the temporary constraint
            let query = format!(
                r#"
                ALTER TABLE "{table}"
                DROP CONSTRAINT "{constraint_name}"
                "#,
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
            );
            db.run(&query)
                .context("failed to drop NOT NULL constraint")?;
        }

        // Replace old indices with the new temporary ones created for the temporary column
        let indices = common::get_indices_for_column(db, &self.table, &self.column)?;
        for (current_index_name, index_oid) in indices {
            // To keep the index handling idempotent, we need to do the following:
            // 1. Add a prefix to the existing index
            // 2. Rename temporary index to its final name
            // 3. Drop existing index concurrently

            // Add prefix (if not already added) to existing index
            let prefix = "__reshape_old";
            let target_index_name = current_index_name.trim_start_matches(prefix);
            let old_index_name = format!("{}_{}", prefix, target_index_name);
            db.query(&format!(
                r#"
                ALTER INDEX IF EXISTS "{current_name}" RENAME TO "{new_name}"
                "#,
                current_name = target_index_name,
                new_name = old_index_name,
            ))
            .context("failed to rename old index")?;

            // Rename temporary index to real name
            let temp_index_name = self.temp_index_name(ctx, index_oid);
            db.query(&format!(
                r#"
                ALTER INDEX IF EXISTS "{temp_index_name}" RENAME TO "{target_index_name}"
                "#,
                temp_index_name = temp_index_name,
                target_index_name = target_index_name,
            ))
            .context("failed to rename temporary index")?;

            // Drop old index concurrently
            db.query(&format!(
                r#"
                DROP INDEX CONCURRENTLY IF EXISTS "{old_index_name}"
                "#,
                old_index_name = old_index_name,
            ))
            .context("failed to drop old index")?;
        }

        // Remove old column
        let query = format!(
            r#"
            ALTER TABLE "{table}" DROP COLUMN IF EXISTS "{column}" CASCADE
			"#,
            table = self.table,
            column = self.column,
        );
        db.run(&query).context("failed to drop old column")?;

        // Rename temporary column
        let column_name = self.changes.name.as_deref().unwrap_or(&self.column);
        let query = format!(
            r#"
            ALTER TABLE "{table}" RENAME COLUMN "{temp_column}" TO "{name}"
			"#,
            table = self.table,
            temp_column = self.temporary_column_name(ctx),
            name = column_name,
        );
        db.run(&query)
            .context("failed to rename temporary column")?;

        // Remove triggers and procedures
        let query = format!(
            r#"
            DROP TRIGGER IF EXISTS "{up_trigger}" ON "{table}";
            DROP FUNCTION IF EXISTS "{up_trigger}";

            DROP TRIGGER IF EXISTS "{down_trigger}" ON "{table}";
            DROP FUNCTION IF EXISTS "{down_trigger}";
            "#,
            table = self.table,
            up_trigger = self.up_trigger_name(ctx),
            down_trigger = self.down_trigger_name(ctx),
        );
        db.run(&query)
            .context("failed to drop up and down triggers")?;

        Ok(None)
    }

    fn update_schema(&self, ctx: &MigrationContext, schema: &mut Schema) {
        // If we are only changing the name of a column, we haven't created a temporary column
        // Instead, we rename the schema column but point it to the old column
        if self.can_short_circuit() {
            if let Some(new_name) = &self.changes.name {
                schema.change_table(&self.table, |table_changes| {
                    table_changes.change_column(&self.column, |column_changes| {
                        column_changes.set_name(new_name);
                    });
                });
            }

            return;
        }

        schema.change_table(&self.table, |table_changes| {
            table_changes.change_column(&self.column, |column_changes| {
                column_changes.set_column(&self.temporary_column_name(ctx));
            });
        });
    }

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        // Safely remove any indices created for the temporary column
        let temp_column_name = self.temporary_column_name(ctx);
        let indices = common::get_indices_for_column(db, &self.table, &temp_column_name)?;
        for (_, index_oid) in indices {
            let temp_index_name = self.temp_index_name(ctx, index_oid);
            db.query(&format!(
                r#"
                DROP INDEX CONCURRENTLY IF EXISTS "{index_name}"
                "#,
                index_name = temp_index_name,
            ))?;
        }

        // Drop temporary column
        let query = format!(
            r#"
			ALTER TABLE "{table}"
            DROP COLUMN IF EXISTS "{temp_column}";
			"#,
            table = self.table,
            temp_column = self.temporary_column_name(ctx),
        );
        db.run(&query).context("failed to drop temporary column")?;

        // Remove triggers and procedures
        let query = format!(
            r#"
            DROP TRIGGER IF EXISTS "{up_trigger}" ON "{table}";
            DROP FUNCTION IF EXISTS "{up_trigger}";

            DROP TRIGGER IF EXISTS "{down_trigger}" ON "{table}";
            DROP FUNCTION IF EXISTS "{down_trigger}";
            "#,
            table = self.table,
            up_trigger = self.up_trigger_name(ctx),
            down_trigger = self.down_trigger_name(ctx),
        );
        db.run(&query)
            .context("failed to drop up and down triggers")?;

        Ok(())
    }
}

impl AlterColumn {
    fn temporary_column_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_new_{}", ctx.prefix(), self.column)
    }

    fn up_trigger_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_alter_column_up_trigger", ctx.prefix())
    }

    fn down_trigger_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_alter_column_down_trigger", ctx.prefix_inverse())
    }

    fn not_null_constraint_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_alter_column_temporary", ctx.prefix())
    }

    fn temp_index_name(&self, ctx: &MigrationContext, index_oid: u32) -> String {
        format!("{}_alter_column_temp_index_{}", ctx.prefix(), index_oid)
    }

    fn can_short_circuit(&self) -> bool {
        self.changes.name.is_some()
            && self.changes.data_type.is_none()
            && self.changes.nullable.is_none()
            && self.changes.default.is_none()
    }
}
