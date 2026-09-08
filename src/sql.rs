//! Parsing of user-provided SQL, used to validate and rewrite the expressions found in
//! migration files.

use crate::schema::{Column, Table};
use anyhow::anyhow;
use pg_query::protobuf::Token;
use serde_json::Value;

/// Validate a complete SQL statement using pg_query
pub fn validate_sql_statement(sql: &str) -> Result<(), String> {
    pg_query::parse(sql).map(|_| ()).map_err(|e| e.to_string())
}

/// Validate an SQL expression by wrapping it in SELECT
pub fn validate_sql_expression(expr: &str) -> Result<(), String> {
    pg_query::parse(&wrap_expression(expr))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Rewrites the column references in an expression to point at the real columns backing
/// the table. Columns are referenced by the names they currently have in the schema, but
/// the columns they are backed by may have different names, for example a temporary
/// column introduced by `alter_column` or a column which is renamed once the migration
/// completes. The rewritten expression can be run directly against the real table.
///
/// Fails if the expression references a column or table which doesn't exist.
pub fn rewrite_column_references(expression: &str, table: &Table) -> anyhow::Result<String> {
    let wrapped = wrap_expression(expression);
    let references = extract_column_references(&wrapped).map_err(|e| anyhow!(e))?;

    let mut errors = Vec::new();
    let mut resolutions = Vec::new();
    for reference in references {
        match resolve(&reference, &[table]) {
            Ok(Some(resolution)) => resolutions.push((reference, resolution)),
            Ok(None) => {}
            Err(error) => errors.push(error),
        }
    }

    if !errors.is_empty() {
        return Err(anyhow!(errors.join(", ")));
    }

    let tokens = scan_tokens(&wrapped).map_err(|e| anyhow!(e))?;
    let mut replacements = Vec::new();
    for (reference, resolution) in &resolutions {
        replacements.extend(
            reference_replacements(reference, resolution, &tokens).map_err(|e| anyhow!(e))?,
        );
    }

    Ok(unwrap_expression(&apply_replacements(
        &wrapped,
        replacements,
    )))
}

/// Rewrites the definition of an existing index, as returned by `pg_get_indexdef`, into a
/// statement creating an equivalent index under a new name with all column references
/// pointing at the real columns of `table`. Used to duplicate an index onto a temporary
/// column by mapping the current column to the temporary one.
///
/// The new index is created concurrently and only if it doesn't already exist.
pub fn rewrite_index_definition(
    definition: &str,
    table: &Table,
    new_name: &str,
) -> anyhow::Result<String> {
    let tree = parse_tree(definition).map_err(|e| anyhow!(e))?;
    let statement = tree
        .pointer("/stmts/0/stmt/node/IndexStmt")
        .ok_or_else(|| anyhow!("expected a CREATE INDEX statement"))?;
    let tokens = scan_tokens(definition).map_err(|e| anyhow!(e))?;

    let mut replacements = Vec::new();

    // Replace the index name and make the creation concurrent and idempotent
    let name_token = tokens
        .iter()
        .position(|token| token_text(definition, token).eq_ignore_ascii_case("INDEX"))
        .and_then(|position| tokens.get(position + 1))
        .ok_or_else(|| anyhow!("failed to locate index name in definition"))?;
    replacements.push(Replacement {
        start: name_token.start as usize,
        end: name_token.end as usize,
        text: format!("CONCURRENTLY IF NOT EXISTS {}", quote_identifier(new_name)),
    });

    // Columns referenced by expressions and the predicate
    for reference in column_references_in(&tree) {
        if let Some(resolution) = resolve(&reference, &[table]).map_err(|e| anyhow!(e))? {
            replacements.extend(
                reference_replacements(&reference, &resolution, &tokens).map_err(|e| anyhow!(e))?,
            );
        }
    }

    // Plain column entries in the key and INCLUDE lists aren't expressions and carry no
    // location, so they are found by their position in the list instead
    for (keyword, params) in [
        ("USING", "index_params"),
        ("INCLUDE", "index_including_params"),
    ] {
        let names: Vec<Option<&str>> = statement[params]
            .as_array()
            .map(|elements| {
                elements
                    .iter()
                    .map(|element| {
                        element
                            .pointer("/node/IndexElem/name")
                            .and_then(Value::as_str)
                            .filter(|name| !name.is_empty())
                    })
                    .collect()
            })
            .unwrap_or_default();

        if names.iter().any(Option::is_some) {
            replacements.extend(
                index_element_replacements(definition, &tokens, keyword, &names, table)
                    .map_err(|e| anyhow!(e))?,
            );
        }
    }

    Ok(apply_replacements(definition, replacements))
}

/// Checks that every column referenced by an expression exists in one of the given tables.
/// Unqualified columns may belong to any of the tables, qualified ones must match the
/// table they name. Returns one error message per invalid reference.
pub fn validate_column_references(expression: &str, tables: &[&Table]) -> Vec<String> {
    let references = match extract_column_references(&wrap_expression(expression)) {
        Ok(references) => references,
        Err(error) => return vec![error],
    };

    references
        .iter()
        .filter_map(|reference| resolve(reference, tables).err())
        .collect()
}

/// Checks that an expression doesn't reference any columns at all, which is the case for
/// default values.
pub fn validate_no_column_references(expression: &str) -> Result<(), String> {
    let references = extract_column_references(&wrap_expression(expression))?;

    match references.first() {
        Some(reference) => Err(format!(
            "column references are not allowed here, found \"{}\"",
            reference.fields.join(".")
        )),
        None => Ok(()),
    }
}

const EXPRESSION_PREFIX: &str = "SELECT (";
const EXPRESSION_SUFFIX: &str = ")";

fn wrap_expression(expression: &str) -> String {
    format!("{EXPRESSION_PREFIX}{expression}{EXPRESSION_SUFFIX}")
}

fn unwrap_expression(wrapped: &str) -> String {
    wrapped[EXPRESSION_PREFIX.len()..wrapped.len() - EXPRESSION_SUFFIX.len()].to_string()
}

/// A column reference found in an expression, for example `name` or `users.name`
#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnReference {
    /// The dot-separated parts of the reference. Unquoted identifiers have already been
    /// lowercased by the parser, matching how Postgres resolves them.
    fields: Vec<String>,
    /// Byte offset of the first field within the parsed SQL
    location: usize,
}

