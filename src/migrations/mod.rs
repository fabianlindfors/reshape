use crate::{
    db::{Conn, Transaction},
    schema::{Schema, Table},
};
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

/// A field of an action which holds user-provided SQL
#[derive(Debug, Clone)]
pub struct SqlField {
    pub name: String,
    pub sql: String,
    pub kind: SqlKind,
    pub references: References,
}

/// The columns a piece of SQL may reference
#[derive(Debug, Clone)]
pub enum References {
    /// Not checked
    Unchecked,
    /// The SQL may not reference any columns at all
    Forbidden,
    /// Columns of a table, referenced with or without the table name
    Table(TableScope),
    /// Columns of two tables, as in a cross-table transformation. Every reference must
    /// be qualified with the name of its table, as the SQL runs in triggers on both
    /// tables where an unqualified name would resolve differently.
    Tables(TableScope, TableScope),
}

impl References {
    pub fn table(table: &str) -> Self {
        References::Table(TableScope::schema(table))
    }

    fn scopes(&self) -> Vec<&TableScope> {
        match self {
            References::Unchecked | References::Forbidden => vec![],
            References::Table(scope) => vec![scope],
            References::Tables(first, second) => vec![first, second],
        }
    }
}

/// A table whose columns may be referenced
#[derive(Debug, Clone)]
pub enum TableScope {
    /// A table in the schema, resolved right before the action runs
    Schema {
        table: String,
        /// Columns which exist on the table but can't be referenced, such as a column
        /// which is being removed
        excluded_columns: Vec<String>,
    },
    /// A table which doesn't exist in the schema yet, with its columns known up front
    Explicit(Table),
}

impl TableScope {
    pub fn schema(table: &str) -> Self {
        TableScope::Schema {
            table: table.to_string(),
            excluded_columns: Vec::new(),
        }
    }

    pub fn excluding(mut self, column: &str) -> Self {
        match &mut self {
            TableScope::Schema {
                excluded_columns, ..
            } => excluded_columns.push(column.to_string()),
            TableScope::Explicit(table) => table.columns.retain(|c| c.name != column),
        }
        self
    }

    fn is_explicit(&self) -> bool {
        matches!(self, TableScope::Explicit(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKind {
    /// An expression evaluating to a value
    Expression,
    /// One or more complete statements
    Statement,
}

impl SqlField {
    pub fn expression(
        name: impl Into<String>,
        sql: impl Into<String>,
        references: References,
    ) -> Self {
        SqlField {
            name: name.into(),
            sql: sql.into(),
            kind: SqlKind::Expression,
            references,
        }
    }

    pub fn statement(name: impl Into<String>, sql: impl Into<String>) -> Self {
        SqlField {
            name: name.into(),
            sql: sql.into(),
            kind: SqlKind::Statement,
            references: References::Unchecked,
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

/// Validates the SQL of an action as far as possible without a schema: the syntax of every
/// field and the column references which don't depend on existing tables
pub fn validate_sql(action: &dyn Action) -> Vec<SqlError> {
    action
        .sql_fields()
        .into_iter()
        .filter_map(|field| {
            validate_field(&field).err().map(|message| SqlError {
                field: field.name,
                sql: field.sql,
                message,
            })
        })
        .collect()
}

/// Validates the SQL of an action, including column references against the tables of
/// the schema as it looks right before the action runs
pub fn validate_sql_against_schema(
    action: &dyn Action,
    db: &mut dyn Conn,
    schema: &Schema,
) -> anyhow::Result<Vec<SqlError>> {
    let mut errors = Vec::new();

    for field in action.sql_fields() {
        let scopes = field.references.scopes();
        let result = match validate_field(&field) {
            Ok(()) if !scopes.iter().all(|scope| scope.is_explicit()) => {
                validate_references_against_schema(&field.sql, &scopes, db, schema)?
            }
            result => result,
        };

        if let Err(message) = result {
            errors.push(SqlError {
                field: field.name,
                sql: field.sql,
                message,
            });
        }
    }

    Ok(errors)
}

fn validate_field(field: &SqlField) -> Result<(), String> {
    match field.kind {
        SqlKind::Expression => crate::sql::validate_sql_expression(&field.sql)?,
        SqlKind::Statement => crate::sql::validate_sql_statement(&field.sql)?,
    }

    if let References::Forbidden = field.references {
        return crate::sql::validate_no_column_references(&field.sql);
    }

    // Tables in the schema can only be checked once the schema is available
    let scopes = field.references.scopes();
    if scopes.is_empty() || !scopes.iter().all(|scope| scope.is_explicit()) {
        return Ok(());
    }

    let tables: Vec<&Table> = scopes
        .iter()
        .filter_map(|scope| match scope {
            TableScope::Explicit(table) => Some(table),
            TableScope::Schema { .. } => None,
        })
        .collect();
    join_errors(crate::sql::validate_column_references(&field.sql, &tables))
}

fn validate_references_against_schema(
    sql: &str,
    scopes: &[&TableScope],
    db: &mut dyn Conn,
    schema: &Schema,
) -> anyhow::Result<Result<(), String>> {
    let mut tables = Vec::new();
    for scope in scopes {
        let table = match scope {
            TableScope::Explicit(table) => table.clone(),
            TableScope::Schema {
                table,
                excluded_columns,
            } => {
                let mut resolved = schema.get_table(db, table)?;

                // A table which doesn't exist comes back without any columns
                if resolved.columns.is_empty() {
                    return Ok(Err(format!("table \"{}\" does not exist", table)));
                }

                resolved
                    .columns
                    .retain(|column| !excluded_columns.contains(&column.name));
                resolved
            }
        };
        tables.push(table);
    }

    let tables: Vec<&Table> = tables.iter().collect();
    Ok(join_errors(crate::sql::validate_column_references(
        sql, &tables,
    )))
}

fn join_errors(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(", "))
    }
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
