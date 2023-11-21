use super::{common, Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::{bail, Context};
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
            "#,
            table = self.table,
            column = self.column,
            trigger_name = self.trigger_name(ctx),
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
            "#,
            trigger_name = self.trigger_name(ctx),
        ))
        .context("failed to drop down trigger")?;

        Ok(())
    }
}