/// Finds all column references in a SQL string.
///
/// pg_query's own node iterator only visits a subset of node types and misses references
/// inside for example array constructors and subscripts. Instead we walk the serialized
/// parse tree, which is complete. References inside subqueries are skipped as they belong
/// to the subquery's own scope.
fn extract_column_references(sql: &str) -> Result<Vec<ColumnReference>, String> {
    Ok(column_references_in(&parse_tree(sql)?))
}

/// Parses SQL into pg_query's parse tree, serialized so it can be walked generically
fn parse_tree(sql: &str) -> Result<Value, String> {
    let parsed = pg_query::parse(sql).map_err(|e| e.to_string())?;
    serde_json::to_value(&parsed.protobuf).map_err(|e| e.to_string())
}

fn column_references_in(tree: &Value) -> Vec<ColumnReference> {
    let mut references = Vec::new();
    collect_column_references(tree, &mut references);
    references.sort_by_key(|reference| reference.location);
    references
}

fn collect_column_references(node: &Value, references: &mut Vec<ColumnReference>) {
    match node {
        Value::Object(fields) => {
            if fields.contains_key("SubLink") {
                return;
            }

            if let Some(column_ref) = fields.get("ColumnRef") {
                if let Some(reference) = parse_column_ref(column_ref) {
                    references.push(reference);
                }
                return;
            }

            for child in fields.values() {
                collect_column_references(child, references);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_column_references(item, references);
            }
        }
        _ => {}
    }
}

fn parse_column_ref(node: &Value) -> Option<ColumnReference> {
    let location = usize::try_from(node.get("location")?.as_i64()?).ok()?;

    // Each field is either an identifier or a star. References containing a star, such
    // as `users.*`, are not column references and are skipped.
    let fields = node
        .get("fields")?
        .as_array()?
        .iter()
        .map(|field| Some(field.pointer("/node/String/sval")?.as_str()?.to_string()))
        .collect::<Option<Vec<String>>>()?;

    Some(ColumnReference { fields, location })
}

/// The column and table a reference resolves to, along with which of the reference's
/// fields name them.
struct Resolution<'a> {
    table_field: Option<usize>,
    table: &'a Table,
    column_field: usize,
    column: &'a Column,
}

