use super::{
    quote_string_literal, Action, MigrationContext, NameField, References, SqlField, TableScope,
};
use crate::{
    db::{Conn, Transaction},
    schema::{Schema, Table},
    sql::rewrite_column_references,
};
use anyhow::{anyhow, bail, Context};
use postgres::error::SqlState;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

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

    pub comment: Option<String>,
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
    // Expressions and predicates reference columns by their current names but the index
    // is created on the real table, so they are rewritten to the real column names.
    fn column_definitions(&self, table: &Table) -> anyhow::Result<Vec<String>> {
        self.columns
            .iter()
            .map(|column| {
                let (target, direction, nulls) = match column {
                    IndexColumn::Name(name) => {
                        (quoted_real_column_name(table, name)?, &None, &None)
                    }
                    IndexColumn::Column(spec) => (
                        quoted_real_column_name(table, &spec.column)?,
                        &spec.direction,
                        &spec.nulls,
                    ),
                    IndexColumn::Expression(spec) => (
                        format!(
                            "({})",
                            rewrite_column_references(&spec.expression, table).with_context(
                                || format!("invalid index expression: {}", spec.expression)
                            )?
                        ),
                        &spec.direction,
                        &spec.nulls,
                    ),
                };

                Ok(format!("{} {}", target, sort_definition(direction, nulls))
                    .trim_end()
                    .to_string())
            })
            .collect()
    }

    fn where_definition(&self, table: &Table) -> anyhow::Result<String> {
        match &self.r#where {
            Some(predicate) => {
                let rewritten = rewrite_column_references(predicate, table)
                    .with_context(|| format!("invalid index predicate: {}", predicate))?;
                Ok(format!("WHERE {rewritten}"))
            }
            None => Ok("".to_string()),
        }
    }
}

