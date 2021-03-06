use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveTable {
    pub table: String,
}

#[typetag::serde(name = "remove_table")]
impl Action for RemoveTable {
    fn describe(&self) -> String {
        format!("Removing table \"{}\"", self.table)
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        _db: &mut dyn Conn,
        _schema: &Schema,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        // Remove table
        let query = format!(
            r#"
            DROP TABLE IF EXISTS "{table}";
            "#,
            table = self.table,
        );
        db.run(&query).context("failed to drop table")?;

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, schema: &mut Schema) {
        schema.change_table(&self.table, |table_changes| {
            table_changes.set_removed();
        });
    }

    fn abort(&self, _ctx: &MigrationContext, _db: &mut dyn Conn) -> anyhow::Result<()> {
        Ok(())
    }
}