/// Resolves a reference against a set of tables. Returns `Ok(None)` for references which
/// can't be checked, such as the `NEW` and `OLD` rows available in triggers.
fn resolve<'a>(
    reference: &ColumnReference,
    tables: &[&'a Table],
) -> Result<Option<Resolution<'a>>, String> {
    let fields = &reference.fields;

    match fields.len() {
        // Unqualified column, e.g. `name`. With more than one table in scope, the name
        // could resolve to a different table depending on where the SQL runs, so it
        // must be qualified.
        1 => {
            if tables.len() > 1 {
                return Err(format!(
                    "column \"{}\" must be qualified with a table name",
                    fields[0]
                ));
            }

            let (table, column) = find_column(tables, &fields[0])?;
            Ok(Some(Resolution {
                table_field: None,
                table,
                column_field: 0,
                column,
            }))
        }
        // Qualified column, e.g. `users.name` or `public.users.name`
        2 | 3 => {
            let qualifier_field = fields.len() - 2;
            let column_field = fields.len() - 1;
            let qualifier = &fields[qualifier_field];

            if let Some(table) = tables.iter().find(|table| table.name == *qualifier) {
                let (table, column) = find_column(&[table], &fields[column_field])?;
                return Ok(Some(Resolution {
                    table_field: Some(qualifier_field),
                    table,
                    column_field,
                    column,
                }));
            }

            if qualifier == "new" || qualifier == "old" {
                return Ok(None);
            }

            // A two-part reference can also be a field of a composite-typed column, e.g.
            // `address.city`. In that case, the first part is the column reference.
            if fields.len() == 2 && tables.len() == 1 {
                if let Ok((table, column)) = find_column(tables, qualifier) {
                    return Ok(Some(Resolution {
                        table_field: None,
                        table,
                        column_field: 0,
                        column,
                    }));
                }
            }

            Err(format!("unknown table \"{}\"", qualifier))
        }
        _ => Ok(None),
    }
}

fn find_column<'a>(tables: &[&'a Table], name: &str) -> Result<(&'a Table, &'a Column), String> {
    tables
        .iter()
        .find_map(|table| table.get_column(name).map(|column| (*table, column)))
        .ok_or_else(|| {
            let table_names: Vec<String> = tables
                .iter()
                .map(|table| format!("\"{}\"", table.name))
                .collect();
            let noun = if tables.len() == 1 { "table" } else { "tables" };
            format!(
                "column \"{}\" does not exist on {} {}",
                name,
                noun,
                table_names.join(" or ")
            )
        })
}

/// A piece of SQL text to be replaced
struct Replacement {
    start: usize,
    end: usize,
    text: String,
}

fn scan_tokens(sql: &str) -> Result<Vec<pg_query::protobuf::ScanToken>, String> {
    let scanned = pg_query::scan(sql).map_err(|e| e.to_string())?;
    Ok(scanned
        .tokens
        .into_iter()
        .filter(|token| !matches!(token.token(), Token::SqlComment | Token::CComment))
        .collect())
}

fn token_text<'a>(sql: &'a str, token: &pg_query::protobuf::ScanToken) -> &'a str {
    &sql[token.start as usize..token.end as usize]
}

/// Replacements of the resolved table and column names of a reference with their real
/// names. The fields of a reference are identifier tokens separated by dots, starting at
/// the reference's location.
fn reference_replacements(
    reference: &ColumnReference,
    resolution: &Resolution,
    tokens: &[pg_query::protobuf::ScanToken],
) -> Result<Vec<Replacement>, String> {
    let not_found = || format!("failed to locate reference {}", reference.fields.join("."));

    let first = tokens
        .iter()
        .position(|token| token.start as usize == reference.location)
        .ok_or_else(not_found)?;

    let field_token = |field: usize| -> Result<&pg_query::protobuf::ScanToken, String> {
        let index = first + field * 2;
        if field > 0 {
            let separator = tokens.get(index - 1).map(|token| token.token());
            if separator != Some(Token::Ascii46) {
                return Err(not_found());
            }
        }
        tokens.get(index).ok_or_else(not_found)
    };

    let mut replacements = Vec::new();

    if let Some(table_field) = resolution.table_field {
        let token = field_token(table_field)?;
        replacements.push(Replacement {
            start: token.start as usize,
            end: token.end as usize,
            text: quote_identifier(&resolution.table.real_name),
        });
    }

    let token = field_token(resolution.column_field)?;
    replacements.push(Replacement {
        start: token.start as usize,
        end: token.end as usize,
        text: quote_identifier(&resolution.column.real_name),
    });

    Ok(replacements)
}

