use postgres::types::{FromSql, ToSql};
use serde::{Deserialize, Serialize};

use crate::{db::Conn, schema::Table};

#[derive(Serialize, Deserialize, Debug)]
pub struct Column {
    pub name: String,

    #[serde(rename = "type")]
    pub data_type: String,

    #[serde(default = "nullable_default")]
    pub nullable: bool,

    pub default: Option<String>,
}

fn nullable_default() -> bool {
    true
}

pub fn add_is_new_column_query(table: &str) -> String {
    format!(
        "
        ALTER TABLE {table}
        ADD COLUMN IF NOT EXISTS __reshape_is_new BOOLEAN DEFAULT FALSE NOT NULL;
        ",
        table = table,
    )
}

pub fn drop_is_new_column_query(table: &str) -> String {
    format!(
        "
        ALTER TABLE {table}
        DROP COLUMN IF EXISTS __reshape_is_new;
        ",
        table = table,
    )
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

const BATCH_SIZE: u16 = 1000;

pub fn batch_update(
    db: &mut dyn Conn,
    table: &Table,
    column: &str,
    up: &str,
) -> anyhow::Result<()> {
    let mut cursor: Option<PostgresRawValue> = None;

    loop {
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();

        let primary_key_columns = table.primary_key.join(", ");

        let primary_key_where = table
            .primary_key
            .iter()
            .map(|column| {
                format!(
                    "{table}.{column} = rows.{column}",
                    table = table.name,
                    column = column,
                )
            })
            .collect::<Vec<String>>()
            .join(" AND ");

        let returning_columns = table
            .primary_key
            .iter()
            .map(|column| format!("rows.{}", column))
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
            "
                WITH rows AS (
                    SELECT {primary_key_columns}
                    FROM {table}
                    {cursor_where}
                    ORDER BY {primary_key_columns}
                    LIMIT {batch_size}
                ), update AS (
                    UPDATE {table}
                    SET {temp_column} = {up}
                    FROM rows
                    WHERE {primary_key_where}
                    RETURNING {returning_columns}
                )
                SELECT LAST_VALUE(({primary_key_columns})) OVER () AS last_value
                FROM update
                LIMIT 1
			    ",
            table = table.name,
            primary_key_columns = primary_key_columns,
            cursor_where = cursor_where,
            batch_size = BATCH_SIZE,
            temp_column = column,
            up = up,
            primary_key_where = primary_key_where,
            returning_columns = returning_columns,
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
