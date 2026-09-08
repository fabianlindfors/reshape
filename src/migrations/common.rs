use anyhow::anyhow;
use postgres::types::{FromSql, ToSql};
use serde::{Deserialize, Serialize};

use crate::{db::Conn, schema::Table};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
    #[serde(default = "nullable_default")]
    pub nullable: bool,
    pub default: Option<String>,
    pub generated: Option<String>,
    pub comment: Option<String>,
}

fn nullable_default() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_delete: Option<ReferentialAction>,
    pub on_update: Option<ReferentialAction>,
}

impl ForeignKey {
    // Renders the ON DELETE and ON UPDATE clauses, if any. The clauses have to be placed
    // directly after the REFERENCES clause and before any constraint attributes, such as
    // NOT VALID.
    pub fn referential_actions_definition(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(on_delete) = &self.on_delete {
            parts.push(format!("ON DELETE {}", on_delete.as_sql()));
        }

        if let Some(on_update) = &self.on_update {
            parts.push(format!("ON UPDATE {}", on_update.as_sql()));
        }

        parts.join(" ")
    }
}

// The referential action taken when the referenced row is deleted or updated.
// Deserialized from the same keywords Postgres uses, for example "CASCADE" or
// "SET NULL", in either upper or lower case.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferentialAction {
    #[serde(rename = "NO ACTION", alias = "no action")]
    NoAction,

    #[serde(rename = "RESTRICT", alias = "restrict")]
    Restrict,

    #[serde(rename = "CASCADE", alias = "cascade")]
    Cascade,

    #[serde(rename = "SET NULL", alias = "set null")]
    SetNull,

    #[serde(rename = "SET DEFAULT", alias = "set default")]
    SetDefault,
}

impl ReferentialAction {
    pub fn as_sql(&self) -> &'static str {
        match self {
            ReferentialAction::NoAction => "NO ACTION",
            ReferentialAction::Restrict => "RESTRICT",
            ReferentialAction::Cascade => "CASCADE",
            ReferentialAction::SetNull => "SET NULL",
            ReferentialAction::SetDefault => "SET DEFAULT",
        }
    }
}

// A CHECK constraint on a table. The name is optional, in which case Postgres
// will generate one, but naming it makes it possible to reference the constraint
// later on.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Check {
    pub name: Option<String>,
    pub expression: String,
}

impl Check {
    pub fn definition(&self) -> String {
        match &self.name {
            Some(name) => format!(
                r#"CONSTRAINT "{name}" CHECK ({expression})"#,
                name = name,
                expression = self.expression,
            ),
            None => format!("CHECK ({})", self.expression),
        }
    }
}

#[derive(Debug)]
struct PostgresRawValue {
    bytes: Vec<u8>,
}

impl<'a> FromSql<'a> for PostgresRawValue {
    fn from_sql(
        _ty: &postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(PostgresRawValue {
            bytes: raw.to_vec(),
        })
    }

    fn accepts(_ty: &postgres::types::Type) -> bool {
        true
    }
}

impl ToSql for PostgresRawValue {
    fn to_sql(
        &self,
        _ty: &postgres::types::Type,
        out: &mut postgres::types::private::BytesMut,
    ) -> Result<postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>>
    where
        Self: Sized,
    {
        out.extend_from_slice(&self.bytes);
        Ok(postgres::types::IsNull::No)
    }

    fn accepts(_ty: &postgres::types::Type) -> bool
    where
        Self: Sized,
    {
        true
    }

    postgres::types::to_sql_checked!();
}

pub fn batch_touch_rows(
    db: &mut dyn Conn,
    table: &str,
    column: Option<&str>,
) -> anyhow::Result<()> {
    const BATCH_SIZE: u16 = 1000;

    let mut cursor: Option<PostgresRawValue> = None;

    loop {
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();

        let primary_key = get_primary_key_columns_for_table(db, table)?;

        // If no column to touch is passed, we default to the first primary key column (just to make some "update")
        let touched_column = match column {
            Some(column) => column,
            None => primary_key.first().unwrap(),
        };

        let primary_key_columns = primary_key.join(", ");

        let primary_key_where = primary_key
            .iter()
            .map(|column| {
                format!(
                    r#"
                    "{table}"."{column}" = rows."{column}"
                    "#,
                    table = table,
                    column = column,
                )
            })
            .collect::<Vec<String>>()
            .join(" AND ");

        let returning_columns = primary_key
            .iter()
            .map(|column| format!("rows.\"{}\"", column))
            .collect::<Vec<String>>()
            .join(", ");

        let cursor_where = if let Some(cursor) = &cursor {
            params.push(cursor);

            format!(
                "WHERE ({primary_key_columns}) > $1",
                primary_key_columns = primary_key_columns
            )
        } else {
            "".to_string()
        };

        let query = format!(
            r#"
            WITH rows AS (
                SELECT {primary_key_columns}
                FROM public."{table}"
                {cursor_where}
                ORDER BY {primary_key_columns}
                LIMIT {batch_size}
            ), update AS (
                UPDATE public."{table}" "{table}"
                SET "{touched_column}" = "{table}"."{touched_column}"
                FROM rows
                WHERE {primary_key_where}
                RETURNING {returning_columns}
            )
            SELECT LAST_VALUE(({primary_key_columns})) OVER () AS last_value
            FROM update
            LIMIT 1
            "#,
            batch_size = BATCH_SIZE,
        );
        let last_value = db
            .query_with_params(&query, &params)?
            .first()
            .and_then(|row| row.get("last_value"));

        if last_value.is_none() {
            break;
        }

        cursor = last_value
    }

    Ok(())
}

