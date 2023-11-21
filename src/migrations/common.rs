use anyhow::anyhow;
use postgres::types::{FromSql, ToSql};
use serde::{Deserialize, Serialize};

use crate::db::Conn;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
    #[serde(default = "nullable_default")]
    pub nullable: bool,
    pub default: Option<String>,
    pub generated: Option<String>,
}

fn nullable_default() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
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
    pub unique: bool,
    pub index_type: String,
}

pub fn get_indices_for_column(
    db: &mut dyn Conn,
    table: &str,
    column: &str,
) -> anyhow::Result<Vec<Index>> {
    let indices = db
        .query(&format!(
            "
            SELECT
                i.relname AS name,
                i.oid AS oid,
                ix.indisunique AS unique,
                am.amname AS type
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_class i ON i.oid = ix.indexrelid
            JOIN pg_am am ON i.relam = am.oid
            JOIN pg_attribute a ON
                a.attrelid = t.oid AND
                a.attnum = ANY(ix.indkey)
            WHERE
                t.relname = '{table}' AND
                a.attname = '{column}'
            ",
            table = table,
            column = column,
        ))?
        .iter()
        .map(|row| Index {
            name: row.get("name"),
            oid: row.get("oid"),
            unique: row.get("unique"),
            index_type: row.get("type"),
        })
        .collect();

    Ok(indices)
}

pub fn get_index_columns(db: &mut dyn Conn, index_name: &str) -> anyhow::Result<Vec<String>> {
    // Get all columns which are part of the index in order
    let (table_oid, column_nums) = db
        .query(&format!(
            "
            SELECT t.oid AS table_oid, ix.indkey::INTEGER[] AS columns
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_class i ON i.oid = ix.indexrelid
            WHERE
	            i.relname = '{index_name}'
            ",
            index_name = index_name,
        ))?
        .first()
        .map(|row| {
            (
                row.get::<'_, _, u32>("table_oid"),
                row.get::<'_, _, Vec<i32>>("columns"),
            )
        })
        .ok_or_else(|| anyhow!("failed to get columns for index"))?;

    // Get the name of each of the columns, still in order
    column_nums
        .iter()
        .map(|column_num| -> anyhow::Result<String> {
            let name: String = db
                .query(&format!(
                    "
                    SELECT attname AS name
                    FROM pg_attribute
                    WHERE attrelid = {table_oid}
                        AND attnum = {column_num};
                    ",
                    table_oid = table_oid,
                    column_num = column_num,
                ))?
                .first()
                .map(|row| row.get("name"))
                .ok_or_else(|| anyhow!("expected to find column"))?;

            Ok(name)
        })
        .collect::<anyhow::Result<Vec<String>>>()
}
