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
