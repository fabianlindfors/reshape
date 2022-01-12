use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RenameTable {
    pub table: String,
    pub new_name: String,
}

#[typetag::serde(name = "rename_table")]
impl Action for RenameTable {
    fn describe(&self) -> String {
        format!("Renaming table \"{}\" to \"{}\"", self.table, self.new_name)
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
        // Rename table
        let query = format!(
            r#"
            ALTER TABLE IF EXISTS "{table}"
            RENAME TO "{new_name}"
            "#,
            table = self.table,
            new_name = self.new_name,
        );
        db.run(&query).context("failed to rename table")?;

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, schema: &mut Schema) {
        schema.change_table(&self.table, |table_changes| {
            table_changes.set_name(&self.new_name);
        });
    }

    fn abort(&self, _ctx: &MigrationContext, _db: &mut dyn Conn) -> anyhow::Result<()> {
        Ok(())
    }
}
