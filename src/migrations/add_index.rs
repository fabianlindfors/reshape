use super::{validate_sql_expression, Action, MigrationContext};
use crate::{
    db::{Conn, Transaction},
    schema::{Schema, Table},
};
use anyhow::Context;
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

    fn validate_sql(&self) -> Vec<(String, String, String)> {
        let mut errors = vec![];

        // Validate index expressions. Plain column names and sort options are not
        // SQL expressions and don't need validation.
        for (idx, column) in self.index.columns.iter().enumerate() {
            if let IndexColumn::Expression(spec) = column {
                if let Err(e) = validate_sql_expression(&spec.expression) {
                    errors.push((
                        format!("index.columns[{}].expression", idx),
                        spec.expression.clone(),
                        e,
                    ));
                }
            }
        }

        // Validate partial index predicate
        if let Some(predicate) = &self.index.r#where {
            if let Err(e) = validate_sql_expression(predicate) {
                errors.push(("index.where".to_string(), predicate.clone(), e));
            }
        }

        errors
    }
}
