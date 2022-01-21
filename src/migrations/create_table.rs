use super::{common::ForeignKey, Action, Column, MigrationContext};
use crate::{
    db::{Conn, Transaction},
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
}

#[typetag::serde(name = "create_table")]
impl Action for CreateTable {
    fn describe(&self) -> String {
        format!("Creating table \"{}\"", self.name)
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
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

        db.run(&format!(
            r#"
            CREATE TABLE "{name}" (
                {definition}
            )
            "#,
            name = self.name,
            definition = definition_rows.join(",\n"),
        ))
        .context("failed to create table")?;
        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        _db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        // Do nothing
        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        db.run(&format!(
            r#"
            DROP TABLE IF EXISTS {name}
            "#,
            name = self.name,
        ))
        .context("failed to drop table")?;

        Ok(())
    }
}
