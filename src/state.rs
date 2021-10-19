use crate::schema::Schema;
use crate::{db::Conn, migrations::Migration};
use anyhow::anyhow;

use serde::{Deserialize, Serialize};
use version::version;

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub version: String,
    pub status: Status,
    pub current_schema: Schema,
    pub current_migration: Option<String>,
    pub migrations: Vec<Migration>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status")]
pub enum Status {
    #[serde(rename = "idle")]
    Idle,

    #[serde(rename = "in_progress")]
    InProgress {
        target_migration: String,
        target_schema: Schema,
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
        self.current_schema = default.current_schema;
        self.migrations = default.migrations;

        Ok(())
    }

    pub fn set_migrations<T>(&mut self, migrations: T) -> Result<(), anyhow::Error>
    where
        T: IntoIterator<Item = Migration>,
    {
        let mut new_iter = migrations.into_iter();

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

        // Add any new migrations
        self.migrations.extend(new_iter);
        Ok(())
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
            current_schema: Schema::new(),
            migrations: vec![],
        }
    }
}
