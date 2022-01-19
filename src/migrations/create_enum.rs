use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateEnum {
    pub name: String,
    pub values: Vec<String>,
}

#[typetag::serde(name = "create_enum")]
impl Action for CreateEnum {
    fn describe(&self) -> String {
        format!("Creating enum \"{}\"", self.name)
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        _schema: &Schema,
    ) -> anyhow::Result<()> {
        // Check if enum already exists. CREATE TYPE doesn't have
        // a IF NOT EXISTS option so we have to do it manually.
        let enum_exists = !db
            .query(&format!(
                "
                SELECT typname
                FROM pg_catalog.pg_type
                WHERE typcategory = 'E'
                AND typname = '{name}'
                ",
                name = self.name,
            ))?
            .is_empty();
        if enum_exists {
            return Ok(());
        }

        let values_def: Vec<String> = self
            .values
            .iter()
            .map(|value| format!("'{}'", value))
            .collect();

        db.run(&format!(
            r#"
            CREATE TYPE "{name}" AS ENUM ({values})
            "#,
            name = self.name,
            values = values_def.join(", "),
        ))
        .context("failed to create enum")?;

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
            r#"
            DROP TYPE IF EXISTS {name}
            "#,
            name = self.name,
        ))
        .context("failed to drop enum")?;

        Ok(())
    }
}