fn quoted_real_column_name(table: &Table, name: &str) -> anyhow::Result<String> {
    Ok(format!("\"{}\"", table.real_column_name(name)?))
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
        ctx: &MigrationContext,
        db: &mut dyn Conn,
        schema: &Schema,
    ) -> anyhow::Result<()> {
        let table = schema.get_table(db, &self.table)?;
        let columns = self.index.column_definitions(&table)?.join(", ");
        let where_def = self.index.where_definition(&table)?;
        let unique = if self.index.unique { "UNIQUE" } else { "" };
        let index_type_def = self
            .index
            .index_type
            .as_ref()
            .map(|kind| format!("USING {kind}"))
            .unwrap_or_default();

        let mut owned = match OwnedIndex::load(db, ctx)? {
            Some(owned) => owned,
            None => {
                // Check before reserving ownership: abort must never touch a conflicting index.
                if relation_oid(db, &self.index.name)?.is_some() {
                    bail!(
                        "index name conflict: public.{} already exists",
                        self.index.name
                    );
                }
                let table_oid = relation_oid(db, &table.real_name)?
                    .ok_or_else(|| anyhow!("index table disappeared"))?;
                let temporary_name = format!("__reshape_idx_{:032x}", rand::random::<u128>());
                if relation_oid(db, &temporary_name)?.is_some() {
                    bail!("temporary index name conflict: {temporary_name}");
                }
                let owned = OwnedIndex {
                    temporary_name,
                    table_oid,
                    index_oid: None,
                    published: false,
                };
                // Commit the random name before CREATE, which can commit even on failure.
                owned.save(db, ctx)?;
                owned
            }
        };

        // Reconcile a previous interrupted attempt before issuing any new DDL.
        match owned.inspect(db, ctx)? {
            Some(index) if index.valid => return self.publish(db, ctx, &mut owned, &index),
            Some(_) => owned.drop_index(db, ctx)?,
            None if owned.index_oid.is_some() => owned.drop_index(db, ctx)?,
            None => {}
        }
        if relation_oid(db, &self.index.name)?.is_some() {
            bail!(
                "index name conflict: public.{} already exists",
                self.index.name
            );
        }

        const MAX_ATTEMPTS: u32 = 10;
        let mut first_error = None;
        for attempt in 0..MAX_ATTEMPTS {
            let create = format!(
                "CREATE {unique} INDEX CONCURRENTLY {name} ON public.{table} {index_type_def} ({columns}) {where_def}",
                name = quote_identifier(&owned.temporary_name),
                table = quote_identifier(&table.real_name),
            );
            match db.run_once(&create) {
                Ok(()) => {
                    let index = owned
                        .inspect(db, ctx)?
                        .ok_or_else(|| anyhow!("created index disappeared"))?;
                    return self.publish(db, ctx, &mut owned, &index);
                }
                Err(error) => {
                    let retryable = error.code() == Some(&SqlState::LOCK_NOT_AVAILABLE);
                    let unknown_outcome = error.as_db_error().is_none_or(|error| {
                        matches!(error.severity(), "FATAL" | "PANIC")
                            || error.code().code().starts_with("08")
                    });
                    first_error.get_or_insert(error);
                    if unknown_outcome {
                        // Never replay DDL on a broken connection. The next invocation holds
                        // the advisory lock and reconciles the persisted name/OID first.
                        return Err(anyhow::Error::new(first_error.take().unwrap()))
                            .context("index creation outcome unknown after connection loss; ownership retained for reconciliation by migrate or abort");
                    }
                    let blockers = blocker_information(db, owned.table_oid);
                    match owned.inspect(db, ctx) {
                        Ok(Some(index)) if index.valid => {
                            return self.publish(db, ctx, &mut owned, &index);
                        }
                        Err(reconcile) => {
                            return Err(anyhow::Error::new(first_error.take().unwrap())).with_context(|| format!(
                                "index reconciliation failed: {reconcile:#}; ownership retained for abort; {blockers}"
                            ));
                        }
                        _ => {}
                    }
                    // Also clean up non-retryable failures, e.g. an invalid unique index
                    // which PostgreSQL may still use to enforce uniqueness.
                    let cleanup = owned.drop_index(db, ctx);
                    if let Err(cleanup) = cleanup {
                        let context = format!(
                            "index creation failed; cleanup failed: {cleanup:#}; ownership retained for abort; {blockers}"
                        );
                        return Err(anyhow::Error::new(first_error.take().unwrap()))
                            .context(context);
                    }
                    if !retryable || attempt + 1 == MAX_ATTEMPTS {
                        return Err(anyhow::Error::new(first_error.take().unwrap())).with_context(|| format!(
                            "index creation failed after {} attempt(s); cleanup succeeded; {blockers}", attempt + 1
                        ));
                    }
                    let delay = (100_u64 << attempt).min(3_200);
                    let jitter = rand::rng().random_range(0..delay / 2);
                    std::thread::sleep(Duration::from_millis(delay + jitter));
                }
            }
        }
        unreachable!()
    }

    fn complete<'a>(
        &self,
        ctx: &MigrationContext,
        db: &'a mut dyn Conn,
    ) -> anyhow::Result<Option<Transaction<'a>>> {
        let mut transaction = db.transaction()?;
        OwnedIndex::forget(&mut transaction, ctx)?;
        Ok(Some(transaction))
    }

    fn update_schema(&self, _ctx: &MigrationContext, _schema: &mut Schema) {}

    fn abort(&self, ctx: &MigrationContext, db: &mut dyn Conn) -> anyhow::Result<()> {
        if let Some(mut owned) = OwnedIndex::load(db, ctx)? {
            owned.drop_index(db, ctx)?;
            OwnedIndex::forget(db, ctx)?;
        }
        Ok(())
    }

    fn sql_fields(&self) -> Vec<SqlField> {
        let mut fields: Vec<SqlField> = self
            .index
            .columns
            .iter()
            .enumerate()
            .filter_map(|(idx, column)| match column {
                IndexColumn::Expression(spec) => Some(SqlField::expression(
                    format!("index.columns[{}].expression", idx),
                    &spec.expression,
                    References::table(&self.table),
                )),
                _ => None,
            })
            .collect();

        if let Some(predicate) = &self.index.r#where {
            fields.push(SqlField::expression(
                "index.where",
                predicate,
                References::table(&self.table),
            ));
        }

        fields
    }

    fn name_fields(&self) -> Vec<NameField> {
        let mut fields = vec![NameField::table("table", &self.table)];

        for (idx, column) in self.index.columns.iter().enumerate() {
            let (name, value) = match column {
                IndexColumn::Name(name) => (format!("index.columns[{}]", idx), name),
                IndexColumn::Column(spec) => {
                    (format!("index.columns[{}].column", idx), &spec.column)
                }
                IndexColumn::Expression(_) => continue,
            };
            fields.push(NameField::column(
                name,
                value,
                TableScope::schema(&self.table),
            ));
        }

        fields
    }
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn relation_oid(db: &mut dyn Conn, name: &str) -> anyhow::Result<Option<u32>> {
    Ok(db.query_with_params(
        "SELECT oid FROM pg_class WHERE relnamespace = 'public'::regnamespace AND relname = $1::name",
        &[&name],
    )?.first().map(|row| row.get(0)))
}

// Names are random, reserved for this action, and saved before any non-transactional
// DDL. Once known, the OID is authoritative, including after a rename or a DROP
// whose acknowledgement was lost. Never infer ownership from the requested name.
#[derive(Serialize, Deserialize)]
struct OwnedIndex {
    temporary_name: String,
    table_oid: u32,
    index_oid: Option<u32>,
    published: bool,
}

struct IndexIdentity {
    name: String,
    namespace: String,
    valid: bool,
}

impl OwnedIndex {
    fn key(ctx: &MigrationContext) -> String {
        format!("{}_add_index", ctx.prefix())
    }

