use super::{Action, Column, Context};
use crate::{db::Conn, schema::Schema};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<Column>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[typetag::serde(name = "create_table")]
impl Action for CreateTable {
    fn describe(&self) -> String {
        format!("Creating table \"{}\"", self.name)
    }

    fn run(&self, _ctx: &Context, db: &mut dyn Conn, _schema: &Schema) -> anyhow::Result<()> {
        let mut definition_rows: Vec<String> = self
            .columns
            .iter()
            .map(|column| {
                let mut parts = vec![column.name.to_string(), column.data_type.to_string()];

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

        let primary_key_columns = self.primary_key.join(", ");
        definition_rows.push(format!("PRIMARY KEY ({})", primary_key_columns));

        for foreign_key in &self.foreign_keys {
            definition_rows.push(format!(
                "FOREIGN KEY ({columns}) REFERENCES {table} ({referenced_columns})",
                columns = foreign_key.columns.join(", "),
                table = foreign_key.referenced_table,
                referenced_columns = foreign_key.referenced_columns.join(", "),
            ));
        }

        db.run(&format!(
            "CREATE TABLE {} (
                {}
            )",
            self.name,
            definition_rows.join(",\n"),
        ))?;
        Ok(())
    }

    fn complete(&self, _ctx: &Context, _db: &mut dyn Conn, _schema: &Schema) -> anyhow::Result<()> {
        // Do nothing
        Ok(())
    }

    fn update_schema(&self, _ctx: &Context, _schema: &mut Schema) -> anyhow::Result<()> {
        Ok(())
    }

    fn abort(&self, _ctx: &Context, db: &mut dyn Conn) -> anyhow::Result<()> {
        let query = format!("DROP TABLE IF EXISTS {table}", table = self.name,);
        db.run(&query)?;

        Ok(())
    }
}
