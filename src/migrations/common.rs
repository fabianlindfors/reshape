use anyhow::{anyhow, Context};
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

    let primary_key = get_primary_key_columns_for_table(db, table)?;

    // If no column to touch is passed, we default to the first primary key column (just to make some "update")
    let touched_column = column.unwrap_or(primary_key[0].as_str());

    let primary_key_columns = primary_key.join(", ");
    let primary_key_where = primary_key_match(table, &primary_key, "rows");

    loop {
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();

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

/// The columns of a table's primary key, in key order. Fails if the table has no primary
/// key, which is needed to identify individual rows when backfilling and when checking
/// rows at the end of a transaction.
pub fn get_primary_key_columns_for_table(
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
            WHERE  i.indrelid = 'public.\"{table}\"'::regclass
            AND    i.indisprimary
            ORDER BY array_position(i.indkey::int2[], a.attnum);
            ",
            table = table
        ))
        .context("failed to get primary key columns")?
        .iter()
        .map(|row| row.get("column_name"))
        .collect();

    if primary_key_columns.is_empty() {
        return Err(anyhow!(
            "table \"{}\" has no primary key, which is required to identify its rows",
            table
        ));
    }

    Ok(primary_key_columns)
}

/// A condition matching a row of `table` against the row `other`, such as a CTE or a
/// trigger's `NEW`, on every primary key column
pub fn primary_key_match(table: &str, primary_key: &[String], other: &str) -> String {
    primary_key
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\" = {other}.\"{column}\""))
        .collect::<Vec<String>>()
        .join(" AND ")
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

pub struct CheckConstraint {
    pub name: String,
    pub oid: u32,
    pub expression: String,
}

// Finds all CHECK constraints which reference a column, including ones which span
// several columns. The expression is returned without the surrounding CHECK ( ... ).
pub fn get_check_constraints_for_column(
    db: &mut dyn Conn,
    table: &str,
    column: &str,
) -> anyhow::Result<Vec<CheckConstraint>> {
    let constraints = db
        .query(&format!(
            "
            SELECT
                c.conname AS name,
                c.oid AS oid,
                pg_get_expr(c.conbin, c.conrelid) AS expression
            FROM pg_constraint c
            JOIN pg_class t ON t.oid = c.conrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            JOIN pg_attribute a ON
                a.attrelid = t.oid AND
                a.attname = '{column}'
            WHERE
                c.contype = 'c' AND
                n.nspname = 'public' AND
                t.relname = '{table}' AND
                a.attnum = ANY(c.conkey)
            ORDER BY c.conname
            ",
            table = table,
            column = column,
        ))?
        .iter()
        .map(|row| CheckConstraint {
            name: row.get("name"),
            oid: row.get("oid"),
            expression: row.get("expression"),
        })
        .collect();

    Ok(constraints)
}

// Whether a constraint is one of the temporary NOT NULL checks which `add_column` and
// `alter_column` add to a new column. These are replaced by a real NOT NULL on
// completion and are not constraints which should be preserved.
pub fn is_temporary_not_null_constraint(name: &str) -> bool {
    name.starts_with("__reshape_")
        && (name.ends_with("_alter_column_temporary") || name.contains("_add_column_not_null_"))
}

