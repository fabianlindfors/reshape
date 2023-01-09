use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddIndex {
    pub table: String,
    pub index: Index,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Index {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default = "bool_true")]
    pub concurrently: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(rename = "type")]
    pub index_type: Option<String>,
}

#[typetag::serde(name = "add_index")]
impl Action for AddIndex {
    fn describe(&self) -> String {
        format!(
            "Adding index \"{}\" to table \"{}\"",
            self.index.name, self.table
        )
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;

        let column_real_names: Vec<String> = table
            .columns
            .iter()
            .filter(|column| self.index.columns.contains(&column.name))
            .map(|column| format!("\"{}\"", column.real_name))
            .collect();

        let concurrently = if self.index.concurrently { "CONCURRENTLY" } else { "" };
        let unique = if self.index.unique { "UNIQUE" } else { "" };
        let index_type_def = if let Some(index_type) = &self.index.index_type {
            format!("USING {index_type}")
        } else {
            "".to_string()
        };

        db.run(&format!(
            r#"
			CREATE {unique} INDEX {concurrently} "{name}" ON "{table}" {index_type_def} ({columns})
			"#,
            name = self.index.name,
            table = self.table,
            columns = column_real_names.join(", "),
        ))
        .context("failed to create index")?;
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
        let concurrently = if self.index.concurrently { "CONCURRENTLY" } else { "" };
        db.run(&format!(
            r#"
			DROP INDEX {concurrently} IF EXISTS "{name}"
			"#,
            name = self.index.name,
        ))
        .context("failed to drop index")?;
        Ok(())
    }
}

// A workaround for serde being unable to default to primitive true.
// See https://github.com/serde-rs/serde/issues/368
fn bool_true() -> bool {
    return true
}