fn get_primary_key_columns_for_table(
    db: &mut dyn Conn,
    table: &str,
) -> anyhow::Result<Vec<String>> {
    // Query from https://wiki.postgresql.org/wiki/Retrieve_primary_key_columns
    let primary_key_columns: Vec<String> = db
        .query(&format!(
            "
            SELECT a.attname AS column_name
            FROM   pg_index i
            JOIN   pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
            WHERE  i.indrelid = '{table}'::regclass
            AND    i.indisprimary;
            ",
            table = table
        ))?
        .iter()
        .map(|row| row.get("column_name"))
        .collect();

    Ok(primary_key_columns)
}

pub struct Index {
    pub name: String,
    pub oid: u32,
}

// Finds all indices which reference a column, either as a key or included column or
// within an expression or predicate. Postgres records a dependency from an index on
// each column it references, except for indices backing a constraint which depend on
// the constraint instead, so those are found through the key columns.
pub fn get_indices_for_column(
    db: &mut dyn Conn,
    table: &str,
    column: &str,
) -> anyhow::Result<Vec<Index>> {
    let indices = db
        .query(&format!(
            "
            SELECT DISTINCT
                i.relname AS name,
                i.oid AS oid
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_class i ON i.oid = ix.indexrelid
            JOIN pg_attribute a ON
                a.attrelid = t.oid AND
                a.attname = '{column}'
            WHERE
                t.relname = '{table}' AND
                (
                    a.attnum = ANY(ix.indkey) OR
                    EXISTS (
                        SELECT 1
                        FROM pg_depend d
                        WHERE
                            d.classid = 'pg_class'::regclass AND
                            d.objid = i.oid AND
                            d.refclassid = 'pg_class'::regclass AND
                            d.refobjid = t.oid AND
                            d.refobjsubid = a.attnum
                    )
                )
            ORDER BY i.relname
            ",
            table = table,
            column = column,
        ))?
        .iter()
        .map(|row| Index {
            name: row.get("name"),
            oid: row.get("oid"),
        })
        .collect();

    Ok(indices)
}

// The CREATE INDEX statement which defines an index
pub fn get_index_definition(db: &mut dyn Conn, index_oid: u32) -> anyhow::Result<String> {
    db.query(&format!(
        "SELECT pg_get_indexdef({index_oid}) AS definition"
    ))?
    .first()
    .map(|row| row.get("definition"))
    .ok_or_else(|| anyhow!("failed to get definition of index {}", index_oid))
}

// The row being written by a trigger with the columns under their current names, for
// use as `SELECT {selection} INTO record`. Together with `table_with_current_columns`
// this lets user-provided SQL reference both tables of a cross-table transformation by
// the names they have in this migration, even when the real columns differ.
pub fn new_row_with_current_columns(table: &Table) -> String {
    table
        .columns
        .iter()
        .map(|column| format!("NEW.\"{}\" AS \"{}\"", column.real_name, column.name))
        .collect::<Vec<String>>()
        .join(", ")
}

// The real table exposed with its columns under their current names, aliased to the
// table's current name, for use in a FROM clause. Includes the ctid so that matched
// rows can be updated.
pub fn table_with_current_columns(table: &Table) -> String {
    let columns = table
        .columns
        .iter()
        .map(|column| format!("\"{}\" AS \"{}\"", column.real_name, column.name))
        .collect::<Vec<String>>()
        .join(", ");

    format!(
        "(SELECT ctid, {columns} FROM public.\"{real_name}\") \"{name}\"",
        columns = columns,
        real_name = table.real_name,
        name = table.name,
    )
}
