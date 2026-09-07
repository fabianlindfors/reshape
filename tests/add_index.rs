mod common;
use common::Test;

#[test]
fn add_index() {
    let mut test = Test::new("Add index");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "name"
            type = "TEXT"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_users_name_index"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "name_idx"
            columns = ["name"]        
        "#,
    );

    test.intermediate(|db, _| {
        // Ensure index is valid and ready
        let (is_ready, is_valid): (bool, bool) = db
            .query(
                "
                SELECT pg_index.indisready, pg_index.indisvalid
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                WHERE pg_class.relname = 'name_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| (row.get("indisready"), row.get("indisvalid")))
            .unwrap();

        assert!(is_ready, "expected index to be ready");
        assert!(is_valid, "expected index to be valid");
    });

    test.after_completion(|db| {
        // Ensure index is valid and ready
        let (is_ready, is_valid): (bool, bool) = db
            .query(
                "
                SELECT pg_index.indisready, pg_index.indisvalid
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                WHERE pg_class.relname = 'name_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| (row.get("indisready"), row.get("indisvalid")))
            .unwrap();

        assert!(is_ready, "expected index to be ready");
        assert!(is_valid, "expected index to be valid");
    });

    test.run();
}

#[test]
fn add_index_unique() {
    let mut test = Test::new("Add unique index");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "name"
            type = "TEXT"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_name_index"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "name_idx"
            columns = ["name"]        
            unique = true
        "#,
    );

    test.intermediate(|db, _| {
        // Ensure index is valid, ready and unique
        let (is_ready, is_valid, is_unique): (bool, bool, bool) = db
            .query(
                "
                SELECT pg_index.indisready, pg_index.indisvalid, pg_index.indisunique
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                WHERE pg_class.relname = 'name_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| {
                (
                    row.get("indisready"),
                    row.get("indisvalid"),
                    row.get("indisunique"),
                )
            })
            .unwrap();

        assert!(is_ready, "expected index to be ready");
        assert!(is_valid, "expected index to be valid");
        assert!(is_unique, "expected index to be unique");
    });

    test.run();
}

#[test]
fn add_index_with_type() {
    let mut test = Test::new("Add GIN index");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "data"
            type = "JSONB"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_data_index"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "data_idx"
            columns = ["data"]        
            type = "gin"
        "#,
    );

    test.intermediate(|db, _| {
        // Ensure index is valid, ready and has the right type
        let (is_ready, is_valid, index_type): (bool, bool, String) = db
            .query(
                "
                SELECT pg_index.indisready, pg_index.indisvalid, pg_am.amname
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                JOIN pg_catalog.pg_am ON pg_class.relam = pg_am.oid
                WHERE pg_class.relname = 'data_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| {
                (
                    row.get("indisready"),
                    row.get("indisvalid"),
                    row.get("amname"),
                )
            })
            .unwrap();

        assert!(is_ready, "expected index to be ready");
        assert!(is_valid, "expected index to be valid");
        assert_eq!("gin", index_type, "expected index type to be GIN");
    });

    test.run();
}

