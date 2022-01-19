use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveEnum {
    #[serde(rename = "enum")]
    pub enum_name: String,
}

#[typetag::serde(name = "remove_enum")]
impl Action for RemoveEnum {
    fn describe(&self) -> String {
        format!("Removing enum \"{}\"", self.enum_name)
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
        db.run(&format!(
            r#"
            DROP TYPE IF EXISTS {name}
            "#,
            name = self.enum_name,
        ))
        .context("failed to drop enum")?;

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, _db: &mut dyn Conn) -> anyhow::Result<()> {
        Ok(())
    }
}
