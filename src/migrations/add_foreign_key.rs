use super::{common::ForeignKey, Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddForeignKey {
    pub table: String,
    foreign_key: ForeignKey,
}

#[typetag::serde(name = "add_foreign_key")]
impl Action for AddForeignKey {
    fn describe(&self) -> String {
        format!(
            "Adding foreign key from table \"{}\" to \"{}\"",
            self.table, self.foreign_key.referenced_table
        )
    }

    fn run(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;
        let referenced_table = schema.get_table(db, &self.foreign_key.referenced_table)?;

        // Add quotes around all column names
        let columns: Vec<String> = table
            .real_column_names(&self.foreign_key.columns)
            .map(|col| format!("\"{}\"", col))
            .collect();
        let referenced_columns: Vec<String> = referenced_table
            .real_column_names(&self.foreign_key.referenced_columns)
            .map(|col| format!("\"{}\"", col))
            .collect();

        // Create foreign key but set is as NOT VALID.
        // This means the foreign key will be enforced for inserts and updates
        // but the existing data won't be checked, that would cause a long-lived lock.
        db.run(&format!(
            r#"
            ALTER TABLE "{table}"
            ADD CONSTRAINT {constraint_name}
            FOREIGN KEY ({columns})
            REFERENCES "{referenced_table}" ({referenced_columns})
            NOT VALID
            "#,
            table = table.real_name,
            constraint_name = self.temp_constraint_name(ctx),
            columns = columns.join(", "),
            referenced_table = referenced_table.real_name,
            referenced_columns = referenced_columns.join(", "),
        ))
        .context("failed to create foreign key")?;

        db.run(&format!(
            r#"
            ALTER TABLE "{table}"
            VALIDATE CONSTRAINT "{constraint_name}"
            "#,
            table = table.real_name,
            constraint_name = self.temp_constraint_name(ctx),
        ))
        .context("failed to validate foreign key")?;

        Ok(())
    }

    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        db.run(&format!(
            r#"
            ALTER TABLE {table}
            RENAME CONSTRAINT {temp_constraint_name} TO {constraint_name}
            "#,
            table = self.table,
            temp_constraint_name = self.temp_constraint_name(ctx),
            constraint_name = self.final_constraint_name(),
        ))
        .context("failed to rename temporary constraint")?;
        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        db.run(&format!(
            r#"
            ALTER TABLE "{table}"
            DROP CONSTRAINT IF EXISTS "{constraint_name}"
            "#,
            table = self.table,
            constraint_name = self.temp_constraint_name(ctx),
        ))
        .context("failed to validate foreign key")?;

        Ok(())
    }
}

impl AddForeignKey {
    fn temp_constraint_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_temp_fkey", ctx.prefix())
    }

    fn final_constraint_name(&self) -> String {
        format!(
            "{table}_{columns}_fkey",
            table = self.table,
            columns = self.foreign_key.columns.join("_")
        )
    }
}
