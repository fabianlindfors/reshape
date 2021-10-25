use crate::migrations::Migration;
use colored::*;
use db::{Conn, DbConn};
use schema::Table;

mod db;
pub mod migrations;
mod schema;
mod state;

pub use crate::state::{State, Status};

pub struct Reshape {
    pub state: State,
    db: DbConn,
}

impl Reshape {
    pub fn new(connection_string: &str) -> anyhow::Result<Reshape> {
        let mut db = DbConn::connect(connection_string)?;
        let state = State::load(&mut db);

        Ok(Reshape { db, state })
    }

    pub fn migrate<T>(&mut self, migrations: T) -> anyhow::Result<()>
    where
        T: IntoIterator<Item = Migration>,
    {
        self.state.set_migrations(migrations)?;
        self.state.save(&mut self.db)?;

        // Make sure no migration is in progress
        if let state::Status::InProgress {
            target_migration: _,
            target_schema: _,
        } = &self.state.status
        {
            println!("Migration already in progress, please complete using 'reshape complete'");
            return Ok(());
        }

        let current_migration = &self.state.current_migration.clone();
        let remaining_migrations = Self::get_remaining_migrations(&self.state);
        if remaining_migrations.is_empty() {
            println!("No migrations left to apply");
            return Ok(());
        }

        println!(" Applying {} migrations\n", remaining_migrations.len());

        let target_migration = remaining_migrations.last().unwrap().name.to_string();

        let mut new_schema = self.state.current_schema.clone();

        for migration in remaining_migrations {
            println!("Migrating '{}':", migration.name);

            for step in &migration.actions {
                print!("  + {} ", step.describe());
                step.run(&mut self.db, &new_schema)?;
                step.update_schema(&mut new_schema)?;
                println!("{}", "done".green());
            }

            println!("");
        }

        // Create schema and views for migration
        self.create_schema_for_migration(&target_migration, &new_schema)?;

        // Update state once migrations have been performed
        self.state.status = state::Status::InProgress {
            target_migration: target_migration.to_string(),
            target_schema: new_schema,
        };
        self.state.save(&mut self.db)?;

        // If we started from a blank slate, we can finish the migration immediately
        if current_migration.is_none() {
            println!("Automatically completing migrations\n");
            self.complete_migration()?;

            println!("Migrations complete:");
            println!(
                "  - Run '{}' from your application to use the latest schema",
                generate_schema_query(&target_migration)
            );
        } else {
            println!("Migrations have been applied and the new schema is ready for use:");
            println!(
                "  - Run '{}' from your application to use the latest schema",
                generate_schema_query(&target_migration)
            );
            println!(
                "  - Run 'reshape complete' once your application has been updated and the previous schema is no longer in use"
            );
        }

        Ok(())
    }

    pub fn complete_migration(&mut self) -> anyhow::Result<()> {
        // Make sure a migration is in progress
        let (target_migration, target_schema) = match &self.state.status {
            state::Status::InProgress {
                target_migration,
                target_schema,
            } => (target_migration, target_schema),
            _ => {
                println!("No migration in progress");
                return Ok(());
            }
        };

        let remaining_migrations = Self::get_remaining_migrations(&self.state);
        let mut temp_schema = self.state.current_schema.clone();

        // Run all the completion changes as a transaction to avoid incomplete updates
        let mut transaction = self.db.transaction()?;

        // Remove previous migration's schema
        if let Some(current_migration) = &self.state.current_migration {
            transaction.run(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                schema_name_for_migration(current_migration)
            ))?;
        }

        for migration in remaining_migrations {
            println!("Completing '{}':", migration.name);

            for step in &migration.actions {
                print!("  + {} ", step.describe());
                step.complete(&mut transaction, &temp_schema)?;
                step.update_schema(&mut temp_schema)?;
                println!("{}", "done".green());
            }

            println!("");
        }

        // Remove any temporary is new columns from tables
        for table in temp_schema.tables.values_mut() {
            if table.has_is_new {
                table.has_is_new = false;

                transaction.run(&format!(
                    "ALTER TABLE {table} DROP COLUMN __reshape_is_new CASCADE",
                    table = table.name,
                ))?;

                // The view will automatically be dropped by CASCADE so let's recreate it
                let schema = schema_name_for_migration(target_migration);
                Self::create_view_for_table(&mut transaction, table, &schema, false)?;
            }
        }