/// Replacements for the plain column entries of an index column list, which starts with
/// the first parenthesis after `keyword`. `names` holds the column name of each entry in
/// the list, or `None` for entries which are expressions.
fn index_element_replacements(
    sql: &str,
    tokens: &[pg_query::protobuf::ScanToken],
    keyword: &str,
    names: &[Option<&str>],
    table: &Table,
) -> Result<Vec<Replacement>, String> {
    let keyword_position = tokens
        .iter()
        .position(|token| token_text(sql, token).eq_ignore_ascii_case(keyword))
        .ok_or_else(|| format!("failed to locate {} in index definition", keyword))?;
    let list_start = tokens[keyword_position..]
        .iter()
        .position(|token| token.token() == Token::Ascii40)
        .map(|offset| keyword_position + offset)
        .ok_or_else(|| format!("failed to locate {} list in index definition", keyword))?;

    let mut replacements = Vec::new();
    let mut depth = 0;
    let mut element = 0;
    let mut at_element_start = false;

    for token in &tokens[list_start..] {
        match token.token() {
            Token::Ascii40 if depth == 0 => {
                depth += 1;
                at_element_start = true;
                continue;
            }
            Token::Ascii40 => depth += 1,
            Token::Ascii41 => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Token::Ascii44 if depth == 1 => {
                element += 1;
                at_element_start = true;
                continue;
            }
            _ => {}
        }

        if !at_element_start {
            continue;
        }
        at_element_start = false;

        if let Some(Some(name)) = names.get(element) {
            let text = token_text(sql, token);
            if !identifier_matches(text, name) {
                return Err(format!(
                    "expected column \"{}\" in index definition, found {}",
                    name, text
                ));
            }

            let (_, column) = find_column(&[table], name)?;
            replacements.push(Replacement {
                start: token.start as usize,
                end: token.end as usize,
                text: quote_identifier(&column.real_name),
            });
        }
    }

    Ok(replacements)
}

/// Whether an identifier token, quoted or not, names `name`
fn identifier_matches(token: &str, name: &str) -> bool {
    match token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(quoted) => quoted.replace("\"\"", "\"") == name,
        None => token == name || token.to_lowercase() == name,
    }
}

