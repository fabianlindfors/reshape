use super::{common, Action, MigrationContext, NameField, References, SqlField};
use crate::{
    db::{Conn, Transaction},
    schema::Schema,
    sql::rewrite_column_references,
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddCheck {
    pub table: String,
    check: CheckDefinition,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CheckDefinition {
    pub name: String,
    pub expression: String,
}

// The number of violating rows to include in an error
const VIOLATION_EXAMPLES: usize = 5;

#[typetag::serde(name = "add_check")]
impl Action for AddCheck {
    fn describe(&self) -> String {
        format!(
            "Adding check constraint \"{}\" to table \"{}\"",
            self.check.name, self.table
        )
    }

    fn run(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;

        // The final name must be free by the time the migration completes. A constraint
        // with the same name may exist if the migration removes it, which makes it
        // possible to replace a check by removing and adding it in the same migration.
        if !schema.is_check_removed(&self.table, &self.check.name)
            && common::check_constraint_exists(db, &table.real_name, &self.check.name)?
        {
            return Err(anyhow!(
                "check constraint \"{}\" already exists on table \"{}\"",
                self.check.name,
                self.table
            ));
        }

        // The expression references columns by their current names, which may be backed
        // by differently named columns during the migration
        let expression = rewrite_column_references(&self.check.expression, &table)
            .context("failed to resolve columns referenced by check")?;

        let temp_constraint_name = self.temp_constraint_name(ctx);

        if !common::check_constraint_exists(db, &table.real_name, &temp_constraint_name)? {
            // Look for existing rows which violate the check before adding it. Once added,
            // the check is enforced on every write, including those of the old schema, so
            // a check which the existing data doesn't satisfy should fail before that
            // point. This is only an early exit: the scan doesn't block the application
            // but rows written after it are only covered by the validation below.
            let violations = common::find_check_violations(
                db,
                &table.real_name,
                &expression,
                VIOLATION_EXAMPLES,
            )
            .context("failed to check existing rows against check")?;
            if !violations.is_empty() {
                return Err(self.violation_error(&violations));
            }

            // Create the check but set it as NOT VALID. This means the check is enforced
            // for inserts and updates but the existing data isn't checked, which would
            // require a long-lived lock.
            db.run(&format!(
                r#"
                ALTER TABLE "{table}"
                ADD CONSTRAINT "{constraint_name}"
                CHECK ({expression})
                NOT VALID
                "#,
                table = table.real_name,
                constraint_name = temp_constraint_name,
                expression = expression,
            ))
            .context("failed to create check constraint")?;
        }

        // Validating scans the table but doesn't block reads or writes. Together with the
        // NOT VALID constraint, which covers every row written since it was added, this
        // proves that every row satisfies the check.
        let validation = db.run(&format!(
            r#"
            ALTER TABLE "{table}"
            VALIDATE CONSTRAINT "{constraint_name}"
            "#,
            table = table.real_name,
            constraint_name = temp_constraint_name,
        ));

        if let Err(error) = validation {
            // Drop the check right away so it doesn't stay enforced on the old schema
            // until the migration is aborted
            self.drop_temp_constraint(ctx, db, &table.real_name)?;

            let violations = common::find_check_violations(
                db,
                &table.real_name,
                &expression,
                VIOLATION_EXAMPLES,
            )
            .unwrap_or_default();
            return Err(error.context(self.violation_error(&violations)));
        }

        Ok(())
    }

    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        let temp_constraint_name = self.temp_constraint_name(ctx);

        // The constraint has already been renamed if completion is being retried
        if common::check_constraint_exists(db, &self.table, &temp_constraint_name)? {
            db.run(&format!(
                r#"
                ALTER TABLE "{table}"
                RENAME CONSTRAINT "{temp_constraint_name}" TO "{constraint_name}"
                "#,
                table = self.table,
                temp_constraint_name = temp_constraint_name,
                constraint_name = self.check.name,
            ))
            .context("failed to rename temporary check constraint")?;
        }

        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, schema: &mut Schema) {
        let check_name = self.check.name.clone();
        schema.change_table(&self.table, |table_changes| {
            table_changes.set_check_added(&check_name);
        });
    }

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        self.drop_temp_constraint(ctx, db, &self.table)
    }

    fn sql_fields(&self) -> Vec<SqlField> {
        vec![SqlField::expression(
            "check.expression",
            &self.check.expression,
            References::table(&self.table),
        )]
    }

    fn name_fields(&self) -> Vec<NameField> {
        vec![NameField::table("table", &self.table)]
    }
}

impl AddCheck {
    fn temp_constraint_name(&self, ctx: &MigrationContext) -> String {
        format!("{}_temp_check", ctx.prefix())
    }

    fn drop_temp_constraint(
        &self,
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        table: &str,
    ) -> anyhow::Result<()> {
        db.run(&format!(
            r#"
            ALTER TABLE "{table}"
            DROP CONSTRAINT IF EXISTS "{constraint_name}"
            "#,
            table = table,
            constraint_name = self.temp_constraint_name(ctx),
        ))
        .context("failed to drop temporary check constraint")
    }

    fn violation_error(&self, violations: &[String]) -> anyhow::Error {
        if violations.is_empty() {
            anyhow!(
                "existing rows in table \"{}\" violate check \"{}\"",
                self.table,
                self.check.name
            )
        } else {
            anyhow!(
                "existing rows in table \"{}\" violate check \"{}\", for example rows with {}",
                self.table,
                self.check.name,
                violations.join("; ")
            )
        }
    }
}