#[test]
fn add_index_keeps_column_order() {
    let mut test = Test::new("Add index with multiple columns");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "first_name"
            type = "TEXT"

            [[actions.columns]]
            name = "last_name"
            type = "TEXT"
        "#,
    );

    // The index columns are declared in the opposite order to the table columns
    test.second_migration(
        r#"
        name = "add_users_name_index"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "name_idx"
            columns = ["last_name", "first_name"]
        "#,
    );

    test.intermediate(|db, _| {
        let definition = get_index_definition(db, "name_idx");
        assert!(
            definition.contains("(last_name, first_name)"),
            "expected index columns to be in the declared order, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_with_direction_and_nulls() {
    let mut test = Test::new("Add index with sort order");

    test.first_migration(
        r#"
        name = "create_posts_table"

        [[actions]]
        type = "create_table"
        name = "posts"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "audience"
            type = "TEXT"

            [[actions.columns]]
            name = "content_updated_at"
            type = "TIMESTAMP"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_posts_keyset_index"

        [[actions]]
        type = "add_index"
        table = "posts"

            [actions.index]
            name = "posts_keyset_idx"
            columns = [
                "audience",
                { column = "content_updated_at", direction = "DESC", nulls = "LAST" },
                "id",
            ]
        "#,
    );

    test.intermediate(|db, _| {
        let definition = get_index_definition(db, "posts_keyset_idx");
        assert!(
            definition.contains("(audience, content_updated_at DESC NULLS LAST, id)"),
            "expected index to use the declared sort order, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_partial() {
    let mut test = Test::new("Add partial index");

    test.first_migration(
        r#"
        name = "create_posts_table"

        [[actions]]
        type = "create_table"
        name = "posts"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "is_public"
            type = "BOOLEAN"

            [[actions.columns]]
            name = "shared_to_community"
            type = "BOOLEAN"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_posts_community_index"

        [[actions]]
        type = "add_index"
        table = "posts"

            [actions.index]
            name = "posts_community_idx"
            columns = ["id"]
            where = "is_public AND shared_to_community"
        "#,
    );

    test.intermediate(|db, _| {
        let definition = get_index_definition(db, "posts_community_idx");
        assert!(
            definition.contains("WHERE (is_public AND shared_to_community)"),
            "expected index to be partial, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_with_expression() {
    let mut test = Test::new("Add expression index");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_users_email_index"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_email_idx"
            unique = true
            columns = [{ expression = "lower(email)" }]
        "#,
    );

    test.intermediate(|db, _| {
        let definition = get_index_definition(db, "users_email_idx");
        assert!(
            definition.contains("lower(email)"),
            "expected an expression index, got: {}",
            definition
        );
        assert!(
            definition.contains("CREATE UNIQUE INDEX"),
            "expected index to be unique, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_to_renamed_table() {
    let mut test = Test::new("Add index to table renamed in the same migration");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "name"
            type = "TEXT"
        "#,
    );

    // The index is added to the table using its new name, which doesn't exist in the
    // database until the migration is completed
    test.second_migration(
        r#"
        name = "rename_users_to_customers"

        [[actions]]
        type = "rename_table"
        table = "users"
        new_name = "customers"

        [[actions]]
        type = "add_index"
        table = "customers"

            [actions.index]
            name = "customers_name_idx"
            columns = ["name"]
        "#,
    );

    test.intermediate(|db, _| {
        // The index is created on the table under its current, real name
        let definition = get_index_definition(db, "customers_name_idx");
        assert!(
            definition.contains("ON public.users"),
            "expected index to be created on the real table, got: {}",
            definition
        );
    });

    test.after_completion(|db| {
        let definition = get_index_definition(db, "customers_name_idx");
        assert!(
            definition.contains("ON public.customers"),
            "expected index to follow the table rename, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_to_migrating_column() {
    let mut test = Test::new("Add index to column added in the same migration");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"
        "#,
    );

    // Plain column references can be rewritten to the temporary column, so indexing a
    // column which is being added in the same migration is fine
    test.second_migration(
        r#"
        name = "add_users_name_column_and_index"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "name"
            type = "TEXT"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_name_idx"
            columns = ["name"]
        "#,
    );

    test.intermediate(|db, _| {
        // The index is created on the temporary column
        let definition = get_index_definition(db, "users_name_idx");
        assert!(
            definition.contains("__reshape"),
            "expected index to be created on the temporary column, got: {}",
            definition
        );
    });

    test.after_completion(|db| {
        // Once completed, the index follows the column to its final name
        let definition = get_index_definition(db, "users_name_idx");
        assert!(
            definition.contains("(name)"),
            "expected index to follow the column rename, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_with_expression_alongside_a_column_migration() {
    let mut test = Test::new("Add expression index while another column is being migrated");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    // The index doesn't reference the column being added, so it is unaffected by it
    test.second_migration(
        r#"
        name = "add_nickname_column_and_email_index"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "nickname"
            type = "TEXT"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_email_idx"
            columns = [{ expression = "lower(email)" }]
            where = "email IS NOT NULL"
        "#,
    );

    test.intermediate(|db, _| {
        let definition = get_index_definition(db, "users_email_idx");
        assert!(
            definition.contains("lower(email)"),
            "expected an expression index, got: {}",
            definition
        );
    });

    test.after_completion(|db| {
        // The index must survive the column being renamed to its final name
        let definition = get_index_definition(db, "users_email_idx");
        assert!(
            definition.contains("lower(email)"),
            "expected the index to survive completion, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_with_expression_to_renamed_column() {
    let mut test = Test::new("Add expression index to renamed column");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    // A rename keeps the same underlying column, so the index follows it and the
    // expression is written against the column's current, real name
    test.second_migration(
        r#"
        name = "rename_email_and_index_it"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "email"

            [actions.changes]
            name = "email_address"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_email_idx"
            columns = [{ expression = "lower(email)" }]
        "#,
    );

    test.after_completion(|db| {
        let definition = get_index_definition(db, "users_email_idx");
        assert!(
            definition.contains("lower(email_address)"),
            "expected the index to follow the rename, got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn add_index_with_expression_to_replaced_column() {
    let mut test = Test::new("Add expression index to a column being replaced");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    // Altering the column replaces it with a new one and drops the original on
    // completion, which would take the index with it. The migration must be rejected
    // rather than silently losing the index.
    test.second_migration(
        r#"
        name = "lowercase_email_and_index_it"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "email"
        up = "LOWER(email)"
        down = "email"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_email_idx"
            columns = [{ expression = "lower(email)" }]
        "#,
    );

    test.expect_failure();
    test.run();
}

fn get_index_definition(db: &mut postgres::Client, index_name: &str) -> String {
    db.query(
        "
        SELECT pg_get_indexdef(pg_class.oid) AS definition
        FROM pg_catalog.pg_class
        JOIN pg_catalog.pg_namespace ON pg_class.relnamespace = pg_namespace.oid
        WHERE pg_class.relname = $1 AND pg_namespace.nspname = 'public'
        ",
        &[&index_name],
    )
    .unwrap()
    .first()
    .map(|row| row.get("definition"))
    .unwrap_or_else(|| panic!("expected index {} to exist", index_name))
}
