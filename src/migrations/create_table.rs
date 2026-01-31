use std::collections::HashMap;

use super::{common::ForeignKey, Action, Column, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    migrations::common,
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<Column>,
    pub primary_key: Vec<String>,

    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,

    pub up: Option<Transformation>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Transformation {
    table: String,
    values: HashMap<String, String>,
    upsert_constraint: Option<String>,
}

impl CreateTable {
    fn trigger_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_create_table_{}", ctx.prefix(), self.name)
    }
}

#[typetag::serde(name = "create_table")]
impl Action for CreateTable {
    fn describe(&self) -> String {
        format!("Creating table \"{}\"", self.name)
    }

    fn run(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let mut definition_rows: Vec<String> = self
            .columns
            .iter()
            .map(|column| {
                let mut parts = vec![format!("\"{}\"", column.name), column.data_type.to_string()];

                if let Some(default) = &column.default {
                    parts.push("DEFAULT".to_string());
                    parts.push(default.to_string());
                }

                if !column.nullable {
                    parts.push("NOT NULL".to_string());
                }

                if let Some(generated) = &column.generated {
                    parts.push("GENERATED".to_string());
                    parts.push(generated.to_string());
                }

                parts.join(" ")
            })
            .collect();

        let primary_key_columns = self
            .primary_key
            .iter()
            // Add quotes around all column names
            .map(|col| format!("\"{}\"", col))
            .collect::<Vec<String>>()
            .join(", ");
        definition_rows.push(format!("PRIMARY KEY ({})", primary_key_columns));

        for foreign_key in &self.foreign_keys {
            // Add quotes around all column names
            let columns: Vec<String> = foreign_key
                .columns
                .iter()
                .map(|col| format!("\"{}\"", col))
                .collect();

            let referenced_table = schema.get_table(db, &foreign_key.referenced_table)?;
            let referenced_columns: Vec<String> = referenced_table
                .real_column_names(&foreign_key.referenced_columns)
                .map(|col| format!("\"{}\"", col))
                .collect();

            definition_rows.push(format!(
                r#"
                FOREIGN KEY ({columns}) REFERENCES "{table}" ({referenced_columns})
                "#,
                columns = columns.join(", "),
                table = referenced_table.real_name,
                referenced_columns = referenced_columns.join(", "),
            ));
        }

        let query = &format!(
            r#"
            CREATE TABLE "{name}" (
                {definition}
            )
            "#,
            name = self.name,
            definition = definition_rows.join(",\n"),
        );
        db.run(query).context("failed to create table")?;

        if let Some(Transformation {
            table: from_table,
            values,
            upsert_constraint,
        }) = &self.up
        {
            let from_table = schema.get_table(db, from_table)?;

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

            let (insert_columns, insert_values): (Vec<&str>, Vec<&str>) = values
                .iter()
                .map(|(k, v)| -> (&str, &str) { (k, v) }) // Force &String to &str
                .unzip();

            let update_set: Vec<String> = values
                .iter()
                .map(|(field, value)| format!("\"{field}\" = {value}"))
                .collect();

            // Constraint to check for conflicts. Defaults to the primary key constraint.
            let conflict_constraint_name = match upsert_constraint {
                Some(custom_constraint) => custom_constraint.clone(),
                _ => format!("{table}_pkey", table = self.name),
            };

            // Add triggers to fill in values as they are inserted/updated
            let query = format!(
                r#"
                    CREATE OR REPLACE FUNCTION {trigger_name}()
                    RETURNS TRIGGER AS $$
                    #variable_conflict use_variable
                    BEGIN
                        IF NOT reshape.is_new_schema() THEN
                            DECLARE
                                {declarations}
                            BEGIN
                                INSERT INTO public."{changed_table_real}" ({columns})
                                VALUES ({values})
                                ON CONFLICT ON CONSTRAINT "{conflict_constraint_name}"
                                DO UPDATE SET
                                    {updates};
                            END;
                        END IF;
                        RETURN NEW;
                    END
                    $$ language 'plpgsql';

                    DROP TRIGGER IF EXISTS "{trigger_name}" ON "{from_table_real}";
                    CREATE TRIGGER "{trigger_name}" BEFORE UPDATE OR INSERT ON "{from_table_real}" FOR EACH ROW EXECUTE PROCEDURE {trigger_name}();
                    "#,
                changed_table_real = self.name,
                from_table_real = from_table.real_name,
                trigger_name = self.trigger_name(ctx),
                declarations = declarations.join("\n"),
                columns = insert_columns.join(", "),
                values = insert_values.join(", "),
                updates = update_set.join(",\n"),
            );
            db.run(&query).context("failed to create up trigger")?;

            // Backfill values in batches by touching the from table
            common::batch_touch_rows(db, &from_table.real_name, None)
                .context("failed to batch update existing rows")?;
        }

        Ok(())
    }

    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        // Remove triggers and procedures
        let query = format!(
            r#"
            DROP FUNCTION IF EXISTS "{trigger_name}" CASCADE;
            "#,
            trigger_name = self.trigger_name(ctx),
        );
        db.run(&query).context("failed to drop up trigger")?;

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        // Remove triggers and procedures
        let query = format!(
            r#"
            DROP FUNCTION IF EXISTS "{trigger_name}" CASCADE;
            "#,
            trigger_name = self.trigger_name(ctx),
        );
        db.run(&query).context("failed to drop up trigger")?;

        db.run(&format!(
            r#"
            DROP TABLE IF EXISTS "{name}"
            "#,
            name = self.name,
        ))
        .context("failed to drop table")?;

        Ok(())
    }
}