fn apply_replacements(sql: &str, mut replacements: Vec<Replacement>) -> String {
    replacements.sort_by_key(|replacement| replacement.start);

    let mut result = String::with_capacity(sql.len());
    let mut position = 0;
    for replacement in replacements {
        result.push_str(&sql[position..replacement.start]);
        result.push_str(&replacement.text);
        position = replacement.end;
    }
    result.push_str(&sql[position..]);

    result
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, real_name: &str) -> Column {
        Column {
            name: name.to_string(),
            real_name: real_name.to_string(),
            data_type: "TEXT".to_string(),
            nullable: true,
            default: None,
        }
    }

    // A table where `status` is backed by a temporary column and `state` was renamed
    // from `old_state`. `id` and `address` are unchanged.
    fn table() -> Table {
        Table {
            name: "users".to_string(),
            real_name: "users".to_string(),
            columns: vec![
                column("id", "id"),
                column("status", "__reshape_0_0_status"),
                column("state", "old_state"),
                column("address", "address"),
            ],
        }
    }

    fn rewrite(expression: &str) -> String {
        rewrite_column_references(expression, &table()).unwrap()
    }

    fn rewrite_error(expression: &str) -> String {
        rewrite_column_references(expression, &table())
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn rewrites_columns_to_real_names() {
        assert_eq!(rewrite("status"), r#""__reshape_0_0_status""#);
        assert_eq!(rewrite("state"), r#""old_state""#);
        assert_eq!(rewrite("id"), r#""id""#);
    }

    #[test]
    fn preserves_formatting_and_rest_of_expression() {
        assert_eq!(
            rewrite("status =   'active' AND  id > 10"),
            r#""__reshape_0_0_status" =   'active' AND  "id" > 10"#
        );
        assert_eq!(
            rewrite("coalesce(lower(status), 'none')"),
            r#"coalesce(lower("__reshape_0_0_status"), 'none')"#
        );
    }

    #[test]
    fn rewrites_qualified_references() {
        assert_eq!(rewrite("users.status"), r#""users"."__reshape_0_0_status""#);
        assert_eq!(
            rewrite("public.users.status"),
            r#"public."users"."__reshape_0_0_status""#
        );
    }

    #[test]
    fn rewrites_table_name_to_real_name() {
        let mut table = table();
        table.name = "customers".to_string();

        assert_eq!(
            rewrite_column_references("customers.status", &table).unwrap(),
            r#""users"."__reshape_0_0_status""#
        );
    }

    #[test]
    fn handles_quoted_and_case_folded_identifiers() {
        // Unquoted identifiers are lowercased by Postgres, quoted ones are not
        assert_eq!(rewrite("STATUS"), r#""__reshape_0_0_status""#);
        assert_eq!(rewrite(r#""status""#), r#""__reshape_0_0_status""#);
        assert!(rewrite_error(r#""Status""#).contains(r#"column "Status" does not exist"#));
    }

    #[test]
    fn rewrites_references_missed_by_pg_query_node_iterator() {
        assert_eq!(
            rewrite("ARRAY[status, state]"),
            r#"ARRAY["__reshape_0_0_status", "old_state"]"#
        );
        assert_eq!(rewrite("status[1]"), r#""__reshape_0_0_status"[1]"#);
        assert_eq!(
            rewrite(r#"status COLLATE "C""#),
            r#""__reshape_0_0_status" COLLATE "C""#
        );
        assert_eq!(rewrite("(address).city"), r#"("address").city"#);
    }

    #[test]
    fn leaves_subqueries_untouched() {
        assert_eq!(
            rewrite("status = (SELECT status FROM other WHERE other.id = 1)"),
            r#""__reshape_0_0_status" = (SELECT status FROM other WHERE other.id = 1)"#
        );
        assert_eq!(
            rewrite("EXISTS (SELECT 1 FROM t WHERE t.missing = 1) AND id > 0"),
            r#"EXISTS (SELECT 1 FROM t WHERE t.missing = 1) AND "id" > 0"#
        );
    }

    #[test]
    fn treats_composite_field_access_as_column_reference() {
        assert_eq!(rewrite("address.city"), r#""address".city"#);
    }

    #[test]
    fn skips_trigger_rows_and_stars() {
        assert_eq!(rewrite("NEW.status"), "NEW.status");
        assert_eq!(rewrite("OLD.status"), "OLD.status");
        assert_eq!(rewrite("count(*)"), "count(*)");
    }

    #[test]
    fn ignores_non_column_identifiers() {
        assert_eq!(rewrite("now()"), "now()");
        assert_eq!(rewrite("CURRENT_TIMESTAMP"), "CURRENT_TIMESTAMP");
        assert_eq!(rewrite("'status'"), "'status'");
        assert_eq!(rewrite("status::text"), r#""__reshape_0_0_status"::text"#);
        assert_eq!(
            rewrite("EXTRACT(year FROM status)"),
            r#"EXTRACT(year FROM "__reshape_0_0_status")"#
        );
    }

    #[test]
    fn fails_for_unknown_columns() {
        assert_eq!(
            rewrite_error("missing"),
            r#"column "missing" does not exist on table "users""#
        );
        assert_eq!(
            rewrite_error("users.missing"),
            r#"column "missing" does not exist on table "users""#
        );
        assert_eq!(
            rewrite_error("missing OR also_missing"),
            r#"column "missing" does not exist on table "users", column "also_missing" does not exist on table "users""#
        );
    }

    #[test]
    fn fails_for_unknown_tables() {
        assert_eq!(rewrite_error("other.status"), r#"unknown table "other""#);
    }

    #[test]
    fn fails_for_invalid_sql() {
        assert!(rewrite_column_references("INVALID $$$", &table()).is_err());
    }

    fn other_table() -> Table {
        Table {
            name: "profiles".to_string(),
            real_name: "profiles".to_string(),
            columns: vec![column("user_id", "user_id"), column("email", "email")],
        }
    }

    #[test]
    fn validates_references_against_multiple_tables() {
        let users = table();
        let profiles = other_table();
        let tables = [&users, &profiles];

        // Every reference must be qualified when more than one table is in scope
        assert!(validate_column_references("profiles.user_id = users.id", &tables).is_empty());
        assert!(
            validate_column_references("lower(profiles.email) = users.status", &tables).is_empty()
        );

        assert_eq!(
            validate_column_references("user_id = users.id", &tables),
            vec![r#"column "user_id" must be qualified with a table name"#]
        );
        assert_eq!(
            validate_column_references("profiles.missing = users.id", &tables),
            vec![r#"column "missing" does not exist on table "profiles""#]
        );
        assert_eq!(
            validate_column_references("users.email = users.id", &tables),
            vec![r#"column "email" does not exist on table "users""#]
        );
        assert_eq!(
            validate_column_references("accounts.id = users.id", &tables),
            vec![r#"unknown table "accounts""#]
        );
    }

    #[test]
    fn validation_reports_all_errors() {
        assert_eq!(
            validate_column_references("a + b", &[&table()]),
            vec![
                r#"column "a" does not exist on table "users""#,
                r#"column "b" does not exist on table "users""#,
            ]
        );
    }

    #[test]
    fn validation_reports_invalid_sql() {
        assert_eq!(validate_column_references("$$$", &[&table()]).len(), 1);
    }

    #[test]
    fn validates_absence_of_column_references() {
        assert!(validate_no_column_references("now()").is_ok());
        assert!(validate_no_column_references("'active'").is_ok());
        assert!(validate_no_column_references("nextval('users_id_seq')").is_ok());
        assert!(validate_no_column_references("CURRENT_TIMESTAMP").is_ok());
        assert_eq!(
            validate_no_column_references("lower(name)"),
            Err(r#"column references are not allowed here, found "name""#.to_string())
        );
        assert!(validate_no_column_references("$$$").is_err());
    }

    fn index_table() -> Table {
        let mut table = table();
        table.columns.push(column("name", "name"));
        table.columns.push(column("Mixed Case", "Mixed Case"));
        table
    }

    #[test]
    fn rewrites_index_definitions() {
        let table = index_table();
        let cases = [
            (
                "CREATE INDEX users_active_idx ON public.users USING btree (id) WHERE (status = 'active'::text)",
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS "tmp" ON public.users USING btree ("id") WHERE ("__reshape_0_0_status" = 'active'::text)"#,
            ),
            (
                "CREATE INDEX users_lower_idx ON public.users USING btree (lower(status))",
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS "tmp" ON public.users USING btree (lower("__reshape_0_0_status"))"#,
            ),
            (
                "CREATE INDEX users_plain_idx ON public.users USING btree (status text_pattern_ops, name)",
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS "tmp" ON public.users USING btree ("__reshape_0_0_status" text_pattern_ops, "name")"#,
            ),
            (
                r#"CREATE INDEX users_quoted_idx ON public.users USING btree ("Mixed Case") WHERE ("Mixed Case" IS NOT NULL)"#,
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS "tmp" ON public.users USING btree ("Mixed Case") WHERE ("Mixed Case" IS NOT NULL)"#,
            ),
            (
                "CREATE UNIQUE INDEX users_mixed_idx ON public.users USING btree (id, lower(status) DESC NULLS LAST) INCLUDE (name) WITH (fillfactor='70')",
                r#"CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS "tmp" ON public.users USING btree ("id", lower("__reshape_0_0_status") DESC NULLS LAST) INCLUDE ("name") WITH (fillfactor='70')"#,
            ),
            (
                "CREATE INDEX users_concat_idx ON public.users USING btree (((status || name)), id)",
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS "tmp" ON public.users USING btree ((("__reshape_0_0_status" || "name")), "id")"#,
            ),
        ];

        for (definition, expected) in cases {
            assert_eq!(
                rewrite_index_definition(definition, &table, "tmp").unwrap(),
                expected,
                "definition: {}",
                definition
            );
        }
    }

    #[test]
    fn index_definition_with_unknown_column_fails() {
        let error = rewrite_index_definition(
            "CREATE INDEX idx ON public.users USING btree (missing)",
            &index_table(),
            "tmp",
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, r#"column "missing" does not exist on table "users""#);

        let error = rewrite_index_definition(
            "CREATE INDEX idx ON public.users USING btree (id) WHERE (missing IS NULL)",
            &index_table(),
            "tmp",
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, r#"column "missing" does not exist on table "users""#);
    }

    #[test]
    fn index_definition_must_be_create_index() {
        assert!(rewrite_index_definition("SELECT 1", &index_table(), "tmp").is_err());
    }

    #[test]
    fn quotes_identifiers_with_quotes() {
        let mut table = table();
        table.columns.push(column("weird", r#"we"ird"#));

        assert_eq!(
            rewrite_column_references("weird", &table).unwrap(),
            r#""we""ird""#
        );
    }
}
