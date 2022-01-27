use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveForeignKey {
    table: String,
    foreign_key: String,
}

#[typetag::serde(name = "remove_foreign_key")]
impl Action for RemoveForeignKey {
    fn describe(&self) -> String {
        format!(
            "Removing foreign key \"{}\" from table \"{}\"",
            self.foreign_key, self.table
        )
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        // The foreign key is only removed once the migration is completed.
        // Removing it earlier would be hard/undesirable for several reasons:
        // - Postgres doesn't have an easy way to temporarily disable a foreign key check.
        //   If it did, we could disable the FK for the new schema.
        // - Even if we could, it probably wouldn't be a good idea as it would cause temporary
        //   inconsistencies for the old schema which still expects the FK to hold.
        // - For the same reason, we can't remove the FK when the migration is first applied.
        //   If the migration was to be aborted, then the FK would have to be recreated with
        //   the risk that it would no longer be valid.

        // Ensure foreign key exists
        let table = schema.get_table(db, &self.table)?;
        let fk_exists = !db
            .query(&format!(
                r#"
                SELECT constraint_name
                FROM information_schema.table_constraints
                WHERE
                    constraint_type = 'FOREIGN KEY' AND
                    table_name = '{table_name}' AND
                    constraint_name = '{foreign_key}' 
                "#,
                table_name = table.real_name,
                foreign_key = self.foreign_key,
            ))
            .context("failed to check for foreign key")?
            .is_empty();

        if !fk_exists {
            return Err(anyhow!(
                "no foreign key \"{}\" exists on table \"{}\"",
                self.foreign_key,
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
            ALTER TABLE {table}
            DROP CONSTRAINT IF EXISTS {foreign_key}
            "#,
            table = self.table,
            foreign_key = self.foreign_key,
        ))
        .context("failed to remove foreign key")?;
        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, _db: &mut dyn Conn) -> anyhow::Result<()> {
        Ok(())
    }
}