    fn load(db: &mut dyn Conn, ctx: &MigrationContext) -> anyhow::Result<Option<Self>> {
        db.query_with_params(
            "SELECT value FROM reshape.data WHERE key = $1",
            &[&Self::key(ctx)],
        )?
        .first()
        .map(|row| serde_json::from_value(row.get(0)).map_err(Into::into))
        .transpose()
    }

    fn save(&self, db: &mut dyn Conn, ctx: &MigrationContext) -> anyhow::Result<()> {
        db.query_with_params(
            "INSERT INTO reshape.data (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            &[&Self::key(ctx), &serde_json::to_value(self)?],
        )?;
        Ok(())
    }

    fn forget(db: &mut dyn Conn, ctx: &MigrationContext) -> anyhow::Result<()> {
        db.query_with_params(
            "DELETE FROM reshape.data WHERE key = $1",
            &[&Self::key(ctx)],
        )?;
        Ok(())
    }

    fn inspect(
        &mut self,
        db: &mut dyn Conn,
        ctx: &MigrationContext,
    ) -> anyhow::Result<Option<IndexIdentity>> {
        let oid = match self.index_oid {
            Some(oid) => oid,
            None => match relation_oid(db, &self.temporary_name)? {
                Some(oid) => oid,
                None => return Ok(None),
            },
        };
        let rows = db.query_with_params(
            "SELECT c.relname, n.nspname, i.indrelid, i.indisvalid
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             LEFT JOIN pg_index i ON i.indexrelid = c.oid WHERE c.oid = $1",
            &[&oid],
        )?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        if row.get::<_, Option<u32>>("indrelid") != Some(self.table_oid) {
            bail!("index ownership conflict: refusing to modify relation OID {oid}");
        }
        if self.index_oid.is_none() {
            self.index_oid = Some(oid);
            self.save(db, ctx)?;
        }
        Ok(Some(IndexIdentity {
            name: row.get("relname"),
            namespace: row.get("nspname"),
            valid: row.get("indisvalid"),
        }))
    }

    fn drop_index(&mut self, db: &mut dyn Conn, ctx: &MigrationContext) -> anyhow::Result<()> {
        if let Some(index) = self.inspect(db, ctx)? {
            // A concurrent DROP can itself commit partial work. Keep metadata until
            // it succeeds; a later abort will inspect the same OID and resume it.
            db.run_once(&format!(
                "DROP INDEX CONCURRENTLY {}.{}",
                quote_identifier(&index.namespace),
                quote_identifier(&index.name)
            ))?;
        }
        self.index_oid = None;
        self.published = false;
        // Never adopt a replacement at the previous name after observing a missing
        // OID, nor reuse a name after a successful DROP.
        self.temporary_name = format!("__reshape_idx_{:032x}", rand::random::<u128>());
        self.save(db, ctx)?;
        Ok(())
    }
}

impl AddIndex {
    fn publish(
        &self,
        db: &mut dyn Conn,
        ctx: &MigrationContext,
        owned: &mut OwnedIndex,
        index: &IndexIdentity,
    ) -> anyhow::Result<()> {
        if !index.valid {
            bail!("cannot publish an invalid index");
        }
        if owned.published {
            return Ok(());
        }
        let mut transaction = db.transaction()?;
        transaction
            .run_once(&format!(
                "ALTER INDEX {}.{} RENAME TO {}",
                quote_identifier(&index.namespace),
                quote_identifier(&index.name),
                quote_identifier(&self.index.name)
            ))
            .context("failed to publish index (possible index name conflict)")?;
        if let Some(comment) = &self.index.comment {
            transaction.run_once(&format!(
                "COMMENT ON INDEX {}.{} IS {}",
                quote_identifier(&index.namespace),
                quote_identifier(&self.index.name),
                quote_string_literal(comment)
            ))?;
        }
        owned.published = true;
        owned.save(&mut transaction, ctx)?;
        transaction.commit()
    }
}

fn blocker_information(db: &mut dyn Conn, table_oid: u32) -> String {
    // The timed-out wait has ended, so pg_blocking_pids(self) is already empty.
    // Report candidate table lockers and old snapshots rather than claiming these
    // are proven blockers. No application queries are cancelled.
    match db.query_with_params(
        "SELECT DISTINCT a.pid, a.state FROM pg_stat_activity a
         LEFT JOIN pg_locks l ON l.pid = a.pid AND l.relation = $1
         WHERE a.datname = current_database() AND a.pid <> pg_backend_pid()
           AND (l.granted OR a.backend_xmin IS NOT NULL)",
        &[&table_oid],
    ) {
        Ok(rows) => format!(
            "possible blockers (table locks or snapshots): {}",
            rows.iter()
                .map(|row| format!(
                    "pid {} ({})",
                    row.get::<_, i32>("pid"),
                    row.get::<_, Option<String>>("state").unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Err(error) => format!("blocker information unavailable: {error:#}"),
    }
}
