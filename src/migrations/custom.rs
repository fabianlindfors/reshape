use super::{validate_sql_statement, Action, MigrationContext};
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

    fn validate_sql(&self) -> Vec<(String, String, String)> {
        let mut errors = vec![];

        if let Some(start) = &self.start {
            if let Err(e) = validate_sql_statement(start) {
                errors.push(("start".to_string(), start.clone(), e));
            }
        }

        if let Some(complete) = &self.complete {
            if let Err(e) = validate_sql_statement(complete) {
                errors.push(("complete".to_string(), complete.clone(), e));
            }
        }

        if let Some(abort) = &self.abort {
            if let Err(e) = validate_sql_statement(abort) {
                errors.push(("abort".to_string(), abort.clone(), e));
            }
        }

        errors
    }
}
