use crate::{db::Conn, schema::Schema};
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

// Re-export migration types
mod common;
pub use common::{Column, ColumnBuilder};

mod create_table;
pub use create_table::{CreateTable, CreateTableBuilder, ForeignKey};

mod alter_column;
pub use alter_column::{AlterColumn, ColumnChanges};

mod add_column;
pub use add_column::AddColumn;

mod remove_column;
pub use remove_column::RemoveColumn;

mod add_index;
pub use add_index::AddIndex;

mod remove_table;
pub use remove_table::RemoveTable;

mod rename_table;
pub use rename_table::RenameTable;

#[derive(Serialize, Deserialize, Debug)]
pub struct Migration {
    pub name: String,
    pub description: Option<String>,
    pub actions: Vec<Box<dyn Action>>,
}

impl Migration {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Migration {
        Migration {
            name: name.into(),
            description,
            actions: vec![],
        }
    }

    pub fn with_action(mut self, action: impl Action + 'static) -> Self {
        self.actions.push(Box::new(action));
        self
    }
}

impl PartialEq for Migration {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Migration {}

impl Clone for Migration {
    fn clone(&self) -> Self {
        let serialized = serde_json::to_string(self).unwrap();
        serde_json::from_str(&serialized).unwrap()
    }
}

pub struct MigrationContext {
    migration_index: usize,
    action_index: usize,
}

impl MigrationContext {
    pub fn new(migration_index: usize, action_index: usize) -> Self {
        MigrationContext {
            migration_index,
            action_index,
        }
    }

    fn prefix(&self) -> String {
        format!(
            "__reshape_{:0>4}_{:0>4}",
            self.migration_index, self.action_index
        )
    }

    fn prefix_inverse(&self) -> String {
        format!(
            "__reshape_{:0>4}_{:0>4}",
            1000 - self.migration_index,
            1000 - self.action_index
        )
    }
}

#[typetag::serde(tag = "type")]
pub trait Action: Debug {
    fn describe(&self) -> String;
    fn run(&self, ctx: &MigrationContext, db: &mut dyn Conn, schema: &Schema)
        -> anyhow::Result<()>;
    fn complete(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()>;
    fn update_schema(&self, ctx: &MigrationContext, schema: &mut Schema);
    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()>;
}
