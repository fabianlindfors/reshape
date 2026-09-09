use crate::db::Conn;
use anyhow::anyhow;
use std::collections::{HashMap, HashSet};

// Schema tracks changes made to tables and columns during a migration.
// These changes are not applied until the migration is completed but
// need to be taken into consideration when creating views for a migration
// and when a user references a table or column in a migration.
//
// The changes to a table are tracked by a `TableChanges` struct. The possible
// changes are:
//   - Changing the name which updates `current_name`.
//   - Removing which sets the `removed` flag.
//
// Changes to a column are tracked by a `ColumnChanges` struct which reside in
// the corresponding `TableChanges`. The possible changes are:
//   - Changing the name which updates `current_name`.
//   - Changing the backing column which will add the new column to the end of
//     `backing_columns`. This is used when temporary columns are
//     introduced which will eventually replace the current column.
//   - Declaring the type, nullability or default, which are recorded so that they
//     take precedence over the physical attributes of the backing column. This
//     matters as temporary columns are nullable until the migration completes.
//   - Removing which sets the `removed` flag.
//
// Schema provides some schema introspection methods, `get_tables` and `get_table`,
// which will retrieve the current schema from the database and apply the changes.
#[derive(Debug)]
pub struct Schema {
    table_changes: Vec<TableChanges>,
}

impl Schema {
    pub fn new() -> Schema {
        Schema {
            table_changes: Vec::new(),
        }
    }

    pub fn change_table<F>(&mut self, current_name: &str, f: F)
    where
        F: FnOnce(&mut TableChanges),
    {
        let table_change_index = self
            .table_changes
            .iter()
            .position(|table| table.current_name == current_name)
            .unwrap_or_else(|| {
                let new_changes = TableChanges::new(current_name.to_string());
                self.table_changes.push(new_changes);
                self.table_changes.len() - 1
            });

        let table_changes = &mut self.table_changes[table_change_index];
        f(table_changes)
    }

    /// The real columns which back a column, oldest first. The last one is the column
    /// currently backing it. The earlier ones have been replaced by temporary columns
    /// but remain in the table, and continue to be written, until the migration
    /// completes.
    pub fn backing_columns(&self, table_name: &str, column_name: &str) -> Vec<String> {
        self.table_changes
            .iter()
            .find(|changes| changes.current_name == table_name)
            .and_then(|changes| {
                changes
                    .column_changes
                    .iter()
                    .find(|column| column.current_name == column_name)
            })
            .map(|column| column.backing_columns.clone())
            .unwrap_or_else(|| vec![column_name.to_string()])
    }

