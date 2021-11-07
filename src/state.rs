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
        migrations: Vec<Migration>,
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

    pub fn add_migrations<'a>(&mut self, new_migrations: impl IntoIterator<Item = &'a Migration>) {
        for migration in new_migrations.into_iter() {
            self.migrations.push(migration.clone());
        }
    }

    // Complete will change the status from InProgress to Idle
    pub fn complete(&mut self) -> anyhow::Result<()> {
        let current_status = std::mem::replace(&mut self.status, Status::Idle);

        match current_status {
            Status::Idle => {
                // Move old status back
                self.status = current_status;
                return Err(anyhow!("status is not in progress"));
            }
            Status::InProgress {
                mut migrations,
                target_schema,
            } => {
                let target_migration = migrations.last().unwrap().name.to_string();
                self.migrations.append(&mut migrations);
                self.current_migration = Some(target_migration);
                self.current_schema = target_schema;
            }
        }

        Ok(())
    }

    pub fn in_progress(&mut self, new_migrations: Vec<Migration>, new_schema: Schema) {
        self.status = Status::InProgress {
            migrations: new_migrations,
            target_schema: new_schema,
        };
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
        println!("Remaining migrations count: {:?}", items);

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
            current_schema: Schema::new(),
            migrations: vec![],
        }
    }
}
