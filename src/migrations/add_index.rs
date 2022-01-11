use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
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

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;

        let column_real_names: Vec<String> = table
            .columns
            .iter()
            .filter(|column| self.columns.contains(&column.name))
            .map(|column| column.real_name.to_string())
            .collect();

        db.run(&format!(
            "
			CREATE INDEX CONCURRENTLY {name} ON {table} ({columns})
			",
            name = self.name,
            table = self.table,
            columns = column_real_names.join(", "),
        ))
        .context("failed to create index")?;
        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        _db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        db.run(&format!(
            "
			DROP INDEX {name} ON {table}
			",
            name = self.name,
            table = self.table,
        ))
        .context("failed to drop index")?;
        Ok(())
    }
}
