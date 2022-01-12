use crate::{
    migrations::{Migration, MigrationContext},
    schema::Schema,
};

use anyhow::{anyhow, Context};
use colored::*;
use db::{Conn, DbConn};
use postgres::Config;
use schema::Table;

mod db;
mod helpers;
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
        self.state = State::load(&mut self.db);

        // Make sure no migration is in progress
        if let state::Status::InProgress { .. } = &self.state.status {
            println!("Migration already in progress, please complete using 'reshape complete'");
            return Ok(());
        }

        if let state::Status::Completing { .. } = &self.state.status {
            println!(
                "Migration already in progress and has started completion, please finish using 'reshape complete'"
            );
            return Ok(());
        }

        // Determine which migrations need to be applied by comparing the provided migrations
        // with the already applied ones stored in the state. This will throw an error if the
        // two sets of migrations don't agree, for example if a new migration has been added
        // in between two existing ones.
        let current_migration = &self.state.current_migration.clone();
        let remaining_migrations = self.state.get_remaining_migrations(migrations)?;
        if remaining_migrations.is_empty() {
            println!("No migrations left to apply");
            return Ok(());
        }

        // If we have already started applying some migrations we need to ensure that
        // they are the same ones we want to apply now
        if let state::Status::Applying {
            migrations: existing_migrations,
        } = &self.state.status
        {
            if existing_migrations != &remaining_migrations {
                return Err(anyhow!(
                    "a previous migration seems to have failed without cleaning up. Please run `reshape abort` and then run migrate again."
                ));
            }
        }

        // Move to the "Applying" state which is necessary as we can't run the migrations
        // and state update as a single transaction. If a migration unexpectedly fails without
        // automatically aborting, this state saves us from dangling migrations. It forces the user
        // to either run migrate again (which works as all migrations are idempotent) or abort.
        self.state.applying(remaining_migrations.clone());
        self.state.save(&mut self.db)?;

        println!("Applying {} migrations\n", remaining_migrations.len());

        helpers::set_up_helpers(&mut self.db, current_migration)
            .context("failed to set up helpers")?;

        let mut new_schema = Schema::new();
        let mut last_migration_index = usize::MAX;
        let mut last_action_index = usize::MAX;
        let mut result: anyhow::Result<()> = Ok(());

        for (migration_index, migration) in remaining_migrations.iter().enumerate() {
            println!("Migrating '{}':", migration.name);
            last_migration_index = migration_index;

            for (action_index, action) in migration.actions.iter().enumerate() {
                last_action_index = action_index;

                let description = action.describe();
                print!("  + {} ", description);

                let ctx = MigrationContext::new(migration_index, action_index);
                result = action
                    .run(&ctx, &mut self.db, &new_schema)
                    .with_context(|| format!("failed to {}", description));

                if result.is_ok() {
                    action.update_schema(&ctx, &mut new_schema);
                    println!("{}", "done".green());
                } else {
                    println!("{}", "failed".red());
                    break;
                }
            }

            println!();
        }

        // If a migration failed, we abort all the migrations that were applied
        if let Err(err) = result {
            println!("A migration failed, aborting migrations that have already been applied");

            // Set to the Aborting state. This is to ensure that the failed
            // migration is fully aborted and nothing is left dangling.
            // If the abort is interrupted for any reason, the user can try again
            // by running `reshape abort`.
            self.state.aborting(
                remaining_migrations.clone(),
                last_migration_index + 1,
                last_action_index + 1,
            );

            // Abort will only
            self.abort()?;

            return Err(err);
        }

        // Create schema and views for migration
        let target_migration = remaining_migrations.last().unwrap().name.to_string();
        self.create_schema_for_migration(&target_migration, &new_schema)
            .with_context(|| {
                format!("failed to create schema for migration {}", target_migration)
            })?;

        // Update state once migrations have been performed
        self.state.in_progress(remaining_migrations);
        self.state
            .save(&mut self.db)
            .context("failed to save in-progress state")?;

        // If we started from a blank slate, we can finish the migration immediately
        if current_migration.is_none() {
            println!("Automatically completing migrations\n");
            self.complete_migration()
                .context("failed to automatically complete migrations")?;

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
        let (remaining_migrations, starting_migration_index, starting_action_index) = match self.state.status.clone() {
            state::Status::InProgress { migrations } => {
                // Move into the Completing state. Once in this state,
                // the migration can't be aborted and must be completed.
                self.state.completing(migrations.clone(), 0, 0);
                self.state.save(&mut self.db).context("failed to save state")?;

                (migrations, 0, 0)
            },
            state::Status::Completing {
                migrations,
                current_migration_index,
                current_action_index
            } => (migrations, current_migration_index, current_action_index),
            state::Status::Aborting { .. } => {
                return Err(anyhow!("migration been aborted and can't be completed. Please finish using `reshape abort`."))
            }
            state::Status::Applying { .. } => {
                return Err(anyhow!("a previous migration unexpectedly failed. Please run `reshape migrate` to try applying the migration again."))
            }
            state::Status::Idle => {
                println!("No migration in progress");
                return Ok(());
            }
        };

        // Remove previous migration's schema
        if let Some(current_migration) = &self.state.current_migration {
            self.db
                .run(&format!(
                    "DROP SCHEMA IF EXISTS {} CASCADE",
                    schema_name_for_migration(current_migration)
                ))
                .context("failed to remove previous migration's schema")?;
        }

        for (migration_index, migration) in remaining_migrations.iter().enumerate() {
            // Skip all the migrations which have already been completed
            if migration_index < starting_migration_index {
                continue;
            }

            println!("Completing '{}':", migration.name);

            for (action_index, action) in migration.actions.iter().enumerate() {
                // Skip all actions which have already been completed
                if migration_index == starting_migration_index
                    && action_index < starting_action_index
                {
                    continue;
                }

                let description = action.describe();
                print!("  + {} ", description);

                let ctx = MigrationContext::new(migration_index, action_index);

                // Update state to indicate that this action has been completed.
                // We won't save this new state until after the action has completed.
                self.state.completing(
                    remaining_migrations.clone(),
                    migration_index + 1,
                    action_index + 1,
                );

                // This did_save check is necessary because of the borrow checker.
                // The Transaction which might be returned from action.complete
                // contains a mutable reference to self.db. We need the Transaction
                // to be dropped before we can save the state using self.db instead,
                // which we achieve here by limiting the lifetime of the Transaction
                // with a new block.
                let did_save = {
                    let result = action
                        .complete(&ctx, &mut self.db)
                        .with_context(|| format!("failed to complete migration {}", migration.name))
                        .with_context(|| format!("failed to complete action: {}", description));

                    let maybe_transaction = match result {
                        Ok(maybe_transaction) => {
                            println!("{}", "done".green());
                            maybe_transaction
                        }
                        Err(e) => {
                            println!("{}", "failed".red());
                            return Err(e);
                        }
                    };

                    // Update state with which migrations and actions have been completed.
                    // Each action can create and return a transaction if they need atomicity.
                    // We use this transaction to update the state to ensure the action only completes.
                    // once.
                    // We want to use a single transaction for each action to keep the length of
                    // the transaction as short as possible. Wherever possible, we don't want to
                    // use a transaction at all.
                    if let Some(mut transaction) = maybe_transaction {
                        self.state
                            .save(&mut transaction)
                            .context("failed to save state after completing action")?;
                        transaction
                            .commit()
                            .context("failed to commit transaction")?;

                        true
                    } else {
                        false
                    }
                };

                // If the action didn't return a transaction we save the state normally instead
                if !did_save {
                    self.state
                        .save(&mut self.db)
                        .context("failed to save state after completing action")?;
                }
            }

            println!();
        }

        // Remove helpers which are no longer in use
        helpers::tear_down_helpers(&mut self.db).context("failed to tear down helpers")?;

        self.state
            .complete()
            .context("failed to update state as completed")?;
        self.state
            .save(&mut self.db)
            .context("failed to save state after setting as completed")?;

        Ok(())
    }

    fn create_schema_for_migration(
        &mut self,
        migration_name: &str,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        // Create schema for migration
        let schema_name = schema_name_for_migration(migration_name);
        self.db
            .run(&format!("CREATE SCHEMA IF NOT EXISTS {}", schema_name))
            .with_context(|| {
                format!(
                    "failed to create schema {} for migration {}",
                    schema_name, migration_name
                )
            })?;

        // Create views inside schema
        for table in schema.get_tables(&mut self.db)? {
            Self::create_view_for_table(&mut self.db, &table, &schema_name)?;
        }

        Ok(())
    }

    fn create_view_for_table(
        db: &mut impl Conn,
        table: &Table,
        schema: &str,
    ) -> anyhow::Result<()> {
        let select_columns: Vec<String> = table
            .columns
            .iter()
            .map(|column| {
                format!(
                    r#"
                    "{real_name}" AS "{alias}"
                    "#,
                    real_name = column.real_name,
                    alias = column.name,
                )
            })
            .collect();

        db.run(&format!(
            r#"
            CREATE OR REPLACE VIEW {schema}."{view_name}" AS
                SELECT {columns}
                FROM "{table_name}"
            "#,
            schema = schema,
            table_name = table.real_name,
            view_name = table.name,
            columns = select_columns.join(","),
        ))
        .with_context(|| format!("failed to create view for table {}", table.name))?;

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

        if let Status::InProgress { migrations } = &self.state.status {
            let target_migration = migrations.last().unwrap().name.to_string();
            self.db.run(&format!(
                "DROP SCHEMA IF EXISTS {} CASCADE",
                schema_name_for_migration(&target_migration)
            ))?;
        }

        // Remove all tables
        let schema = Schema::new();
        for table in schema.get_tables(&mut self.db)? {
            self.db.run(&format!(
                r#"
                DROP TABLE IF EXISTS "{}" CASCADE
                "#,
                table.real_name
            ))?;
        }

        // Reset state
        self.state.clear(&mut self.db)?;

        println!("Reshape and all data has been removed");

        Ok(())
    }

    pub fn abort(&mut self) -> anyhow::Result<()> {
        let (remaining_migrations, last_migration_index, last_action_index) = match self
            .state
            .status
            .clone()
        {
            Status::InProgress { migrations } | Status::Applying { migrations } => {
                // Set to the Aborting state. Once this is done, the migration has to
                // be fully aborted and can't be completed.
                self.state.aborting(migrations.clone(), 0, 0);
                self.state.save(&mut self.db)?;

                (migrations, usize::MAX, usize::MAX)
            }
            Status::Aborting {
                migrations,
                last_migration_index,
                last_action_index,
            } => (migrations, last_migration_index, last_action_index),
            Status::Completing { .. } => {
                return Err(anyhow!("Migration completion has already been started. Please run `reshape complete` again to finish it."));
            }
            Status::Idle => {
                println!("No migration is in progress");
                return Ok(());
            }
        };

        // Remove new migration's schema
        let target_migration = remaining_migrations.last().unwrap().name.to_string();
        let schema_name = schema_name_for_migration(&target_migration);
        self.db
            .run(&format!("DROP SCHEMA IF EXISTS {} CASCADE", schema_name,))
            .with_context(|| format!("failed to drop schema {}", schema_name))?;

        // Abort all pending migrations
        self.abort_migrations(
            &remaining_migrations,
            last_migration_index,
            last_action_index,
        )?;

        helpers::tear_down_helpers(&mut self.db).context("failed to tear down helpers")?;

        self.state.status = state::Status::Idle;
        self.state
            .save(&mut self.db)
            .context("failed to save state")?;

        Ok(())
    }

    fn abort_migrations(
        &mut self,
        migrations: &[Migration],
        upper_migration_index: usize,
        upper_action_index: usize,
    ) -> anyhow::Result<()> {
        // Abort all migrations in reverse order
        for (migration_index, migration) in migrations.iter().enumerate().rev() {
            // Skip migrations which shouldn't be aborted
            // The reason can be that they have already been aborted or that
            // the migration was never applied in the first place.
            if migration_index >= upper_migration_index {
                continue;
            }

            print!("Aborting '{}' ", migration.name);

            for (action_index, action) in migration.actions.iter().enumerate().rev() {
                // Skip actions which shouldn't be aborted
                // The reason can be that they have already been aborted or that
                // the action was never applied in the first place.
                if migration_index == upper_migration_index - 1
                    && action_index >= upper_action_index
                {
                    continue;
                }

                let ctx = MigrationContext::new(migration_index, action_index);
                action
                    .abort(&ctx, &mut self.db)
                    .with_context(|| format!("failed to abort migration {}", migration.name))
                    .with_context(|| format!("failed to abort action: {}", action.describe()))?;

                // Update state with which migrations and actions have been aborted.
                // We don't need to run this in a transaction as aborts are idempotent.
                self.state
                    .aborting(migrations.to_vec(), migration_index, action_index);
                self.state
                    .save(&mut self.db)
                    .context("failed to save state")?;
            }

            println!("{}", "done".green());
        }
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