pub fn check_constraint_exists(
    db: &mut dyn Conn,
    table: &str,
    constraint: &str,
) -> anyhow::Result<bool> {
    let exists = !db
        .query(&format!(
            "
            SELECT 1
            FROM pg_constraint c
            JOIN pg_class t ON t.oid = c.conrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            WHERE
                c.contype = 'c' AND
                n.nspname = 'public' AND
                t.relname = '{table}' AND
                c.conname = '{constraint}'
            ",
            table = table,
            constraint = constraint,
        ))?
        .is_empty();

    Ok(exists)
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

// Renames the NOT NULL constraint on a column, if there is one, to the name Postgres
// would have given it had the column been declared NOT NULL under its current name.
//
// Since Postgres 18, `SET NOT NULL` creates a catalogued constraint named after the
// column at the time. Reshape sets columns NOT NULL while they still have their temporary
// names and renames them afterwards, which would otherwise leave the constraint named
// after the temporary column. Earlier versions don't catalogue NOT NULL constraints, in
// which case this does nothing.
pub fn rename_not_null_constraint(
    db: &mut dyn Conn,
    table: &str,
    column: &str,
) -> anyhow::Result<()> {
    let target_name = bounded_identifier("", &format!("{table}_{column}"), "_not_null");

    let current_name: Option<String> = db
        .query_with_params(
            "
            SELECT c.conname AS name
            FROM pg_constraint c
            JOIN pg_class t ON t.oid = c.conrelid
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(c.conkey)
            WHERE
                c.contype = 'n' AND
                t.relname = $1 AND
                a.attname = $2
            ",
            &[&table, &column],
        )
        .context("failed to get NOT NULL constraint")?
        .first()
        .map(|row| row.get("name"));

    let Some(current_name) = current_name else {
        return Ok(());
    };
    if current_name == target_name {
        return Ok(());
    }

    // Leave the constraint alone if the name is already taken, as the constraint's
    // name is cosmetic and shouldn't stop a migration from completing
    let target_name_taken = !db
        .query_with_params(
            "
            SELECT 1
            FROM pg_constraint c
            JOIN pg_class t ON t.oid = c.conrelid
            WHERE t.relname = $1 AND c.conname = $2
            ",
            &[&table, &target_name],
        )
        .context("failed to check for existing constraint")?
        .is_empty();
    if target_name_taken {
        return Ok(());
    }

    db.run(&format!(
        r#"
        ALTER TABLE "{table}"
        RENAME CONSTRAINT "{current_name}" TO "{target_name}"
        "#,
    ))
    .context("failed to rename NOT NULL constraint")?;

    Ok(())
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

/// The longest identifier Postgres accepts. Longer names are silently cut down to this
/// length, which makes generated names which only differ at the end collide.
pub const MAX_IDENTIFIER_LENGTH: usize = 63;

/// Builds the identifier `{head}{body}{tail}`, shortened to fit within Postgres' limit on
/// identifier length. Names which already fit are returned unchanged. Longer names keep
/// `head` and `tail` intact, as those are the parts which tell related objects apart, and
/// have their `body` cut down with a hash of the complete name in its place so that
/// distinct names stay distinct.
pub fn bounded_identifier(head: &str, body: &str, tail: &str) -> String {
    let full = format!("{head}{body}{tail}");
    if full.len() <= MAX_IDENTIFIER_LENGTH {
        return full;
    }

    let hash = format!("{:08x}", fnv1a_hash(&full) as u32);
    let budget = MAX_IDENTIFIER_LENGTH
        .saturating_sub(head.len() + tail.len() + hash.len() + 1)
        .min(body.len());
    let mut cut = budget;
    while !body.is_char_boundary(cut) {
        cut -= 1;
    }

    format!("{head}{}_{hash}{tail}", &body[..cut])
}

/// Cuts an identifier down to Postgres' limit the same way Postgres does, so that a name
/// built by reshape matches what Postgres reports back, for example in `search_path`.
pub fn truncate_identifier(name: &str) -> String {
    let mut cut = MAX_IDENTIFIER_LENGTH.min(name.len());
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    name[..cut].to_string()
}

// 64-bit FNV-1a. Names are recomputed on every run, so the hash has to be stable
// across processes and versions, which rules out the standard library's hasher.
fn fnv1a_hash(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_identifier_keeps_short_names() {
        assert_eq!(
            "__reshape_0000_0000_add_column_users_email_rev",
            bounded_identifier("__reshape_0000_0000_add_column_", "users_email", "_rev")
        );
    }

    #[test]
    fn bounded_identifier_shortens_long_names() {
        let head = "__reshape_0000_0000_add_column_";
        let body = "organization_membership_profiles_primary_contact_email_address";

        let name = bounded_identifier(head, body, "");
        let reverse = bounded_identifier(head, body, "_rev");

        assert_eq!(MAX_IDENTIFIER_LENGTH, name.len());
        assert_eq!(MAX_IDENTIFIER_LENGTH, reverse.len());
        assert!(name.starts_with(head));
        assert!(reverse.starts_with(head));
        assert!(reverse.ends_with("_rev"));
        assert_ne!(name, reverse);
        assert_eq!(name, bounded_identifier(head, body, ""));
    }

    #[test]
    fn bounded_identifier_tells_apart_names_with_the_same_start() {
        let head = "__reshape_0000_0000_temp_column_";
        let first = bounded_identifier(
            head,
            "organization_membership_profiles_primary_contact_email",
            "",
        );
        let second = bounded_identifier(
            head,
            "organization_membership_profiles_primary_contact_email_address",
            "",
        );
        assert_ne!(first, second);
    }

    #[test]
    fn truncate_identifier_cuts_at_the_limit() {
        assert_eq!("migration_short", truncate_identifier("migration_short"));

        let long = format!("migration_{}", "a".repeat(60));
        assert_eq!(&long[..63], truncate_identifier(&long));

        let multibyte = format!("migration_{}", "ä".repeat(30));
        let truncated = truncate_identifier(&multibyte);
        assert_eq!(62, truncated.len());
        assert!(multibyte.starts_with(&truncated));
    }

    #[test]
    fn bounded_identifier_respects_char_boundaries() {
        let name = bounded_identifier("__reshape_0000_0000_add_column_", &"ä".repeat(40), "_rev");
        assert!(name.len() <= MAX_IDENTIFIER_LENGTH);
        assert!(name.ends_with("_rev"));
    }
}
