use crate::{db::Conn, migrations::Migration};
use anyhow::anyhow;

use serde::{Deserialize, Serialize};
use version::version;

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub version: String,
    pub status: Status,
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

        Ok(())
    }

    // Complete will change the status from Completing to Idle
    pub fn complete(&mut self, db: &mut impl Conn) -> anyhow::Result<()> {
        let current_status = std::mem::replace(&mut self.status, Status::Idle);
        match current_status {
            Status::Completing { migrations, .. } => {
                // Add migrations and update state in a transaction to ensure atomicity
                let mut transaction = db.transaction()?;
                save_migrations(&mut transaction, &migrations)?;
                self.save(&mut transaction)?;
                transaction.commit()?;
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

    fn ensure_schema_and_table(db: &mut impl Conn) {
        db.run("CREATE SCHEMA IF NOT EXISTS reshape").unwrap();

        db.run("CREATE TABLE IF NOT EXISTS reshape.data (key TEXT PRIMARY KEY, value JSONB)")
            .unwrap();

        db.run(
            "
            CREATE TABLE IF NOT EXISTS reshape.migrations (
                index INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                actions JSONB NOT NULL,
                completed_at TIMESTAMP DEFAULT NOW()
            )
            ",
        )
        .unwrap();
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            version: version!().to_string(),
            status: Status::Idle,
        }
    }
}

pub fn current_migration(db: &mut dyn Conn) -> anyhow::Result<Option<String>> {
    let name: Option<String> = db
        .query(
            "
            SELECT name
            FROM reshape.migrations
            ORDER BY index DESC
            LIMIT 1
            ",
        )?
        .first()
        .map(|row| row.get("name"));
    Ok(name)
}

pub fn remaining_migrations(
    db: &mut impl Conn,
    new_migrations: impl IntoIterator<Item = Migration>,
) -> anyhow::Result<Vec<Migration>> {
    let mut new_iter = new_migrations.into_iter();

    // Ensure the new migrations match up with the existing ones
    let mut highest_index: Option<i32> = None;
    loop {
        let migrations = get_migrations(db, highest_index)?;
        if migrations.is_empty() {
            break;
        }

        for (index, existing) in migrations {
            highest_index = Some(index);

            let new = match new_iter.next() {
                Some(migration) => migration,
                None => {
                    return Err(anyhow!(
                        "existing migration {} doesn't exist in local migrations",
                        existing
                    ))
                }
            };

            if existing != new.name {
                return Err(anyhow!(
                    "existing migration {} does not match new migration {}",
                    existing,
                    new.name
                ));
            }
        }
    }

    // Return the remaining migrations
    let items: Vec<Migration> = new_iter.collect();
    Ok(items)
}

fn get_migrations(
    db: &mut impl Conn,
    index_larger_than: Option<i32>,
) -> anyhow::Result<Vec<(i32, String)>> {
    let rows = if let Some(index_larger_than) = index_larger_than {
        db.query_with_params(
            "
            SELECT index, name
            FROM reshape.migrations
            WHERE index > $1
            ORDER BY index ASC
            LIMIT 100
            ",
            &[&index_larger_than],
        )?
    } else {
        db.query(
            "
            SELECT index, name
            FROM reshape.migrations
            LIMIT 100
            ",
        )?
    };

    let migrations = rows
        .iter()
        .map(|row| (row.get("index"), row.get("name")))
        .collect();
    Ok(migrations)
}

fn save_migrations(db: &mut impl Conn, migrations: &[Migration]) -> anyhow::Result<()> {
    for migration in migrations {
        let encoded_actions = serde_json::to_value(&migration.actions)?;
        db.query_with_params(
            "INSERT INTO reshape.migrations(name, description, actions) VALUES ($1, $2, $3)",
            &[&migration.name, &migration.description, &encoded_actions],
        )?;
    }

    Ok(())
}
