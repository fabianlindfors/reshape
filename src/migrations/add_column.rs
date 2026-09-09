use super::{
    common, quote_string_literal, Action, Column, MigrationContext, NameField, References,
    SqlField, TableScope,
};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddColumn {
    pub table: String,
    pub column: Column,
    pub up: Option<Transformation>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Transformation {
    Simple(String),
    Update {
        table: String,
        value: String,
        r#where: String,
    },
}

impl AddColumn {
    fn temp_column_name(&self, ctx: &MigrationContext) -> String {
        ctx.name("temp_column", &[&self.table, &self.column.name], "")
    }

    fn trigger_name(&self, ctx: &MigrationContext) -> String {
        ctx.name("add_column", &[&self.table, &self.column.name], "")
    }

    fn reverse_trigger_name(&self, ctx: &MigrationContext) -> String {
        ctx.name("add_column", &[&self.table, &self.column.name], "_rev")
    }

    fn not_null_constraint_name(&self, ctx: &MigrationContext) -> String {
        ctx.name("add_column_not_null", &[&self.table, &self.column.name], "")
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
            format!("\"{}\"", temp_column_name.to_string()),
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

        // Set the comment on the temporary column. Comments follow the column, so it
        // will still be there once the column is renamed to its final name.
        if let Some(comment) = &self.column.comment {
            db.run(&format!(
                r#"
                COMMENT ON COLUMN "{table}"."{column}" IS {comment}
                "#,
                table = self.table,
                column = temp_column_name,
                comment = quote_string_literal(comment),
            ))
            .context("failed to set column comment")?;
        }

        let declarations: Vec<String> = table
            .columns
            .iter()
            .map(|column| {
                format!(
                    "\"{alias}\" public.{table}.{real_name}%TYPE := NEW.{real_name};",
                    table = table.real_name,
                    alias = column.name,
                    real_name = column.real_name,
                )
            })
            .collect();

        if let Some(up) = &self.up {
            if let Transformation::Simple(up) = up {
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

                // Backfill values in batches
                common::batch_touch_rows(db, &table.real_name, Some(&temp_column_name))
                    .context("failed to batch update existing rows")?;
            }

            if let Transformation::Update {
                table: from_table,
                value,
                r#where,
            } = up
            {
                let from_table = schema.get_table(db, from_table)?;

                // When the source table is written in the old schema, update the matching
                // rows of the changed table
                let query = format!(
                    r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    #variable_conflict use_variable
                    BEGIN
                        IF NOT reshape.is_new_schema() THEN
                            DECLARE
                                {from_table} record;
                            BEGIN
                                SELECT {from_table_row} INTO {from_table};

                                -- Don't trigger reverse trigger when making this update
                                perform set_config('reshape.disable_triggers', 'TRUE', TRUE);

                                UPDATE public."{changed_table_real}" AS __target
                                SET "{temp_column_name}" = __values.value
                                FROM (
                                    SELECT "{changed_table}".ctid, ({value}) AS value
                                    FROM {changed_table_subquery}
                                    WHERE {where}
                                ) __values
                                WHERE __target.ctid = __values.ctid;

                                perform set_config('reshape.disable_triggers', '', TRUE);
                            END;
                        END IF;
                        RETURN NEW;
                    END
                    $$ language 'plpgsql';

                    DROP TRIGGER IF EXISTS "{trigger_name}" ON "{from_table_real}";
                    CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{from_table_real}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                    "#,
                    changed_table = table.name,
                    changed_table_real = table.real_name,
                    changed_table_subquery = common::table_with_current_columns(&table),
                    from_table = from_table.name,
                    from_table_real = from_table.real_name,
                    from_table_row = common::new_row_with_current_columns(&from_table),
                    trigger_name = self.trigger_name(ctx),
                    temp_column_name = temp_column_name,
                );
                db.run(&query).context("failed to create up trigger")?;

                // When the changed table is written in the old schema, fill in the new
                // column from the matching row of the source table
                let query = format!(
                    r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    #variable_conflict use_variable
                    BEGIN
                        IF NOT reshape.is_new_schema() AND NOT current_setting('reshape.disable_triggers', TRUE) = 'TRUE' THEN
                            DECLARE
                                {changed_table} record;
                                __from_row record;
                            BEGIN
                                SELECT {changed_table_row} INTO {changed_table};

                                SELECT "{from_table}".*
                                INTO __from_row
                                FROM {from_table_subquery}
                                WHERE {where};

                                DECLARE
                                    {from_table} record;
                                BEGIN
                                    {from_table} := __from_row;
                                    NEW."{temp_column_name}" = {value};
                                END;
                            END;
                        END IF;
                        RETURN NEW;
                    END
                    $$ language 'plpgsql';

                    DROP TRIGGER IF EXISTS "{trigger_name}" ON "{changed_table_real}";
                    CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{changed_table_real}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                    "#,
                    changed_table = table.name,
                    changed_table_real = table.real_name,
                    changed_table_row = common::new_row_with_current_columns(&table),
                    from_table = from_table.name,
                    from_table_subquery = common::table_with_current_columns(&from_table),
                    trigger_name = self.reverse_trigger_name(ctx),
                    temp_column_name = temp_column_name,
                );
                db.run(&query)
                    .context("failed to create reverse up trigger")?;

                // Backfill values in batches by touching the from table
                common::batch_touch_rows(db, &from_table.real_name, None)
                    .context("failed to batch update existing rows")?;
            }
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
            DROP FUNCTION IF EXISTS "{trigger_name}" CASCADE;
            DROP FUNCTION IF EXISTS "{reverse_trigger_name}" CASCADE;
            "#,
            trigger_name = self.trigger_name(ctx),
            reverse_trigger_name = self.reverse_trigger_name(ctx),
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

        if !self.column.nullable {
            common::rename_not_null_constraint(&mut transaction, &self.table, &self.column.name)?;
        }

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
            DROP FUNCTION IF EXISTS "{trigger_name}" CASCADE;
            DROP FUNCTION IF EXISTS "{reverse_trigger_name}" CASCADE;
            "#,
            trigger_name = self.trigger_name(ctx),
            reverse_trigger_name = self.reverse_trigger_name(ctx),
        );
        db.run(&query).context("failed to drop up trigger")?;

        Ok(())
    }

    fn sql_fields(&self) -> Vec<SqlField> {
        let mut fields = vec![];

        match &self.up {
            // `up` runs in a trigger on the table and can reference its existing columns
            Some(Transformation::Simple(up)) => {
                fields.push(SqlField::expression(
                    "up",
                    up,
                    References::table(&self.table),
                ));
            }
            // A cross-table `up` can reference both the table the values are taken from
            // and the table being changed
            Some(Transformation::Update {
                table,
                value,
                r#where,
            }) => {
                let tables = References::CrossTable(
                    TableScope::schema(table),
                    TableScope::schema(&self.table),
                );
                fields.push(SqlField::expression("up.value", value, tables.clone()));
                fields.push(SqlField::expression("up.where", r#where, tables));
            }
            None => {}
        }

        if let Some(default) = &self.column.default {
            fields.push(SqlField::expression(
                "column.default",
                default,
                References::Forbidden,
            ));
        }

        fields
    }

    fn name_fields(&self) -> Vec<NameField> {
        let mut fields = vec![NameField::table("table", &self.table)];

        if let Some(Transformation::Update { table, .. }) = &self.up {
            fields.push(NameField::table("up.table", table));
        }

        fields
    }
}
