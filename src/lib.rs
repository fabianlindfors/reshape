use crate::migrations::{Context, Migration};
use colored::*;
use db::{Conn, DbConn};
use postgres::Config;
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
        let config: Config = connection_string.parse()?;
        Self::new_with_config(&config)
    }

    pub fn new_with_options(
        host: &str,
        port: u16,
        username: &str,
        password: &str,
    ) -> anyhow::Result<Reshape> {
        let mut config = Config::new();
        config
            .host(host)
            .port(port)
            .user(username)
            .password(password);

        Self::new_with_config(&config)
    }

    fn new_with_config(config: &Config) -> anyhow::Result<Reshape> {
        let mut db = DbConn::connect(config)?;
        let state = State::load(&mut db);

        Ok(Reshape { db, state })
    }

    pub fn migrate<T>(&mut self, migrations: T) -> anyhow::Result<()>
    where
        T: IntoIterator<Item = Migration>,
    {
        // Make sure no migration is in progress
        if let state::Status::InProgress {
            migrations: _,
            target_schema: _,
        } = &self.state.status
        {
            println!("Migration already in progress, please complete using 'reshape complete'");
            return Ok(());
        }

        let current_migration = &self.state.current_migration.clone();
        let remaining_migrations = self.state.get_remaining_migrations(migrations)?;
        if remaining_migrations.is_empty() {
            println!("No migrations left to apply");
            return Ok(());
        }

        println!(" Applying {} migrations\n", remaining_migrations.len());

        let target_migration = remaining_migrations.last().unwrap().name.to_string();

        let mut new_schema = self.state.current_schema.clone();

        for (migration_index, migration) in remaining_migrations.iter().enumerate() {
            println!("Migrating '{}':", migration.name);

            for (action_index, action) in migration.actions.iter().enumerate() {
                print!("  + {} ", action.describe());

                let ctx = Context::new(migration_index, action_index);
                action.run(&ctx, &mut self.db, &new_schema)?;
                action.update_schema(&mut new_schema)?;

                println!("{}", "done".green());
            }

            println!("");
        }

        // Create schema and views for migration
        self.create_schema_for_migration(&target_migration, &new_schema)?;

        // Update state once migrations have been performed
        self.state.in_progress(remaining_migrations, new_schema);
        self.state.save(&mut self.db)?;

        // If we started from a blank slate, we can finish the migration immediately
        if current_migration.is_none() {
            println!("Automatically completing migrations\n");
            self.complete_migration()?;

            println!("Migrations complete:");
            println!(
                "  - Run '{}' from your application to use the latest schema",
                schema_query_for_migration(&target_migration)
            );
        } else {
            println!("Migrations have been applied and the new schema is ready for use:");
            println!(
                "  - Run '{}' from your application to use the latest schema",
                schema_query_for_migration(&target_migration)
            );
            println!(
                "  - Run 'reshape complete' once your application has been updated and the previous schema is no longer in use"
            );
        }

        Ok(())
    }

    pub fn complete_migration(&mut self) -> anyhow::Result<()> {
        // Make sure a migration is in progress
        let remaining_migrations = match &self.state.status {
            state::Status::InProgress {
                migrations,
                target_schema: _,
            } => migrations,
            _ => {
                println!("No migration in progress");
                return Ok(());
            }
        };

        let target_migration = remaining_migrations.last().unwrap().name.to_string();

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

        for (migration_index, migration) in remaining_migrations.iter().enumerate() {
            println!("Completing '{}':", migration.name);

            for (action_index, action) in migration.actions.iter().enumerate() {
                print!("  + {} ", action.describe());

                let ctx = Context::new(migration_index, action_index);
                action.complete(&ctx, &mut transaction, &temp_schema)?;
                action.update_schema(&mut temp_schema)?;

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
                let schema = schema_name_for_migration(&target_migration);
                Self::create_view_for_table(&mut transaction, table, &schema, false)?;
            }
        }

        self.state.complete()?;
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

    pub fn remove(&mut self) -> anyhow::Result<()> {
        // Remove migration schemas and views
        if let Some(current_migration) = &self.state.current_migration {
            self.db.run(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                schema_name_for_migration(current_migration)
            ))?;
        }

        if let Status::InProgress {
            migrations,
            target_schema: _,
        } = &self.state.status
        {
            let target_migration = migrations.last().unwrap().name.to_string();
            self.db.run(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                schema_name_for_migration(&target_migration)
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

    pub fn abort(&mut self) -> anyhow::Result<()> {
        let remaining_migrations = match &self.state.status {
            Status::InProgress {
                migrations,
                target_schema: _,
            } => migrations,
            _ => {
                println!("No migration is in progress");
                return Ok(());
            }
        };
        let target_migration = remaining_migrations.last().unwrap().name.to_string();

        // Run all the abort changes as a transaction to avoid incomplete changes
        let mut transaction = self.db.transaction()?;

        // Remove new migration's schema
        transaction.run(&format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            schema_name_for_migration(&target_migration)
        ))?;

        // Abort all pending migrations in reverse order
        for (migration_index, migration) in remaining_migrations.iter().rev().enumerate() {
            print!("Aborting'{}' ", migration.name);
            for (action_index, action) in migration.actions.iter().rev().enumerate() {
                let ctx = Context::new(migration_index, action_index);
                action.abort(&ctx, &mut transaction)?;
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

pub fn latest_schema_from_migrations(migrations: &[Migration]) -> Option<String> {
    migrations
        .last()
        .map(|migration| schema_name_for_migration(&migration.name))
}

pub fn schema_query_for_migration(migration_name: &str) -> String {
    let schema_name = schema_name_for_migration(migration_name);
    format!("SET search_path TO {}", schema_name)
}

fn schema_name_for_migration(migration_name: &str) -> String {
    format!("migration_{}", migration_name)
}
