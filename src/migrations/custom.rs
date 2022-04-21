use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Custom {
    #[serde(default)]
    pub start: Option<String>,

    #[serde(default)]
    pub complete: Option<String>,

    #[serde(default)]
    pub abort: Option<String>,
}

#[typetag::serde(name = "custom")]
impl Action for Custom {
    fn describe(&self) -> String {
        "Running custom migration".to_string()
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        _schema: &Schema,
    ) -> anyhow::Result<()> {
        if let Some(start_query) = &self.start {
            println!("Running query: {}", start_query);
            db.run(start_query)?;
        }

        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        if let Some(complete_query) = &self.complete {
            db.run(complete_query)?;
        }

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        if let Some(abort_query) = &self.abort {
            db.run(abort_query)?;
        }

        Ok(())
    }
}
