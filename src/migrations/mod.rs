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
    /// Columns of the two tables of a cross-table transformation. Every reference must
    /// be qualified with the name of its table, as the SQL runs in triggers on both
    /// tables where an unqualified name would resolve differently.
    CrossTable(TableScope, TableScope),
}

impl References {
    pub fn table(table: &str) -> Self {
        References::Table(TableScope::schema(table))
    }

    fn scopes(&self) -> Vec<&TableScope> {
        match self {
            References::Unchecked | References::Forbidden => vec![],
            References::Table(scope) => vec![scope],
            References::CrossTable(first, second) => vec![first, second],
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

/// A field of an action which names a table or a column
#[derive(Debug, Clone)]
pub struct NameField {
    pub name: String,
    pub value: String,
    pub kind: NameKind,
}

#[derive(Debug, Clone)]
pub enum NameKind {
    /// The name of a table
    Table,
    /// The name of a column of the given table
    Column(TableScope),
}

impl NameField {
    pub fn table(name: impl Into<String>, value: impl Into<String>) -> Self {
        NameField {
            name: name.into(),
            value: value.into(),
            kind: NameKind::Table,
        }
    }

    pub fn column(name: impl Into<String>, value: impl Into<String>, table: TableScope) -> Self {
        NameField {
            name: name.into(),
            value: value.into(),
            kind: NameKind::Column(table),
        }
    }
}

/// An error found in a field of an action
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub value: String,
    pub message: String,
}

/// Validates the fields of an action as far as possible without a schema: the syntax of
/// every SQL field and the references which don't depend on tables in the schema
pub fn validate(action: &dyn Action) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for field in action.sql_fields() {
        if let Err(message) = validate_sql_field(&field) {
            errors.push(ValidationError {
                field: field.name,
                value: field.sql,
                message,
            });
        }
    }

    for field in action.name_fields() {
        if let Err(message) = validate_name_field(&field) {
            errors.push(ValidationError {
                field: field.name,
                value: field.value,
                message,
            });
        }
    }

    errors
}

/// Validates the fields of an action, including references to tables in the schema as
/// it looks right before the action runs
pub fn validate_against_schema(
    action: &dyn Action,
    db: &mut dyn Conn,
    schema: &Schema,
) -> anyhow::Result<Vec<ValidationError>> {
    let mut errors = Vec::new();

    for field in action.sql_fields() {
        let scopes = field.references.scopes();
        let result = match validate_sql_field(&field) {
            Ok(()) if !scopes.iter().all(|scope| scope.is_explicit()) => {
                validate_sql_references_against_schema(&field.sql, &scopes, db, schema)?
            }
            result => result,
        };

        if let Err(message) = result {
            errors.push(ValidationError {
                field: field.name,
                value: field.sql,
                message,
            });
        }
    }

    for field in action.name_fields() {
        let result = match validate_name_field(&field) {
            Ok(()) => validate_name_against_schema(&field, db, schema)?,
            result => result,
        };

        if let Err(message) = result {
            errors.push(ValidationError {
                field: field.name,
                value: field.value,
                message,
            });
        }
    }

    Ok(errors)
}

fn validate_sql_field(field: &SqlField) -> Result<(), String> {
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

fn validate_sql_references_against_schema(
    sql: &str,
    scopes: &[&TableScope],
    db: &mut dyn Conn,
    schema: &Schema,
) -> anyhow::Result<Result<(), String>> {
    let mut tables = Vec::new();
    for scope in scopes {
        match resolve_scope(scope, db, schema)? {
            Ok(table) => tables.push(table),
            Err(message) => return Ok(Err(message)),
        }
    }

    let tables: Vec<&Table> = tables.iter().collect();
    Ok(join_errors(crate::sql::validate_column_references(
        sql, &tables,
    )))
}

// Names of tables and of columns of tables in the schema can only be checked once the
// schema is available
fn validate_name_field(field: &NameField) -> Result<(), String> {
    match &field.kind {
        NameKind::Column(TableScope::Explicit(table)) => check_column(table, &field.value),
        NameKind::Table | NameKind::Column(TableScope::Schema { .. }) => Ok(()),
    }
}

fn validate_name_against_schema(
    field: &NameField,
    db: &mut dyn Conn,
    schema: &Schema,
) -> anyhow::Result<Result<(), String>> {
    match &field.kind {
        NameKind::Table => Ok(match schema.find_table(db, &field.value)? {
            Some(_) => Ok(()),
            None => Err(format!("table \"{}\" does not exist", field.value)),
        }),
        NameKind::Column(scope @ TableScope::Schema { .. }) => {
            Ok(resolve_scope(scope, db, schema)?
                .and_then(|table| check_column(&table, &field.value)))
        }
        NameKind::Column(TableScope::Explicit(_)) => Ok(Ok(())),
    }
}

fn check_column(table: &Table, column: &str) -> Result<(), String> {
    if table.get_column(column).is_some() {
        Ok(())
    } else {
        Err(format!(
            "column \"{}\" does not exist on table \"{}\"",
            column, table.name
        ))
    }
}

fn resolve_scope(
    scope: &TableScope,
    db: &mut dyn Conn,
    schema: &Schema,
) -> anyhow::Result<Result<Table, String>> {
    match scope {
        TableScope::Explicit(table) => Ok(Ok(table.clone())),
        TableScope::Schema {
            table,
            excluded_columns,
        } => {
            let Some(mut resolved) = schema.find_table(db, table)? else {
                return Ok(Err(format!("table \"{}\" does not exist", table)));
            };

            resolved
                .columns
                .retain(|column| !excluded_columns.contains(&column.name));
            Ok(Ok(resolved))
        }
    }
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
pub(crate) mod common;
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

mod add_check;
pub use add_check::AddCheck;

mod remove_check;
pub use remove_check::RemoveCheck;

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

    /// Builds the name `{prefix}_{kind}_{parts joined by _}{suffix}` for an object created
    /// by an action, shortened to fit within Postgres' identifier limit when needed. The
    /// prefix, kind and suffix are always kept intact so that related objects, like a
    /// trigger and its reverse, never end up with the same name.
    fn name(&self, kind: &str, parts: &[&str], suffix: &str) -> String {
        common::bounded_identifier(
            &format!("{}_{}_", self.prefix(), kind),
            &parts.join("_"),
            suffix,
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

    /// Tables and columns named by the action, which are validated before the action runs
    fn name_fields(&self) -> Vec<NameField> {
        vec![]
    }
}
