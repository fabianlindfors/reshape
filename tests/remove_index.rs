mod common;
use common::Test;

#[test]
fn remove_index() {
    let mut test = Test::new("Remove index");

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

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "name_idx"
            columns = ["name"]        
        "#,
    );

    test.second_migration(
        r#"
        name = "remove_name_index"

        [[actions]]
        type = "remove_index"
        index = "name_idx"
        "#,
    );

    test.intermediate(|db, _| {
        // Ensure index is still valid and ready during the migration
        let result: Vec<(bool, bool)> = db
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
            .iter()
            .map(|row| (row.get("indisready"), row.get("indisvalid")))
            .collect();

        assert_eq!(vec![(true, true)], result);
    });

    test.after_completion(|db| {
        // Ensure index has been removed after the migration is complete
        let count: i64 = db
            .query(
                "
                SELECT COUNT(*)
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                WHERE pg_class.relname = 'name_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get(0))
            .unwrap();

        assert_eq!(0, count, "expected index to not exist");
    });

    test.run();
}
