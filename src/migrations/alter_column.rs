use super::{Action, Context};
use crate::{db::Conn, migrations::common, schema::Schema};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AlterColumn {
    pub table: String,
    pub column: String,
    pub up: Option<String>,
    pub down: Option<String>,
    pub changes: ColumnChanges,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ColumnChanges {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub data_type: Option<String>,
    pub nullable: Option<bool>,
}

impl AlterColumn {
    fn temporary_column_name(&self) -> String {
        format!("__new__{}", self.column)
    }

    fn insert_trigger_name(&self, ctx: &Context) -> String {
        format!(
            "{}_alter_column_insert_trigger_{}_{}",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn update_old_trigger_name(&self, ctx: &Context) -> String {
        format!(
            "{}_alter_column_update_old_trigger_{}_{}",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn update_new_trigger_name(&self, ctx: &Context) -> String {
        format!(
            "{}_alter_column_update_new_trigger_{}_{}",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn not_null_constraint_name(&self, ctx: &Context) -> String {
        format!(
            "{}_alter_column_temporary_not_null_{}_{}",
            ctx.prefix(),
            self.table,
            self.column
        )
    }

    fn can_short_circuit(&self) -> bool {
        self.changes.name.is_some()
            && self.changes.data_type.is_none()
            && self.changes.nullable.is_none()
    }
}

#[typetag::serde(name = "uppercase_column")]
impl Action for AlterColumn {
    fn describe(&self) -> String {
        format!("Altering column \"{}\" on \"{}\"", self.column, self.table)
    }

    fn run(&self, ctx: &Context, db: &mut dyn Conn, schema: &Schema) -> anyhow::Result<()> {
        let table = schema.find_table(&self.table)?;
        let column = table.find_column(&self.column)?;

        // If we are only changing the name of a column, we don't have to do anything at this stage
        // We'll set the new schema to point to the old column. When the migration is completed,
        // we rename the actual column.
        if self.can_short_circuit() {
            return Ok(());
        }

        // If we couldn't short circuit, then we need up and down functions
        let (up, down) = match (&self.up, &self.down) {
            (Some(up), Some(down)) => (up, down),
            _ => return Err(anyhow!("missing up or down values")),
        };

        let temporary_column_type = self.changes.data_type.as_ref().unwrap_or(&column.data_type);

        // Add temporary, nullable column
        let query = format!(
            "
			ALTER TABLE {table}
            ADD COLUMN IF NOT EXISTS {temp_column} {temp_column_type};
			",
            table = self.table,
            temp_column = self.temporary_column_name(),
            temp_column_type = temporary_column_type,
        );
        db.run(&query)?;

        // Add temporary is new column
        db.run(&common::add_is_new_column_query(&self.table))?;

        // Add triggers to fill in values as they are inserted/updated
        let query = format!(
            "
            CREATE OR REPLACE FUNCTION {insert_trigger}()
            RETURNS TRIGGER AS $$
            BEGIN
                IF NEW.__reshape_is_new THEN
                    DECLARE
                        {existing_column} public.{table}.{temp_column}%TYPE := NEW.{temp_column};
                    BEGIN
                        {existing_column} = NEW.{temp_column};
                        NEW.{existing_column} = {down};
                    END;
                ELSIF NOT NEW.__reshape_is_new THEN
                    DECLARE
                        {existing_column} public.{table}.{existing_column}%TYPE := NEW.{existing_column};
                    BEGIN
                        {existing_column} = NEW.{existing_column};
                        NEW.{temp_column} = {up};
                    END;
                END IF;
                RETURN NEW;
            END
            $$ language 'plpgsql';

            DROP TRIGGER IF EXISTS {insert_trigger} ON {table};
            CREATE TRIGGER {insert_trigger} BEFORE INSERT ON {table} FOR EACH ROW EXECUTE PROCEDURE {insert_trigger}();


            CREATE OR REPLACE FUNCTION {update_old_trigger}()
            RETURNS TRIGGER AS $$
            BEGIN
                NEW.{temp_column} = UPPER(NEW.{existing_column});
                RETURN NEW;
            END
            $$ language 'plpgsql';

            DROP TRIGGER IF EXISTS {update_old_trigger} ON {table};
            CREATE TRIGGER {update_old_trigger} BEFORE UPDATE OF {existing_column} ON {table} FOR EACH ROW EXECUTE PROCEDURE {update_old_trigger}();


            CREATE OR REPLACE FUNCTION {update_new_trigger}()
            RETURNS TRIGGER AS $$
            BEGIN
                NEW.{existing_column} = LOWER(NEW.{temp_column});
                RETURN NEW;
            END
            $$ language 'plpgsql';

            DROP TRIGGER IF EXISTS {update_new_trigger} ON {table};
            CREATE TRIGGER {update_new_trigger} BEFORE UPDATE OF {temp_column} ON {table} FOR EACH ROW EXECUTE PROCEDURE {update_new_trigger}();
            ",
            existing_column = column.real_name(),
            temp_column = self.temporary_column_name(),
            up = up,
            down = down,
            table = self.table,
            insert_trigger = self.insert_trigger_name(ctx),
            update_old_trigger = self.update_old_trigger_name(ctx),
            update_new_trigger = self.update_new_trigger_name(ctx),
        );
        db.run(&query)?;

        // Add a temporary NOT NULL constraint if the column shouldn't be nullable.
        // This constraint is set as NOT VALID so it doesn't apply to existing rows and
        // the existing rows don't need to be scanned under an exclusive lock.
        // Thanks to this, we can set the full column as NOT NULL later with minimal locking.
        if !column.nullable {
            let query = format!(
                "
                ALTER TABLE {table}
                ADD CONSTRAINT {constraint_name}
                CHECK ({column} IS NOT NULL) NOT VALID
                ",
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
                column = self.temporary_column_name(),
            );
            db.run(&query)?;
        }

        // Backfill values in batches
        common::batch_update(db, table, &self.temporary_column_name(), up)?;

        Ok(())
    }

    fn complete(&self, ctx: &Context, db: &mut dyn Conn, schema: &Schema) -> anyhow::Result<()> {
        if self.can_short_circuit() {
            if let Some(new_name) = &self.changes.name {
                let query = format!(
                    "
			        ALTER TABLE {table}
			        RENAME COLUMN {existing_name} TO {new_name}
			        ",
                    table = self.table,
                    existing_name = self.column,
                    new_name = new_name,
                );
                db.run(&query)?;
            }
            return Ok(());
        }

        let column = schema
            .find_table(&self.table)
            .and_then(|table| table.find_column(&self.column))?;
        let column_name = self.changes.name.as_deref().unwrap_or(column.real_name());

        // Remove old column
        let query = format!(
            "
            ALTER TABLE {} DROP COLUMN {} CASCADE
			",
            self.table,
            column.real_name()
        );
        db.run(&query)?;

        // Rename temporary column
        let query = format!(
            "
            ALTER TABLE {table} RENAME COLUMN {temp_column} TO {name}
			",
            table = self.table,
            temp_column = self.temporary_column_name(),
            name = column_name,
        );
        db.run(&query)?;

        // Remove triggers and procedures
        let query = format!(
            "
            DROP TRIGGER IF EXISTS {insert_trigger} ON {table};
            DROP FUNCTION IF EXISTS {insert_trigger};

            DROP TRIGGER IF EXISTS {update_old_trigger} ON {table};
            DROP FUNCTION IF EXISTS {update_old_trigger};

            DROP TRIGGER IF EXISTS {update_new_trigger} ON {table};
            DROP FUNCTION IF EXISTS {update_new_trigger};
            ",
            table = self.table,
            insert_trigger = self.insert_trigger_name(ctx),
            update_old_trigger = self.update_old_trigger_name(ctx),
            update_new_trigger = self.update_new_trigger_name(ctx),
        );
        db.run(&query)?;

        // Update column to be NOT NULL if necessary
        if !column.nullable {
            // Validate the temporary constraint (should always be valid).
            // This performs a sequential scan but does not take an exclusive lock.
            let query = format!(
                "
                ALTER TABLE {table}
                VALIDATE CONSTRAINT {constraint_name}
                ",
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
            );
            db.run(&query)?;

            // Update the column to be NOT NULL.
            // This requires an exclusive lock but since PG 12 it can check
            // the existing constraint for correctness which makes the lock short-lived.
            // Source: https://dba.stackexchange.com/a/268128
            let query = format!(
                "
                ALTER TABLE {table}
                ALTER COLUMN {column} SET NOT NULL
                ",
                table = self.table,
                column = column_name,
            );
            db.run(&query)?;

            // Drop the temporary constraint
            let query = format!(
                "
                ALTER TABLE {table}
                DROP CONSTRAINT {constraint_name}
                ",
                table = self.table,
                constraint_name = self.not_null_constraint_name(ctx),
            );
            db.run(&query)?;
        }

        Ok(())
    }

    fn update_schema(&self, schema: &mut Schema) -> anyhow::Result<()> {
        let table = schema.find_table_mut(&self.table)?;
        let column = table.find_column_mut(&self.column)?;

        // If we are only changing the name of a column, we haven't created a temporary column
        // Instead we rename the schema column but point it to the old column
        if self.can_short_circuit() {
            if let Some(new_name) = &self.changes.name {
                column.real_name = Some(column.real_name().to_string());
                column.name = new_name.to_string();
            }

            return Ok(());
        }

        column.name = self
            .changes
            .name
            .as_ref()
            .map(|n| n.to_string())
            .unwrap_or(self.column.to_string());
        column.real_name = Some(format!("__new__{}", self.column));
        table.has_is_new = true;

        Ok(())
    }

    fn abort(&self, ctx: &Context, db: &mut dyn Conn) -> anyhow::Result<()> {
        // Remove triggers and procedures
        let query = format!(
            "
            DROP TRIGGER IF EXISTS {insert_trigger} ON {table};
            DROP FUNCTION IF EXISTS {insert_trigger};

            DROP TRIGGER IF EXISTS {update_old_trigger} ON {table};
            DROP FUNCTION IF EXISTS {update_old_trigger};

            DROP TRIGGER IF EXISTS {update_new_trigger} ON {table};
            DROP FUNCTION IF EXISTS {update_new_trigger};
            ",
            table = self.table,
            insert_trigger = self.insert_trigger_name(ctx),
            update_old_trigger = self.update_old_trigger_name(ctx),
            update_new_trigger = self.update_new_trigger_name(ctx),
        );
        db.run(&query)?;

        // Drop temporary column
        let query = format!(
            "
			ALTER TABLE {table}
            DROP COLUMN IF EXISTS {temp_column};
			",
            table = self.table,
            temp_column = self.temporary_column_name(),
        );
        db.run(&query)?;

        // Drop temporary "is new" column
        db.run(&common::drop_is_new_column_query(&self.table))?;

        Ok(())
    }
}
