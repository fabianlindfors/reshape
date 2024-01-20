mod common;
use common::Test;

#[test]
fn remove_column() {
    let mut test = Test::new("Remove column");

    test.first_migration(
        r#"
        name = "create_user_table"

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
        name = "remove_name_column"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        down = "'TEST_DOWN_VALUE'"
        "#,
    );

    test.intermediate(|old_db, new_db| {
        // Insert using old schema and ensure it can be retrieved through new schema
        old_db
            .simple_query("INSERT INTO users(id, name) VALUES (1, 'John Doe')")
            .unwrap();
        let results = new_db
            .query("SELECT id FROM users WHERE id = 1", &[])
            .unwrap();
        assert_eq!(1, results.len());
        assert_eq!(1, results[0].get::<_, i32>("id"));

        // Ensure the name column is not accesible through the new schema
        assert!(new_db.query("SELECT id, name FROM users", &[]).is_err());

        // Insert using new schema and ensure the down function is correctly applied
        new_db
            .simple_query("INSERT INTO users(id) VALUES (2)")
            .unwrap();
        let result = old_db
            .query_opt("SELECT name FROM users WHERE id = 2", &[])
            .unwrap();
        assert_eq!(
            Some("TEST_DOWN_VALUE"),
            result.as_ref().map(|row| row.get("name"))
        );
    });

    test.run();
}

#[test]
fn remove_column_with_index() {
    let mut test = Test::new("Remove column");

    test.first_migration(
        r#"
        name = "create_user_table"

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
        name = "remove_name_column"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        down = "'TEST_DOWN_VALUE'"
        "#,
    );

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

#[test]
fn remove_column_with_complex_down() {
    let mut test = Test::new("Remove column complex");

    test.first_migration(
        r#"
        name = "create_tables"

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

        [[actions]]
        type = "create_table"
        name = "profiles"
        primary_key = ["user_id"]

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    test.second_migration(
        r#"
        name = "remove_users_email_column"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "email"

            [actions.down]
            table = "profiles"
            value = "profiles.email"
            where = "users.id = profiles.user_id"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, email) VALUES (1, 'test@example.com')")
            .unwrap();
        db.simple_query("INSERT INTO profiles (user_id, email) VALUES (1, 'test@example.com')")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        new_db
            .simple_query("UPDATE profiles SET email = 'test2@example.com' WHERE user_id = 1")
            .unwrap();

        // Ensure new email was propagated to users table in old schema
        let email: String = old_db
            .query(
                "
                SELECT email
                FROM users
                WHERE id = 1
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get("email"))
            .unwrap();
        assert_eq!("test2@example.com", email);
    });

    test.run();
}
