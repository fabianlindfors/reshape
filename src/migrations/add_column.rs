use super::{common, Action, Column, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddColumn {
    pub table: String,
    pub up: Option<String>,
    pub column: Column,
}

impl AddColumn {
    fn temp_column_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_temp_column_{}_{}",
            ctx.prefix(),
            self.table,
            self.column.name,
        )
    }

    fn trigger_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_add_column_{}_{}",
            ctx.prefix(),
            self.table,
            self.column.name
        )
    }

    fn not_null_constraint_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_add_column_not_null_{}_{}",
            ctx.prefix(),
            self.table,
            self.column.name
        )
    }
}

#[typetag::serde(name = "add_column")]
impl Action for AddColumn {
    fn describe(&self) -> String {
        format!(
            "Adding column \"{}\" to \"{}\"",
            self.column.name, self.table
        )
    }

    fn run(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;
        let temp_column_name = self.temp_column_name(ctx);

        let mut definition_parts = vec![
            ("\"".to_string() + &temp_column_name.to_string() + "\""),
            self.column.data_type.to_string(),
        ];

        if let Some(default) = &self.column.default {
            definition_parts.push("DEFAULT".to_string());
            definition_parts.push(default.to_string());
        }

        if let Some(generated) = &self.column.generated {
            definition_parts.push("GENERATED".to_string());
            definition_parts.push(generated.to_string());
        }

        // Add column as NOT NULL
        let query = format!(
            r#"
			ALTER TABLE "{table}"
            ADD COLUMN IF NOT EXISTS {definition};
			"#,
            table = self.table,
            definition = definition_parts.join(" "),
        );
        db.run(&query).context("failed to add column")?;

        if let Some(up) = &self.up {
            let declarations: Vec<String> = table
                .columns
                .iter()
                .map(|column| {
                    format!(
                        "\"{alias}\" public.{table}.{real_name}%TYPE := NEW.\"{real_name}\";",
                        table = table.real_name,
                        alias = column.name,
                        real_name = column.real_name,
                    )
                })
                .collect();

            // Add triggers to fill in values as they are inserted/updated
            let query = format!(
                r#"
                CREATE OR REPLACE FUNCTION {trigger_name}()
                RETURNS TRIGGER AS $$
                BEGIN
                    IF NOT reshape.is_new_schema() THEN
                        DECLARE
                            {declarations}
                        BEGIN
                            NEW."{temp_column_name}" = {up};
                        END;
                    END IF;
                    RETURN NEW;
                END
                $$ language 'plpgsql';

                DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}";
                CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{table}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                "#,
                temp_column_name = temp_column_name,
                trigger_name = self.trigger_name(ctx),
                up = up,
                table = self.table,
                declarations = declarations.join("\n"),
            );
            db.run(&query).context("failed to create up trigger")?;
        }

        // Backfill values in batches
        if self.up.is_some() {
            common::batch_touch_rows(db, &table.real_name, &temp_column_name)
                .context("failed to batch update existing rows")?;
        }

        // Add a temporary NOT NULL constraint if the column shouldn't be nullable.
        // This constraint is set as NOT VALID so it doesn't apply to existing rows and
        // the existing rows don't need to be scanned under an exclusive lock.
        // Thanks to this, we can set the full column as NOT NULL later with minimal locking.
        if !self.column.nullable {
            let query = format!(
                r#"
                ALTER TABLE "{table}"
                ADD CONSTRAINT "{constraint_name}"
                CHECK ("{column}" IS NOT NULL) NOT VALID
                "#,
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
                column = temp_column_name,
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
        let mut transaction = db.transaction().context("failed to create transaction")?;

        // Remove triggers and procedures
        let query = format!(
            r#"
            DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}";
            DROP FUNCTION IF EXISTS "{trigger_name}";
            "#,
            table = self.table,
            trigger_name = self.trigger_name(ctx),
        );
        transaction
            .run(&query)
            .context("failed to drop up trigger")?;

        // Update column to be NOT NULL if necessary
        if !self.column.nullable {
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
            transaction
                .run(&query)
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
                column = self.temp_column_name(ctx),
            );
            transaction
                .run(&query)
                .context("failed to set column as NOT NULL")?;

            // Drop the temporary constraint
            let query = format!(
                r#"
                ALTER TABLE "{table}"
                DROP CONSTRAINT "{constraint_name}"
                "#,
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
            );
            transaction
                .run(&query)
                .context("failed to drop NOT NULL constraint")?;
        }

        // Rename the temporary column to its real name
        transaction
            .run(&format!(
                r#"
                ALTER TABLE "{table}"
                RENAME COLUMN "{temp_column_name}" TO "{column_name}"
                "#,
                table = self.table,
                temp_column_name = self.temp_column_name(ctx),
                column_name = self.column.name,
            ))
            .context("failed to rename column to final name")?;

        Ok(Some(transaction))
    }

    fn update_schema(&self, ctx: &MigrationContext, schema: &mut Schema) {
        schema.change_table(&self.table, |table_changes| {
            table_changes.change_column(&self.column.name, |column_changes| {
                column_changes.set_column(&self.temp_column_name(ctx));
            })
        });
    }

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        // Remove column
        let query = format!(
            r#"
            ALTER TABLE "{table}"
            DROP COLUMN IF EXISTS "{column}"
            "#,
            table = self.table,
            column = self.temp_column_name(ctx),
        );
        db.run(&query).context("failed to drop column")?;

        // Remove triggers and procedures
        let query = format!(
            r#"
            DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}";
            DROP FUNCTION IF EXISTS "{trigger_name}";
            "#,
            table = self.table,
            trigger_name = self.trigger_name(ctx),
        );
        db.run(&query).context("failed to drop up trigger")?;

        Ok(())
    }
}
