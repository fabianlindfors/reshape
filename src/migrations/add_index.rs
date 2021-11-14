use super::{Action, Context};
use crate::{
    db::Conn,
    schema::{Column, Schema},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddIndex {
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
}

#[typetag::serde(name = "add_index")]
impl Action for AddIndex {
    fn describe(&self) -> String {
        format!("Adding index \"{}\" to table \"{}\"", self.name, self.table)
    }

    fn run(&self, _ctx: &Context, db: &mut dyn Conn, schema: &Schema) -> anyhow::Result<()> {
        let table = schema.find_table(&self.table)?;
        let column_real_names: Vec<&str> = self
            .columns
            .iter()
            .map(|col| table.find_column(col))
            .collect::<anyhow::Result<Vec<&Column>>>()?
            .iter()
            .map(|col| col.real_name())
            .collect();

        db.run(&format!(
            "
			CREATE INDEX CONCURRENTLY {name} ON {table} ({columns})
			",
            name = self.name,
            table = self.table,
            columns = column_real_names.join(", "),
        ))?;
        Ok(())
    }

    fn complete(&self, _ctx: &Context, _db: &mut dyn Conn, _schema: &Schema) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_schema(&self, _ctx: &Context, _schema: &mut Schema) -> anyhow::Result<()> {
        Ok(())
    }

    fn abort(&self, _ctx: &Context, db: &mut dyn Conn) -> anyhow::Result<()> {
        db.run(&format!(
            "
			DROP INDEX {name} ON {table}
			",
            name = self.name,
            table = self.table,
        ))?;
        Ok(())
    }
}
