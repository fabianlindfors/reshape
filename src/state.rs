use crate::{db::Conn, migrations::Migration};
use anyhow::anyhow;

use serde::{Deserialize, Serialize};
use version::version;

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub version: String,
    pub status: Status,
    pub current_migration: Option<String>,
    pub migrations: Vec<Migration>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "status")]
pub enum Status {
    #[serde(rename = "idle")]
    Idle,

    #[serde(rename = "applying")]
    Applying { migrations: Vec<Migration> },

    #[serde(rename = "in_progress")]
    InProgress { migrations: Vec<Migration> },

    #[serde(rename = "completing")]
    Completing {
        migrations: Vec<Migration>,
        current_migration_index: usize,
        current_action_index: usize,
    },

    #[serde(rename = "aborting")]
    Aborting {
        migrations: Vec<Migration>,
        last_migration_index: usize,
        last_action_index: usize,
    },
}

impl State {
    pub fn load(db: &mut impl Conn) -> State {
        Self::ensure_schema_and_table(db);

        let results = db
            .query("SELECT value FROM reshape.data WHERE key = 'state'")
            .unwrap();

        match results.first() {
            Some(row) => {
                let json: serde_json::Value = row.get(0);
                serde_json::from_value(json).unwrap()
            }
            None => Default::default(),
        }
    }

    pub fn save(&self, db: &mut impl Conn) -> anyhow::Result<()> {
        Self::ensure_schema_and_table(db);

        let json = serde_json::to_value(self)?;
        db.query_with_params(
            "INSERT INTO reshape.data (key, value) VALUES ('state', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
            &[&json]
        )?;
        Ok(())
    }

    pub fn clear(&mut self, db: &mut impl Conn) -> anyhow::Result<()> {
        db.run("DROP SCHEMA reshape CASCADE")?;

        let default = Self::default();
        self.status = default.status;
        self.current_migration = default.current_migration;
        self.migrations = default.migrations;

        Ok(())
    }

    // Complete will change the status from Completing to Idle
    pub fn complete(&mut self) -> anyhow::Result<()> {
        let current_status = std::mem::replace(&mut self.status, Status::Idle);

        match current_status {
            Status::Completing { mut migrations, .. } => {
                let target_migration = migrations.last().unwrap().name.to_string();
                self.migrations.append(&mut migrations);
                self.current_migration = Some(target_migration);
            }
            _ => {
                // Move old status back
                self.status = current_status;
                return Err(anyhow!(
                    "couldn't update state to be completed, not in Completing state"
                ));
            }
        }

        Ok(())
    }

    pub fn applying(&mut self, new_migrations: Vec<Migration>) {
        self.status = Status::Applying {
            migrations: new_migrations,
        };
    }

    pub fn in_progress(&mut self, new_migrations: Vec<Migration>) {
        self.status = Status::InProgress {
            migrations: new_migrations,
        };
    }

    pub fn completing(
        &mut self,
        migrations: Vec<Migration>,
        current_migration_index: usize,
        current_action_index: usize,
    ) {
        self.status = Status::Completing {
            migrations,
            current_migration_index,
            current_action_index,
        }
    }

    pub fn aborting(
        &mut self,
        migrations: Vec<Migration>,
        last_migration_index: usize,
        last_action_index: usize,
    ) {
        self.status = Status::Aborting {
            migrations,
            last_migration_index,
            last_action_index,
        }
    }

    pub fn get_remaining_migrations(
        &self,
        new_migrations: impl IntoIterator<Item = Migration>,
    ) -> anyhow::Result<Vec<Migration>> {
        let mut new_iter = new_migrations.into_iter();

        // Ensure the new migration match up with the existing ones
        for pair in self.migrations.iter().zip(new_iter.by_ref()) {
            let (existing, ref new) = pair;
            if existing != new {
                return Err(anyhow!(
                    "existing migration {} does not match new migration {}",
                    existing.name,
                    new.name
                ));
            }
        }

        let items: Vec<Migration> = new_iter.collect();

        // Return the remaining migrations
        Ok(items)
    }

    fn ensure_schema_and_table(db: &mut impl Conn) {
        db.run("CREATE SCHEMA IF NOT EXISTS reshape").unwrap();

        db.run("CREATE TABLE IF NOT EXISTS reshape.data (key TEXT PRIMARY KEY, value JSONB)")
            .unwrap();
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            version: version!().to_string(),
            status: Status::Idle,
            current_migration: None,
            migrations: vec![],
        }
    }
}
