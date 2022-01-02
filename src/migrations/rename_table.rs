use super::{Action, MigrationContext};
use crate::{db::Conn, schema::Schema};
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

    fn complete(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        _schema: &Schema,
    ) -> anyhow::Result<()> {
        // Rename table
        let query = format!(
            "
            ALTER TABLE {table}
            RENAME TO {new_name}
            ",
            table = self.table,
            new_name = self.new_name,
        );
        db.run(&query)?;

        Ok(())
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
