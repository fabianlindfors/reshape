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

    fn null_constraint_trigger_name(&self, ctx: &MigrationContext) -> String {
        format!(
            "{}_remove_column_{}_{}_nn",
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

                let from_table = schema.get_table(db, &from_table)?;

                let declarations: Vec<String> = from_table
                    .columns
                    .iter()
                    .map(|column| {
                        format!(
                            "{alias} public.{table}.{real_name}%TYPE := NEW.{real_name};",
                            table = from_table.real_name,
                            alias = column.name,
                            real_name = column.real_name,
                        )
                    })
                    .collect();

                let maybe_null_check = if !column.nullable {
                    // Replace NOT NULL constraint with a constraint trigger that only triggers on the old schema.
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
                        trigger_name = self.null_constraint_trigger_name(ctx),
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

                let query = format!(
                    r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    #variable_conflict use_variable
                    BEGIN
                        IF reshape.is_new_schema() THEN
                            DECLARE
                                {declarations}
                            BEGIN
                                {maybe_null_check}
                                UPDATE "migration_{existing_schema_name}"."{changed_table}" "{changed_table}"
                                SET "{column_name}" = {value}
                                WHERE {where};
                            END;
                        END IF;
                        RETURN NEW;
                    END
                    $$ language 'plpgsql';

                    DROP TRIGGER IF EXISTS "{trigger_name}" ON "{from_table_real}";
                    CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{from_table_real}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                    "#,
                    changed_table = self.table,
                    from_table_real = from_table.real_name,
                    column_name = self.column,
                    trigger_name = self.trigger_name(ctx),
                    declarations = declarations.join("\n"),
                );
                db.run(&query).context("failed to create down trigger")?;
            }
        }

        if !column.nullable {}

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
        let trigger_table = match &self.down {
            Some(Transformation::Update {
                table,
                value: _,
                r#where: _,
            }) => table,
            _ => &self.table,
        };
        let query = format!(
            r#"
            ALTER TABLE "{table}"
            DROP COLUMN IF EXISTS "{column}";

            DROP TRIGGER IF EXISTS "{trigger_name}" ON "{trigger_table}";
            DROP FUNCTION IF EXISTS "{trigger_name}";

            DROP TRIGGER IF EXISTS "{null_trigger_name}" ON "{table}";
            DROP FUNCTION IF EXISTS "{null_trigger_name}";
            "#,
            table = self.table,
            column = self.column,
            trigger_name = self.trigger_name(ctx),
            null_trigger_name = self.null_constraint_trigger_name(ctx),
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
        // We might have removed the NOT NULL check so we reinstate it
        db.run(&format!(
            r#"
            ALTER TABLE {table}
            ALTER COLUMN {column}
            SET NOT NULL
            "#,
            table = self.table,
            column = self.column
        ))
        .context("failed to reinstate column not null constraint")?;

        // Remove function and trigger
        let trigger_table = match &self.down {
            Some(Transformation::Update {
                table,
                value: _,
                r#where: _,
            }) => table,
            _ => &self.table,
        };
        db.run(&format!(
            r#"
            DROP TRIGGER IF EXISTS "{trigger_name}" ON "{trigger_table}";
            DROP FUNCTION IF EXISTS "{trigger_name}";

            DROP TRIGGER IF EXISTS "{null_trigger_name}" ON "{table}";
            DROP FUNCTION IF EXISTS "{null_trigger_name}";
            "#,
            table = self.table,
            trigger_name = self.trigger_name(ctx),
            null_trigger_name = self.null_constraint_trigger_name(ctx),
        ))
        .context("failed to drop down trigger")?;

        Ok(())
    }
}
