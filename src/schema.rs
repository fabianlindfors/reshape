use crate::db::Conn;
use anyhow::anyhow;
use bimap::BiMap;
use std::collections::{hash_map::Entry, HashMap, HashSet};

#[derive(Debug)]
pub struct Schema {
    table_alias_to_name: BiMap<String, String>,
    hidden_tables: HashSet<String>,
    table_schemas_by_name: HashMap<String, TableSchema>,
}

impl Schema {
    pub fn new() -> Schema {
        Schema {
            table_alias_to_name: BiMap::new(),
            hidden_tables: HashSet::new(),
            table_schemas_by_name: HashMap::new(),
        }
    }

    pub fn set_table_alias(&mut self, current_name: &str, alias: &str) {
        self.table_alias_to_name
            .insert(alias.to_string(), current_name.to_string());
    }

    pub fn set_table_hidden(&mut self, table: &str) {
        let real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());
        self.hidden_tables.insert(real_name);
    }

    pub fn set_column_alias(&mut self, table: &str, current_name: &str, alias: &str) {
        let table_real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());

        let column = match self.table_schemas_by_name.entry(table_real_name) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(TableSchema::new()),
        };

        column
            .column_alias_to_name
            .insert(alias.to_string(), current_name.to_string());
    }

    pub fn set_column_hidden(&mut self, table: &str, name: &str) {
        let table_real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());
        let column = match self.table_schemas_by_name.entry(table_real_name) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(TableSchema::new()),
        };

        column.hidden_columns.insert(name.to_string());
    }

    fn get_table_real_name(&self, name: &str) -> Option<String> {
        self.table_alias_to_name
            .get_by_left(name)
            .map(|real_name| real_name.to_string())
    }

    fn get_table_alias_from_real_name<'a>(&'a self, real_name: &'a str) -> Option<String> {
        self.table_alias_to_name
            .get_by_right(real_name)
            .map(|name| name.to_string())
    }

    fn is_table_hidden(&self, table: &str) -> bool {
        let table_real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());
        self.hidden_tables.contains(&table_real_name)
    }

    fn get_column_real_name<'a>(&'a self, table: &'a str, name: &'a str) -> Option<&'a str> {
        let table_real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());

        self.table_schemas_by_name
            .get(&table_real_name)
            .map(|column| column.get_column_real_name(name))
    }

    fn get_column_alias_from_real_name(&self, table: &str, real_name: &str) -> Option<String> {
        let table_real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());

        self.table_schemas_by_name
            .get(&table_real_name)
            .and_then(|column| column.get_column_alias_from_real_name(real_name))
    }

    fn is_column_hidden(&self, table: &str, name: &str) -> bool {
        let table_real_name = self
            .get_table_real_name(table)
            .unwrap_or_else(|| table.to_string());
        self.table_schemas_by_name
            .get(&table_real_name)
            .map(|column| {
                let alias = column
                    .get_column_alias_from_real_name(name)
                    .unwrap_or_else(|| name.to_string());
                column.hidden_columns.contains(&alias)
            })
            .unwrap_or(false)
    }
}

#[derive(Debug)]
struct TableSchema {
    column_alias_to_name: BiMap<String, String>,
    hidden_columns: HashSet<String>,
}

impl TableSchema {
    fn new() -> TableSchema {
        TableSchema {
            column_alias_to_name: BiMap::new(),
            hidden_columns: HashSet::new(),
        }
    }

    fn get_column_real_name<'a>(&'a self, name: &'a str) -> &'a str {
        if let Some(real_name) = self.column_alias_to_name.get_by_left(name) {
            real_name
        } else {
            name
        }
    }

    fn get_column_alias_from_real_name(&self, real_name: &str) -> Option<String> {
        self.column_alias_to_name
            .get_by_right(real_name)
            .map(|name| name.to_string())
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
        .filter(|real_name| !self.is_table_hidden(real_name))
        .map(|real_name| self.get_table_by_real_name(db, &real_name))
        .collect()
    }

    pub fn get_table(&self, db: &mut dyn Conn, table_name: &str) -> anyhow::Result<Table> {
        let real_table_name = self
            .get_table_real_name(table_name)
            .unwrap_or_else(|| table_name.to_string());
        self.get_table_by_real_name(db, &real_table_name)
    }

    pub fn get_table_by_real_name(
        &self,
        db: &mut dyn Conn,
        real_table_name: &str,
    ) -> anyhow::Result<Table> {
        if self.is_table_hidden(real_table_name) {
            return Err(anyhow!("no table named {}", real_table_name));
        }

        let real_columns: Vec<(String, String, bool)> = db
            .query(&format!(
                "
                SELECT column_name, data_type, is_nullable
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
                )
            })
            .collect();

        let mut columns: Vec<Column> = Vec::new();

        for (column_name, data_type, nullable) in real_columns {
            if self.is_column_hidden(real_table_name, &column_name) {
                continue;
            }

            if is_column_temporary(&column_name) {
                continue;
            }

            let (name, real_name) = if let Some(alias) =
                self.get_column_alias_from_real_name(real_table_name, &column_name)
            {
                (alias, column_name)
            } else {
                let real_name = self
                    .get_column_real_name(real_table_name, &column_name)
                    .unwrap_or(&column_name);
                (column_name.to_string(), real_name.to_string())
            };

            columns.push(Column {
                name,
                real_name,
                data_type,
                nullable,
            });
        }

        let table_name = self
            .get_table_alias_from_real_name(real_table_name)
            .unwrap_or_else(|| real_table_name.to_string());

        let table = Table {
            name: table_name,
            real_name: real_table_name.to_string(),
            columns: columns,
        };

        Ok(table)
    }
}

fn is_column_temporary(name: &str) -> bool {
    name.starts_with("__reshape_")
}
