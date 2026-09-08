use crate::{
    db::{Conn, Transaction},
    schema::Schema,
};
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

/// A field of an action which holds user-provided SQL
#[derive(Debug, Clone)]
pub struct SqlField {
    pub name: String,
    pub sql: String,
    pub kind: SqlKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKind {
    /// An expression evaluating to a value
    Expression,
    /// One or more complete statements
    Statement,
}

impl SqlField {
    pub fn expression(name: impl Into<String>, sql: impl Into<String>) -> Self {
        SqlField {
            name: name.into(),
            sql: sql.into(),
            kind: SqlKind::Expression,
        }
    }

    pub fn statement(name: impl Into<String>, sql: impl Into<String>) -> Self {
        SqlField {
            name: name.into(),
            sql: sql.into(),
            kind: SqlKind::Statement,
        }
    }
}

/// An error found in user-provided SQL
#[derive(Debug, Clone)]
pub struct SqlError {
    pub field: String,
    pub sql: String,
    pub message: String,
}

pub fn validate_sql(action: &dyn Action) -> Vec<SqlError> {
    action
        .sql_fields()
        .into_iter()
        .filter_map(|field| {
            let result = match field.kind {
                SqlKind::Expression => crate::sql::validate_sql_expression(&field.sql),
                SqlKind::Statement => crate::sql::validate_sql_statement(&field.sql),
            };

            result.err().map(|message| SqlError {
                field: field.name,
                sql: field.sql,
                message,
            })
        })
        .collect()
}

/// Quote a value so it can be used as an SQL string literal.
///
/// Used for statements which don't accept query parameters, such as `COMMENT ON`.
pub fn quote_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

// Re-export migration types
mod common;
pub use common::Column;

mod create_table;
pub use create_table::CreateTable;

mod alter_column;
pub use alter_column::{AlterColumn, ColumnChanges};

mod add_column;
pub use add_column::AddColumn;

mod remove_column;
pub use remove_column::RemoveColumn;

mod add_index;
pub use add_index::{AddIndex, Index};

mod remove_index;
pub use remove_index::RemoveIndex;

mod remove_table;
pub use remove_table::RemoveTable;

mod rename_table;
pub use rename_table::RenameTable;

mod create_enum;
pub use create_enum::CreateEnum;

mod remove_enum;
pub use remove_enum::RemoveEnum;

mod custom;
pub use custom::Custom;

mod add_foreign_key;
pub use add_foreign_key::AddForeignKey;

mod remove_foreign_key;
pub use remove_foreign_key::RemoveForeignKey;

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
    existing_schema_name: Option<String>,
}

impl MigrationContext {
    pub fn new(
        migration_index: usize,
        action_index: usize,
        existing_schema_name: Option<String>,
    ) -> Self {
        MigrationContext {
            migration_index,
            action_index,
            existing_schema_name,
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
    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>>;
    fn update_schema(&self, ctx: &MigrationContext, schema: &mut Schema);
    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()>;

    /// User-provided SQL in the action, which is validated before the action runs
    fn sql_fields(&self) -> Vec<SqlField> {
        vec![]
    }
}
