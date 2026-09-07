use super::{Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::{Schema, Table},
};
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AddIndex {
    pub table: String,
    pub index: Index,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Index {
    pub name: String,
    pub columns: Vec<IndexColumn>,
    #[serde(default)]
    pub unique: bool,
    #[serde(rename = "type")]
    pub index_type: Option<String>,

    // Predicate for a partial index, without the WHERE keyword
    pub r#where: Option<String>,
}

// A single entry in an index. Can either be a plain column name, a column with an
// explicit sort order or an arbitrary expression.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum IndexColumn {
    Name(String),
    Column(IndexColumnSpec),
    Expression(IndexExpressionSpec),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct IndexColumnSpec {
    pub column: String,
    pub direction: Option<Direction>,
    pub nulls: Option<Nulls>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct IndexExpressionSpec {
    pub expression: String,
    pub direction: Option<Direction>,
    pub nulls: Option<Nulls>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    #[serde(rename = "ASC", alias = "asc")]
    Asc,

    #[serde(rename = "DESC", alias = "desc")]
    Desc,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nulls {
    #[serde(rename = "FIRST", alias = "first")]
    First,

    #[serde(rename = "LAST", alias = "last")]
    Last,
}

fn sort_definition(direction: &Option<Direction>, nulls: &Option<Nulls>) -> String {
    let mut parts: Vec<&str> = Vec::new();

    match direction {
        Some(Direction::Asc) => parts.push("ASC"),
        Some(Direction::Desc) => parts.push("DESC"),
        None => {}
    }

    match nulls {
        Some(Nulls::First) => parts.push("NULLS FIRST"),
        Some(Nulls::Last) => parts.push("NULLS LAST"),
        None => {}
    }

    parts.join(" ")
}

impl Index {
    // Whether the index relies on user-provided SQL, which is passed to Postgres as
    // written rather than having its column references resolved by Reshape.
    fn has_raw_sql(&self) -> bool {
        self.r#where.is_some()
            || self
                .columns
                .iter()
                .any(|column| matches!(column, IndexColumn::Expression(_)))
    }

    fn column_definitions(&self, table: &Table) -> Vec<String> {
        self.columns
            .iter()
            .map(|column| {
                let (target, direction, nulls) = match column {
                    IndexColumn::Name(name) => (real_column_name(table, name), &None, &None),
                    IndexColumn::Column(spec) => (
                        real_column_name(table, &spec.column),
                        &spec.direction,
                        &spec.nulls,
                    ),
                    IndexColumn::Expression(spec) => (
                        format!("({})", spec.expression),
                        &spec.direction,
                        &spec.nulls,
                    ),
                };

                format!("{} {}", target, sort_definition(direction, nulls))
                    .trim_end()
                    .to_string()
            })
            .collect()
    }
}

fn real_column_name(table: &Table, name: &str) -> String {
    let real_name = table
        .get_column(name)
        .map(|column| column.real_name.as_ref())
        .unwrap_or(name);

    format!("\"{}\"", real_name)
}

// Name of the temporary view used to work out which columns an index references.
// Temporary views are session scoped, so this can't collide with another Reshape run.
const PROBE_VIEW_NAME: &str = "__reshape_index_probe";

impl AddIndex {
    // Works out which columns of the table the index's expressions and predicate
    // reference, by declaring them as a temporary view and asking Postgres what that view
    // depends on. This gets the resolution exactly right without Reshape having to parse
    // the SQL, and unlike creating the index it doesn't scan the table.
    //
    // Returns None if the probe can't be created, for example because the SQL references
    // a column which doesn't exist. The CREATE INDEX statement will fail with the same
    // error, which is the more useful place for it to surface.
    fn referenced_columns(&self, db: &mut dyn Conn, table: &Table) -> Option<Vec<String>> {
        let selects: Vec<String> = self
            .index
            .columns
            .iter()
            .enumerate()
            .map(|(idx, column)| {
                let target = match column {
                    IndexColumn::Name(name) => real_column_name(table, name),
                    IndexColumn::Column(spec) => real_column_name(table, &spec.column),
                    IndexColumn::Expression(spec) => format!("({})", spec.expression),
                };

                format!("{target} AS c{idx}")
            })
            .collect();

        let where_def = match &self.index.r#where {
            Some(predicate) => format!("WHERE {predicate}"),
            None => "".to_string(),
        };

        let drop_query = format!(r#"DROP VIEW IF EXISTS pg_temp."{PROBE_VIEW_NAME}""#);
        db.run(&drop_query).ok()?;
        db.run(&format!(
            r#"
            CREATE VIEW pg_temp."{view}" AS
                SELECT {selects}
                FROM public."{table}"
                {where_def}
            "#,
            view = PROBE_VIEW_NAME,
            selects = selects.join(", "),
            table = table.real_name,
        ))
        .ok()?;

        let columns = db
            .query(&format!(
                r#"
                SELECT DISTINCT attribute.attname AS name
                FROM pg_depend dependency
                JOIN pg_rewrite rule ON rule.oid = dependency.objid
                JOIN pg_attribute attribute ON
                    attribute.attrelid = dependency.refobjid AND
                    attribute.attnum = dependency.refobjsubid
                WHERE rule.ev_class = 'pg_temp."{view}"'::regclass
                    AND dependency.refclassid = 'pg_class'::regclass
                    AND dependency.refobjid = 'public."{table}"'::regclass
                    AND dependency.refobjsubid > 0
                "#,
                view = PROBE_VIEW_NAME,
                table = table.real_name,
            ))
            .map(|rows| rows.iter().map(|row| row.get("name")).collect());

        db.run(&drop_query).ok()?;

        columns.ok()
    }
}

#[typetag::serde(name = "add_index")]
impl Action for AddIndex {
    fn describe(&self) -> String {
        format!(
            "Adding index \"{}\" to table \"{}\"",
            self.index.name, self.table
        )
    }

    fn run(
        &self,
        _ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;

        // Expressions and predicates are passed to Postgres as written, which means they
        // reference the real columns of the table. Most of the time that's fine, but a
        // column which is being replaced in this migration will be dropped on completion,
        // taking any index built on it along with it. That would leave the migration
        // looking successful with the index silently gone, so reject it instead.
        if self.index.has_raw_sql() {
            if let Some(columns) = self.referenced_columns(db, &table) {
                for column in columns {
                    if !table
                        .columns
                        .iter()
                        .any(|surviving| surviving.real_name == column)
                    {
                        bail!(
                            "index \"{index}\" references column \"{column}\" of table \
                             \"{table}\" in an expression or a WHERE predicate, but that column \
                             is being replaced or removed in this migration. The index would be \
                             dropped along with the column when the migration is completed. Move \
                             the index to a later migration.",
                            index = self.index.name,
                            column = column,
                            table = table.name,
                        );
                    }
                }
            }
        }

        let unique = if self.index.unique { "UNIQUE" } else { "" };
        let index_type_def = if let Some(index_type) = &self.index.index_type {
            format!("USING {index_type}")
        } else {
            "".to_string()
        };
        let where_def = if let Some(predicate) = &self.index.r#where {
            format!("WHERE {predicate}")
        } else {
            "".to_string()
        };

        db.run(&format!(
            r#"
			CREATE {unique} INDEX CONCURRENTLY "{name}" ON "{table}" {index_type_def} ({columns}) {where_def}
			"#,
            name = self.index.name,
            table = table.real_name,
            columns = self.index.column_definitions(&table).join(", "),
        ))
        .context("failed to create index")?;
        Ok(())
    }

    fn complete<'a>(
        &self,
        _ctx: &MigrationContext,
        _db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        Ok(None)
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, _ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        db.run(&format!(
            r#"
			DROP INDEX CONCURRENTLY IF EXISTS "{name}"
			"#,
            name = self.index.name,
        ))
        .context("failed to drop index")?;
        Ok(())
    }
}
