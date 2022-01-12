use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveIndex {
    pub index: String,
}

#[typetag::serde(name = "remove_index")]
impl Action for RemoveIndex {
    fn describe(&self) -> String {
        format!("Removing index \"{}\"", self.index)
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        _db: &mut dyn Conn,
        _schema: &Schema,
    ) -> anyhow::Result<()> {
        // Do nothing, the index isn't removed until completion
        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        db.run(&format!(
            "
            DROP INDEX CONCURRENTLY IF EXISTS {}
            ",
            self.index
        ))
        .context("failed to drop index")?;

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, _db: &mut dyn Conn) -> anyhow::Result<()> {
        Ok(())
    }
}
