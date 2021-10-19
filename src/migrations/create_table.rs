use super::{Action, Column};
use crate::{
    db::Conn,
    schema::{Schema, Table},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<Column>,
}

#[typetag::serde(name = "create_table")]
impl Action for CreateTable {
    fn describe(&self) -> String {
        format!("Creating table \"{}\"", self.name)
    }

    fn run(&self, db: &mut dyn Conn, _schema: &Schema) -> anyhow::Result<()> {
        let column_definitions: Vec<String> = self
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

                parts.join(" ")
            })
            .collect();

        let query = format!(
            "CREATE TABLE {} (
                {}
            )",
            self.name,
            column_definitions.join(",\n"),
        );
        db.run(&query)?;
        Ok(())
    }

    fn complete(&self, _db: &mut dyn Conn, _schema: &Schema) -> anyhow::Result<()> {
        // Do nothing
        Ok(())
    }

    fn update_schema(&self, schema: &mut Schema) -> anyhow::Result<()> {
        let mut table = Table::new(self.name.to_string());
        for column in &self.columns {
            table.add_column(crate::schema::Column {
                name: column.name.to_string(),
                real_name: None,
                data_type: column.data_type.to_string(),
                nullable: column.nullable,
            });
        }
        schema.add_table(table);

        Ok(())
    }
}