    /// Whether a check constraint on a table is removed by an earlier action of the
    /// migration. The constraint still exists in the database until the migration
    /// completes, but shouldn't be treated as part of the schema.
    pub fn is_check_removed(&self, table_name: &str, check_name: &str) -> bool {
        self.table_changes
            .iter()
            .find(|changes| changes.current_name == table_name)
            .map(|changes| changes.removed_checks.iter().any(|name| name == check_name))
            .unwrap_or(false)
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct TableChanges {
    current_name: String,
    real_name: String,
    column_changes: Vec<ColumnChanges>,
    removed: bool,
    removed_checks: Vec<String>,
}

impl TableChanges {
    fn new(name: String) -> Self {
        Self {
            current_name: name.to_string(),
            real_name: name,
            column_changes: Vec::new(),
            removed: false,
            removed_checks: Vec::new(),
        }
    }

    pub fn set_check_removed(&mut self, name: &str) {
        self.removed_checks.push(name.to_string());
    }

    pub fn set_check_added(&mut self, name: &str) {
        self.removed_checks.retain(|removed| removed != name);
    }

    pub fn set_name(&mut self, name: &str) {
        self.current_name = name.to_string();
    }

    pub fn change_column<F>(&mut self, current_name: &str, f: F)
    where
        F: FnOnce(&mut ColumnChanges),
    {
        let column_change_index = self
            .column_changes
            .iter()
            .position(|column| column.current_name == current_name)
            .unwrap_or_else(|| {
                let new_changes = ColumnChanges::new(current_name.to_string());
                self.column_changes.push(new_changes);
                self.column_changes.len() - 1
            });

        let column_changes = &mut self.column_changes[column_change_index];
        f(column_changes)
    }

    pub fn set_removed(&mut self) {
        self.removed = true;
    }
}

#[derive(Debug)]
pub struct ColumnChanges {
    current_name: String,
    backing_columns: Vec<String>,
    removed: bool,
    data_type: Option<String>,
    nullable: Option<bool>,
    default: Option<String>,
}

impl ColumnChanges {
    fn new(name: String) -> Self {
        Self {
            current_name: name.to_string(),
            backing_columns: vec![name],
            removed: false,
            data_type: None,
            nullable: None,
            default: None,
        }
    }

    pub fn set_name(&mut self, name: &str) {
        self.current_name = name.to_string();
    }

    pub fn set_column(&mut self, column_name: &str) {
        self.backing_columns.push(column_name.to_string())
    }

    pub fn set_data_type(&mut self, data_type: &str) {
        self.data_type = Some(data_type.to_string());
    }

    pub fn set_nullable(&mut self, nullable: bool) {
        self.nullable = Some(nullable);
    }

    pub fn set_default(&mut self, default: &str) {
        self.default = Some(default.to_string());
    }

    pub fn set_removed(&mut self) {
        self.removed = true;
    }

    fn real_name(&self) -> &str {
        self.backing_columns
            .last()
            .expect("backing_columns should never be empty")
    }

    /// The column as the schema sees it, given the physical column currently backing it.
    ///
    /// Attributes declared by an action take precedence. Anything not declared is
    /// inherited from the column the chain of backing columns started from, as a
    /// temporary column is nullable until the migration completes no matter what the
    /// column is declared as. A column added in this migration has no such original
    /// column and its temporary column is used instead.
    fn resolve(
        &self,
        current: &PhysicalColumn,
        physical: &HashMap<&str, &PhysicalColumn>,
    ) -> Column {
        let original = self
            .backing_columns
            .first()
            .and_then(|name| physical.get(name.as_str()))
            .copied()
            .unwrap_or(current);

        Column {
            name: self.current_name.clone(),
            real_name: current.name.clone(),
            data_type: self
                .data_type
                .clone()
                .unwrap_or_else(|| original.data_type.clone()),
            nullable: self.nullable.unwrap_or(original.nullable),
            default: self.default.clone().or_else(|| original.default.clone()),
        }
    }
}

/// A column as it exists in the database
#[derive(Debug)]
struct PhysicalColumn {
    name: String,
    data_type: String,
    nullable: bool,
    default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub real_name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub real_name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
}

impl Schema {
    pub fn get_tables(&self, db: &mut dyn Conn) -> anyhow::Result<Vec<Table>> {
        db.query(
            "
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = 'public'
            ",
        )?
        .iter()
        .map(|row| row.get::<'_, _, String>("table_name"))
        .filter_map(|real_name| {
            let table_changes = self
                .table_changes
                .iter()
                .find(|changes| changes.real_name == real_name);

            // Skip table if it has been removed
            if let Some(changes) = table_changes {
                if changes.removed {
                    return None;
                }
            }

            Some(self.get_table_by_real_name(db, &real_name))
        })
        .collect()
    }

    pub fn get_table(&self, db: &mut dyn Conn, table_name: &str) -> anyhow::Result<Table> {
        self.find_table(db, table_name)?
            .ok_or_else(|| anyhow!("table \"{}\" does not exist", table_name))
    }

    /// Looks up a table by its current name. Returns `None` if the schema has no such
    /// table, either because it doesn't exist in the database, because it has been
    /// removed or because the name is one it no longer has.
    pub fn find_table(&self, db: &mut dyn Conn, table_name: &str) -> anyhow::Result<Option<Table>> {
        let table_changes = self
            .table_changes
            .iter()
            .find(|changes| changes.current_name == table_name);

        let real_table_name = match table_changes {
            Some(changes) if changes.removed => return Ok(None),
            Some(changes) => changes.real_name.to_string(),
            None => {
                // The name may be the old name of a table which has been renamed. The
                // real table still exists until the migration completes, but it's no
                // longer known by that name.
                let renamed = self.table_changes.iter().any(|changes| {
                    changes.real_name == table_name && changes.current_name != table_name
                });
                if renamed {
                    return Ok(None);
                }

                table_name.to_string()
            }
        };

        if !self.table_exists(db, &real_table_name)? {
            return Ok(None);
        }

        self.get_table_by_real_name(db, &real_table_name).map(Some)
    }

    fn table_exists(&self, db: &mut dyn Conn, real_table_name: &str) -> anyhow::Result<bool> {
        let rows = db.query(&format!(
            "
            SELECT 1
            FROM information_schema.tables
            WHERE table_name = '{table}' AND table_schema = 'public'
            ",
            table = real_table_name,
        ))?;

        Ok(!rows.is_empty())
    }

    fn get_table_by_real_name(
        &self,
        db: &mut dyn Conn,
        real_table_name: &str,
    ) -> anyhow::Result<Table> {
        let table_changes = self
            .table_changes
            .iter()
            .find(|changes| changes.real_name == real_table_name);

        let real_columns: Vec<PhysicalColumn> = db
            .query(&format!(
                "
                SELECT column_name, CASE WHEN data_type = 'USER-DEFINED' THEN udt_name ELSE data_type END AS data_type, is_nullable, column_default
                FROM information_schema.columns
                WHERE table_name = '{table}' AND table_schema = 'public'
                ORDER BY ordinal_position
                ",
                table = real_table_name,
            ))?
            .iter()
            .map(|row| PhysicalColumn {
                name: row.get("column_name"),
                data_type: row.get("data_type"),
                nullable: row.get::<'_, _, String>("is_nullable") == "YES",
                default: row.get("column_default"),
            })
            .collect();

        // Changed columns inherit attributes from the column they originally replaced,
        // which is looked up by name
        let physical: HashMap<&str, &PhysicalColumn> = real_columns
            .iter()
            .map(|column| (column.name.as_str(), column))
            .collect();

        let mut ignore_columns: HashSet<String> = HashSet::new();
        let mut changed_columns: HashMap<String, &ColumnChanges> = HashMap::new();

        if let Some(changes) = table_changes {
            for column_changes in &changes.column_changes {
                if column_changes.removed {
                    ignore_columns.insert(column_changes.real_name().to_string());
                } else {
                    changed_columns.insert(column_changes.real_name().to_string(), column_changes);
                }

                let (_, rest) = column_changes
                    .backing_columns
                    .split_last()
                    .expect("backing_columns should never be empty");

                for column in rest {
                    ignore_columns.insert(column.to_string());
                }
            }
        }

        let mut columns: Vec<Column> = Vec::new();

        for column in &real_columns {
            if ignore_columns.contains(&column.name) {
                continue;
            }

            let column = match changed_columns.get(&column.name) {
                Some(changes) => changes.resolve(column, &physical),
                None => Column {
                    name: column.name.clone(),
                    real_name: column.name.clone(),
                    data_type: column.data_type.clone(),
                    nullable: column.nullable,
                    default: column.default.clone(),
                },
            };

            columns.push(column);
        }

        let current_table_name = table_changes
            .map(|changes| changes.current_name.as_ref())
            .unwrap_or_else(|| real_table_name);

        let table = Table {
            name: current_table_name.to_string(),
            real_name: real_table_name.to_string(),
            columns,
        };

        Ok(table)
    }
}

impl Table {
    /// The name of the real column backing a column, failing if the column doesn't exist
    pub fn real_column_name(&self, name: &str) -> anyhow::Result<&str> {
        self.get_column(name)
            .map(|column| column.real_name.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "column \"{}\" does not exist on table \"{}\"",
                    name,
                    self.name
                )
            })
    }

    pub fn real_column_names(&self, columns: &[String]) -> anyhow::Result<Vec<String>> {
        columns
            .iter()
            .map(|name| self.real_column_name(name).map(str::to_string))
            .collect()
    }

    pub fn get_column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }
}
