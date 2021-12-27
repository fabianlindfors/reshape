use super::{Action, Context};
use crate::{db::Conn, schema::Schema};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveColumn {
    pub table: String,
    pub column: String,
    pub down: Option<String>,
}

impl RemoveColumn {
    fn trigger_name(&self, ctx: &Context) -> String {
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

    fn run(&self, ctx: &Context, db: &mut dyn Conn, schema: &Schema) -> anyhow::Result<()> {
        // Add down trigger
        if let Some(down) = &self.down {
            let table = schema.get_table(db, &self.table)?;

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

            let query = format!(
                "
                CREATE OR REPLACE FUNCTION {trigger_name}()
                RETURNS TRIGGER AS $$
                BEGIN
                    IF NEW.{column_name} IS NULL THEN
                        DECLARE
                            {declarations}
                        BEGIN
                            NEW.{column_name} = {down};
                        END;
                    END IF;
                    RETURN NEW;
                END
                $$ language 'plpgsql';

                DROP TRIGGER IF EXISTS {trigger_name} ON {table};
                CREATE TRIGGER {trigger_name} BEFORE UPDATE OR INSERT ON {table} FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                ",
                column_name = self.column,
                trigger_name = self.trigger_name(&ctx),
                down = down,
                table = self.table,
                declarations = declarations.join("\n"),
            );
            db.run(&query)?;
        }

        Ok(())
    }

    fn complete(&self, ctx: &Context, db: &mut dyn Conn, _schema: &Schema) -> anyhow::Result<()> {
        // Remove column, function and trigger
        let query = format!(
            "
            ALTER TABLE {table}
            DROP COLUMN {column};

            DROP TRIGGER IF EXISTS {trigger_name} ON {table};
            DROP FUNCTION IF EXISTS {trigger_name};
            ",
            table = self.table,
            column = self.column,
            trigger_name = self.trigger_name(&ctx),
        );
        db.run(&query)?;

        Ok(())
    }

    fn update_schema(&self, _ctx: &Context, schema: &mut Schema) -> anyhow::Result<()> {
        schema.set_column_hidden(&self.table, &self.column);

        Ok(())
    }

    fn abort(&self, ctx: &Context, db: &mut dyn Conn) -> anyhow::Result<()> {
        // Remove function and trigger
        db.query(&format!(
            "
            DROP TRIGGER IF EXISTS {trigger_name} ON {table};
            DROP FUNCTION IF EXISTS {trigger_name};
            ",
            table = self.table,
            trigger_name = self.trigger_name(&ctx),
        ))?;

        Ok(())
    }
}
