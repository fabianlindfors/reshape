use crate::db::Conn;
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
//     `intermediate_columns`. This is used when temporary columns are
//     introduced which will eventually replace the current column.
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
}

impl TableChanges {
    fn new(name: String) -> Self {
        Self {
            current_name: name.to_string(),
            real_name: name,
            column_changes: Vec::new(),
            removed: false,
        }
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
}

impl ColumnChanges {
    fn new(name: String) -> Self {
        Self {
            current_name: name.to_string(),
            backing_columns: vec![name],
            removed: false,
        }
    }

    pub fn set_name(&mut self, name: &str) {
        self.current_name = name.to_string();
    }

    pub fn set_column(&mut self, column_name: &str) {
        self.backing_columns.push(column_name.to_string())
    }

    pub fn set_removed(&mut self) {
        self.removed = true;
    }

    fn real_name(&self) -> &str {
        self.backing_columns
            .last()
            .expect("backing_columns should never be empty")
    }
}

#[derive(Debug)]
pub struct Table {
    pub name: String,
    pub real_name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug)]
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
        let table_changes = self
            .table_changes
            .iter()
            .find(|changes| changes.current_name == table_name);

        let real_table_name = table_changes
            .map(|changes| changes.real_name.to_string())
            .unwrap_or_else(|| table_name.to_string());

        self.get_table_by_real_name(db, &real_table_name)
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

        let real_columns: Vec<(String, String, bool, Option<String>)> = db
            .query(&format!(
                "
                SELECT column_name, data_type, is_nullable, column_default
                FROM information_schema.columns
                WHERE table_name = '{table}' AND table_schema = 'public'
                ORDER BY ordinal_position
                ",
                table = real_table_name,
            ))?
            .iter()
            .map(|row| {
                (
                    row.get("column_name"),
                    row.get("data_type"),
                    row.get::<'_, _, String>("is_nullable") == "YES",
                    row.get("column_default"),
                )
            })
            .collect();

        let mut ignore_columns: HashSet<String> = HashSet::new();
        let mut aliases: HashMap<String, &str> = HashMap::new();

        if let Some(changes) = table_changes {
            for column_changes in &changes.column_changes {
                if column_changes.removed {
                    ignore_columns.insert(column_changes.real_name().to_string());
                } else {
                    aliases.insert(
                        column_changes.real_name().to_string(),
                        &column_changes.current_name,
                    );
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

        for (real_name, data_type, nullable, default) in real_columns {
            if ignore_columns.contains(&*real_name) {
                continue;
            }

            let name = aliases
                .get(&real_name)
                .map(|alias| alias.to_string())
                .unwrap_or_else(|| real_name.to_string());

            columns.push(Column {
                name,
                real_name,
                data_type,
                nullable,
                default,
            });
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
