use super::{common, Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveColumn {
    pub table: String,
    pub column: String,
    pub down: Option<Transformation>,
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

impl RemoveColumn {
    fn trigger_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_remove_column_{}_{}",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn reverse_trigger_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_remove_column_{}_{}_rev",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn not_null_constraint_trigger_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_remove_column_{}_{}_nn",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn not_null_constraint_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_add_column_not_null_{}_{}",
            ctx.prefix(),
            self.table,
            self.column
        )
    }
}

#[typetag::serde(name = "remove_column")]
impl Action for RemoveColumn {
    fn describe(&self) -> String {
        format!(
            "Removing column \"{}\" from \"{}\"",
            self.column, self.table
        )
    }

    fn run(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;
        let column = table
            .get_column(&self.column)
            .ok_or_else(|| anyhow!("no such column {} exists", self.column))?;

        // Add down trigger
        if let Some(down) = &self.down {
            let declarations: Vec<String> = table
                .columns
                .iter()
                .map(|column| {
                    format!(
                        "{alias} public.{table}.{real_name}%TYPE := NEW.{real_name};",
                        table = table.real_name,
                        alias = column.name,
                        real_name = column.real_name,
                    )
                })
                .collect();

            if let Transformation::Simple(down) = down {
                let query = format!(
                    r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    BEGIN
                        IF reshape.is_new_schema() THEN
                            DECLARE
                                {declarations}
                            BEGIN
                                NEW.{column_name} = {down};
                            END;
                        END IF;
                        RETURN NEW;
                    END
                    $$ language 'plpgsql';

                    DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}";
                    CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{table}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                    "#,
                    column_name = self.column,
                    trigger_name = self.trigger_name(ctx),
                    down = down,
                    table = self.table,
                    declarations = declarations.join("\n"),
                );
                db.run(&query).context("failed to create down trigger")?;
            }

            if let Transformation::Update {
                table: from_table,
                value,
                r#where,
            } = down
            {
                let existing_schema_name = match &ctx.existing_schema_name {
                    Some(name) => name,
                    None => bail!("can't use update without previous migration"),
                };

                let from_table = schema.get_table(db, from_table)?;

                let maybe_null_check = if !column.nullable {
                    // Replace NOT NULL constraint with a constraint trigger that only triggers on the old schema.
                    // We will add a null check to the down function on the new schema below as well to cover both cases.
                    // As we are using a complex down function, we must remove the NOT NULL check for the new schema.
                    // NOT NULL is not checked at the end of a transaction, but immediately upon update.
                    let query = format!(
                        r#"
                        CREATE OR REPLACE FUNCTION {trigger_name}()
                        RETURNS TRIGGER AS $$
                        BEGIN
                            IF NOT reshape.is_new_schema() THEN
                                IF NEW.{column} IS NULL THEN
                                    RAISE EXCEPTION '{column} can not be null';
                                END IF;
                            END IF;
                            RETURN NEW;
                        END
                        $$ language 'plpgsql';

                        DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}";

                        CREATE CONSTRAINT TRIGGER "{trigger_name}"
                            AFTER INSERT OR UPDATE
                            ON "{table}"
                            FOR EACH ROW
                            EXECUTE PROCEDURE {trigger_name}();
                        "#,
                        table = self.table,
                        trigger_name = self.not_null_constraint_trigger_name(ctx),
                        column = self.column,
                    );
                    db.run(&query)
                        .context("failed to create null constraint trigger")?;

                    db.run(&format!(
                        r#"
                        ALTER TABLE {table}
                        ALTER COLUMN {column}
                        DROP NOT NULL
                        "#,
                        table = self.table,
                        column = self.column
                    ))
                    .context("failed to remove column not null constraint")?;

                    format!(
                        r#"
                        IF {value} IS NULL THEN
                            RAISE EXCEPTION '{column_name} can not be null';
                        END IF;
                        "#,
                        column_name = self.column,
                    )
                } else {
                    "".to_string()
                };

                let into_variables = from_table
                    .columns
                    .iter()
                    .map(|column| {
                        format!(
                            "NEW.{real_name} AS {alias}",
                            alias = column.name,
                            real_name = column.real_name,
                        )
                    })
                    .collect::<Vec<String>>()
                    .join(", ");

                let query = format!(
                    r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    #variable_conflict use_variable
                    BEGIN
                        IF reshape.is_new_schema() THEN
                            DECLARE
                                {from_table} record;
                            BEGIN
                                SELECT {into_variables}
                                INTO {from_table};

                                {maybe_null_check}

                                -- Don't trigger reverse trigger when making this update
                                perform set_config('reshape.disable_triggers', 'TRUE', TRUE);

                                UPDATE "migration_{existing_schema_name}"."{changed_table}" "{changed_table}"
                                SET "{column_name}" = {value}
                                WHERE {where};

                                perform set_config('reshape.disable_triggers', '', TRUE);
                            END;
                        END IF;
                        RETURN NEW;
                    END
                    $$ language 'plpgsql';

                    DROP TRIGGER IF EXISTS "{trigger_name}" ON "{from_table_real}";
                    CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{from_table_real}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                    "#,
                    changed_table = self.table,
                    from_table = from_table.name,
                    from_table_real = from_table.real_name,
                    column_name = self.column,
                    trigger_name = self.trigger_name(ctx),
                );
                db.run(&query).context("failed to create down trigger")?;

                let changed_into_variables = table
                    .columns
                    .iter()
                    .map(|column| {
                        format!(
                            "NEW.{real_name} AS {alias}",
                            alias = column.name,
                            real_name = column.real_name,
                        )
                    })
                    .collect::<Vec<String>>()
                    .join(", ");

                let from_table_columns = from_table
                    .columns
                    .iter()
                    .map(|column| format!("{} as {}", column.real_name, column.name))
                    .collect::<Vec<String>>()
                    .join(", ");

                let query = format!(
                    r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    #variable_conflict use_variable
                    BEGIN
                        IF reshape.is_new_schema() AND NOT current_setting('reshape.disable_triggers', TRUE) = 'TRUE' THEN
                            DECLARE
                                {changed_table} record;
                                __temp_row record;
                            BEGIN
                                SELECT {changed_into_variables}
                                INTO {changed_table};

                                SELECT *
                                INTO __temp_row
                                FROM (
                                    SELECT {from_table_columns}
                                    FROM public.{from_table_real}
                                ) {from_table}
                                WHERE {where};

                                DECLARE
                                    {from_table} record;
                                BEGIN
                                    {from_table} := __temp_row;
                                    NEW.{column_name_real} = {value};
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
                    from_table = from_table.name,
                    from_table_real = from_table.real_name,
                    column_name_real = column.real_name,
                    trigger_name = self.reverse_trigger_name(ctx),
                    // declarations = declarations.join("\n"),
                );
                db.run(&query)
                    .context("failed to create reverse down trigger")?;
            }
        }

        Ok(())
    }

    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        let indices = common::get_indices_for_column(db, &self.table, &self.column)
            .context("failed getting column indices")?;

        for index in indices {
            db.run(&format!(
                "
                DROP INDEX CONCURRENTLY IF EXISTS {name}
                ",
                name = index.name,
            ))
            .context("failed to drop index")?;
        }

        // Remove column, function and trigger
        let query = format!(
            r#"
            ALTER TABLE "{table}"
            DROP COLUMN IF EXISTS "{column}";

            DROP FUNCTION IF EXISTS "{trigger_name}" CASCADE;
            DROP FUNCTION IF EXISTS "{reverse_trigger_name}" CASCADE;
            DROP FUNCTION IF EXISTS "{null_trigger_name}" CASCADE;
            "#,
            table = self.table,
            column = self.column,
            trigger_name = self.trigger_name(ctx),
            reverse_trigger_name = self.reverse_trigger_name(ctx),
            null_trigger_name = self.not_null_constraint_trigger_name(ctx),
        );
        db.run(&query)
            .context("failed to drop column and down trigger")?;

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, schema: &mut Schema) {
        schema.change_table(&self.table, |table_changes| {
            table_changes.change_column(&self.column, |column_changes| {
                column_changes.set_removed();
            })
        });
    }

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        // We might have temporaily removed the NOT NULL check and have to reinstate it
        let has_not_null_function = !db
            .query_with_params(
                "
                SELECT routine_name
                FROM information_schema.routines
                WHERE routine_schema = 'public'
                AND routine_name = $1
                ",
                &[&self.not_null_constraint_trigger_name(ctx)],
            )
            .context("failed to get any NOT NULL function")?
            .is_empty();

        if has_not_null_function {
            // Make column NOT NULL again without taking any long lived locks with a temporary constraint
            let query = format!(
                r#"
                 ALTER TABLE "{table}"
                 ADD CONSTRAINT "{constraint_name}"
                 CHECK ("{column}" IS NOT NULL) NOT VALID
                 "#,
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
                column = self.column,
            );
            db.run(&query)
                .context("failed to add NOT NULL constraint")?;

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

            // This ALTER TABLE call will not require any exclusive locks as it can use the validated constraint from above
            db.run(&format!(
                r#"
                ALTER TABLE {table}
                ALTER COLUMN {column}
                SET NOT NULL
                "#,
                table = self.table,
                column = self.column
            ))
            .context("failed to reinstate column NOT NULL")?;

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

        // Remove function and trigger
        db.run(&format!(
            r#"
            DROP FUNCTION IF EXISTS "{trigger_name}" CASCADE;
            DROP FUNCTION IF EXISTS "{reverse_trigger_name}" CASCADE;
            DROP FUNCTION IF EXISTS "{null_trigger_name}" CASCADE;
            "#,
            trigger_name = self.trigger_name(ctx),
            reverse_trigger_name = self.reverse_trigger_name(ctx),
            null_trigger_name = self.not_null_constraint_trigger_name(ctx),
        ))
        .context("failed to drop down trigger")?;

        Ok(())
    }
}