        self.state.current_migration = Some(target_migration.to_string());
        self.state.current_schema = target_schema.clone();
        self.state.status = state::Status::Idle;
        self.state.save(&mut transaction)?;

        transaction.commit()?;

        Ok(())
    }

    fn create_schema_for_migration(
        &mut self,
        migration_name: &str,
        schema: &schema::Schema,
    ) -> anyhow::Result<()> {
        // Create schema for migration
        let schema_name = schema_name_for_migration(migration_name);
        self.db
            .run(&format!("CREATE SCHEMA IF NOT EXISTS {}", schema_name))?;

        // Create views inside schema
        for table in schema.tables.values() {
            Self::create_view_for_table(&mut self.db, table, &schema_name, true)?;
        }

        Ok(())
    }

    fn create_view_for_table(
        db: &mut impl Conn,
        table: &Table,
        schema: &str,
        use_alias: bool,
    ) -> anyhow::Result<()> {
        let mut select_columns: Vec<String> = table
            .columns
            .iter()
            .map(|column| {
                if use_alias {
                    format!("{} AS {}", column.real_name(), column.name)
                } else {
                    column.name.to_string()
                }
            })
            .collect();

        if table.has_is_new {
            select_columns.push("__reshape_is_new".to_string());
        }

        db.run(&format!(
            "CREATE OR REPLACE VIEW {schema}.{table} AS
                SELECT {columns}
                FROM {table}",
            schema = schema,
            table = table.name,
            columns = select_columns.join(","),
        ))?;

        if table.has_is_new {
            db.run(&format!(
                "ALTER VIEW {schema}.{view} ALTER __reshape_is_new SET DEFAULT TRUE",
                schema = schema,
                view = table.name,
            ))?;
        }

        Ok(())
    }

    fn get_remaining_migrations(state: &State) -> Vec<&Migration> {
        match &state.current_migration {
            Some(current_migration) => state
                .migrations
                .iter()
                .skip_while(|migration| &migration.name != current_migration)
                .skip(1)
                .collect(),
            None => state.migrations.iter().collect(),
        }
    }

    pub fn remove(&mut self) -> anyhow::Result<()> {
        // Remove migration schemas and views
        if let Some(current_migration) = &self.state.current_migration {
            self.db.run(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                schema_name_for_migration(current_migration)
            ))?;
        }

        if let Status::InProgress {
            target_migration,
            target_schema: _,
        } = &self.state.status
        {
            self.db.run(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                schema_name_for_migration(target_migration)
            ))?;
        }

        // Remove all tables
        let schema = &self.state.current_schema;
        for table in schema.tables.values() {
            self.db
                .run(&format!("DROP TABLE IF EXISTS {} CASCADE", table.name))?;
        }

        // Reset state
        self.state.clear(&mut self.db)?;

        println!("Reshape and all data has been removed");

        Ok(())
    }

    pub fn latest_schema(&self) -> Option<String> {
        self.state
            .migrations
            .last()
            .map(|migration| schema_name_for_migration(&migration.name))
    }

    pub fn abort(&mut self) -> anyhow::Result<()> {
        let target_migration = match &self.state.status {
            Status::InProgress {
                target_migration,
                target_schema: _,
            } => target_migration,
            _ => {
                println!("No migration is in progress");
                return Ok(());
            }
        };

        let remaining_migrations = Self::get_remaining_migrations(&self.state);

        // Run all the abort changes as a transaction to avoid incomplete changes
        let mut transaction = self.db.transaction()?;

        // Remove new migration's schema
        transaction.run(&format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            schema_name_for_migration(target_migration)
        ))?;

        // Abort all pending migrations in reverse order
        for migration in remaining_migrations.iter().rev() {
            print!("Aborting'{}' ", migration.name);
            for action in migration.actions.iter().rev() {
                action.abort(&mut transaction)?;
            }
            println!("{}", "done".green());
        }

        let keep_count = self.state.migrations.len() - remaining_migrations.len();
        self.state.migrations.truncate(keep_count);
        self.state.status = state::Status::Idle;
        self.state.save(&mut transaction)?;

        transaction.commit()?;

        Ok(())
    }
}

pub fn generate_schema_query(migration_name: &str) -> String {
    let schema_name = schema_name_for_migration(migration_name);
    format!("SET search_path TO {}", schema_name)
}

fn schema_name_for_migration(migration_name: &str) -> String {
    format!("migration_{}", migration_name)
}
