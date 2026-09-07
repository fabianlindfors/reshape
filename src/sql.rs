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
        match resolve(&reference, table) {
            Ok(Some(resolution)) => resolutions.push((reference, resolution)),
            Ok(None) => {}
            Err(error) => errors.push(error),
        }
    }

    if !errors.is_empty() {
        return Err(anyhow!(errors.join(", ")));
    }

    let rewritten = splice(&wrapped, &resolutions).map_err(|e| anyhow!(e))?;
    Ok(unwrap_expression(&rewritten))
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
    let parsed = pg_query::parse(sql).map_err(|e| e.to_string())?;
    let tree = serde_json::to_value(&parsed.protobuf).map_err(|e| e.to_string())?;

    let mut references = Vec::new();
    collect_column_references(&tree, &mut references);
    references.sort_by_key(|reference| reference.location);

    Ok(references)
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

/// Resolves a reference against a table. Returns `Ok(None)` for references which can't
/// be checked, such as the `NEW` and `OLD` rows available in triggers.
fn resolve<'a>(
    reference: &ColumnReference,
    table: &'a Table,
) -> Result<Option<Resolution<'a>>, String> {
    let fields = &reference.fields;

    match fields.len() {
        // Unqualified column, e.g. `name`
        1 => {
            let column = find_column(table, &fields[0])?;
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

            if *qualifier == table.name {
                let column = find_column(table, &fields[column_field])?;
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
            if fields.len() == 2 {
                if let Some(column) = table.get_column(qualifier) {
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

fn find_column<'a>(table: &'a Table, name: &str) -> Result<&'a Column, String> {
    table.get_column(name).ok_or_else(|| {
        format!(
            "column \"{}\" does not exist on table \"{}\"",
            name, table.name
        )
    })
}

/// Replaces the resolved table and column names in the SQL with their real names, leaving
/// everything else untouched.
fn splice(sql: &str, resolutions: &[(ColumnReference, Resolution)]) -> Result<String, String> {
    let scanned = pg_query::scan(sql).map_err(|e| e.to_string())?;
    let tokens: Vec<_> = scanned
        .tokens
        .iter()
        .filter(|token| !matches!(token.token(), Token::SqlComment | Token::CComment))
        .collect();

    let mut replacements: Vec<(usize, usize, String)> = Vec::new();

    for (reference, resolution) in resolutions {
        let first = tokens
            .iter()
            .position(|token| token.start as usize == reference.location)
            .ok_or_else(|| format!("failed to locate reference {}", reference.fields.join(".")))?;

        // The fields of a reference are identifier tokens separated by dots
        let field_token = |field: usize| -> Result<(usize, usize), String> {
            let index = first + field * 2;
            if field > 0 {
                let separator = tokens.get(index - 1).map(|token| token.token());
                if separator != Some(Token::Ascii46) {
                    return Err(format!(
                        "failed to locate reference {}",
                        reference.fields.join(".")
                    ));
                }
            }
            tokens
                .get(index)
                .map(|token| (token.start as usize, token.end as usize))
                .ok_or_else(|| format!("failed to locate reference {}", reference.fields.join(".")))
        };

        if let Some(table_field) = resolution.table_field {
            let (start, end) = field_token(table_field)?;
            replacements.push((start, end, quote_identifier(&resolution.table.real_name)));
        }

        let (start, end) = field_token(resolution.column_field)?;
        replacements.push((start, end, quote_identifier(&resolution.column.real_name)));
    }

    replacements.sort_by_key(|(start, _, _)| *start);

    let mut result = String::with_capacity(sql.len());
    let mut position = 0;
    for (start, end, replacement) in replacements {
        result.push_str(&sql[position..start]);
        result.push_str(&replacement);
        position = end;
    }
    result.push_str(&sql[position..]);

    Ok(result)
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
