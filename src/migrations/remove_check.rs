use super::{common, Action, MigrationContext, NameField};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveCheck {
    table: String,
    check: String,
}

#[typetag::serde(name = "remove_check")]
impl Action for RemoveCheck {
    fn describe(&self) -> String {
        format!(
            "Removing check constraint \"{}\" from table \"{}\"",
            self.check, self.table
        )
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        // Ensure check exists
        let table = schema.get_table(db, &self.table)?;
        if !common::check_constraint_exists(db, &table.real_name, &self.check)? {
            return Err(anyhow!(
                "no check constraint \"{}\" exists on table \"{}\"",
                self.check,
                self.table
            ));
        }

        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        db.run(&format!(
            r#"
            ALTER TABLE "{table}"
            DROP CONSTRAINT IF EXISTS "{check}"
            "#,
            table = self.table,
            check = self.check,
        ))
        .context("failed to remove check constraint")?;
        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, schema: &mut Schema) {
        let check = self.check.clone();
        schema.change_table(&self.table, |table_changes| {
            table_changes.set_check_removed(&check);
        });
    }

    fn abort(&self, _ctx: &MigrationContext, _db: &mut dyn Conn) -> anyhow::Result<()> {
        Ok(())
    }

    fn name_fields(&self) -> Vec<NameField> {
        vec![NameField::table("table", &self.table)]
    }
}
