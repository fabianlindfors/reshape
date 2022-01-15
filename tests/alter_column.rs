mod common;
use common::Test;

#[test]
fn alter_column_data() {
    let mut test = Test::new("Alter column");

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
        name = "uppercase_name"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        up = "UPPER(name)"
        down = "LOWER(name)"
        "#,
    );

    test.after_first(|db| {
        // Insert some test users
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'john Doe'),
                (2, 'jane Doe');
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Check that the existing users has the altered data
        let expected = vec!["JOHN DOE", "JANE DOE"];
        assert!(new_db
            .query("SELECT name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<_, String>("name"))
            .eq(expected));

        // Insert data using old schema and make sure the new schema gets correct values
        old_db
            .simple_query("INSERT INTO users (id, name) VALUES (3, 'test testsson')")
            .unwrap();
        let result = new_db
            .query_one("SELECT name from users WHERE id = 3", &[])
            .unwrap();
        assert_eq!("TEST TESTSSON", result.get::<_, &str>("name"));

        // Insert data using new schema and make sure the old schema gets correct values
        new_db
            .simple_query("INSERT INTO users (id, name) VALUES (4, 'TEST TESTSSON')")
            .unwrap();
        let result = old_db
            .query_one("SELECT name from users WHERE id = 4", &[])
            .unwrap();
        assert_eq!("test testsson", result.get::<_, &str>("name"));
    });

    test.run();
}

#[test]
fn alter_column_set_not_null() {
    let mut test = Test::new("Set column not null");

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
        name = "set_name_not_null"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        up = "COALESCE(name, 'TEST_DEFAULT_VALUE')"
        down = "name"

            [actions.changes]
            nullable = false
        "#,
    );

    test.after_first(|db| {
        // Insert some test users
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe'),
                (2, NULL);
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Check that existing users got the correct values
        let expected = vec!["John Doe", "TEST_DEFAULT_VALUE"];
        assert!(new_db
            .query("SELECT name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<_, String>("name"))
            .eq(expected));

        // Insert data using old schema and make sure the new schema gets correct values
        old_db
            .simple_query("INSERT INTO users (id, name) VALUES (3, NULL)")
            .unwrap();
        let result = new_db
            .query_one("SELECT name from users WHERE id = 3", &[])
            .unwrap();
        assert_eq!("TEST_DEFAULT_VALUE", result.get::<_, &str>("name"));

        // Insert data using new schema and make sure the old schema gets correct values
        new_db
            .simple_query("INSERT INTO users (id, name) VALUES (4, 'Jane Doe')")
            .unwrap();
        let result = old_db
            .query_one("SELECT name from users WHERE id = 4", &[])
            .unwrap();
        assert_eq!("Jane Doe", result.get::<_, &str>("name"));
    });

    test.run();
}

#[test]
fn alter_column_rename() {
    let mut test = Test::new("Rename column");

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
        name = "set_name_not_null"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"

            [actions.changes]
            name = "full_name"
        "#,
    );

    test.after_first(|db| {
        // Insert some test data
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe'),
                (2, 'Jane Doe');
            ",
        )
        .unwrap();
    });

    test.intermediate(|_, new_db| {
        // Check that existing values can be queried using new column name
        let expected = vec!["John Doe", "Jane Doe"];
        assert!(new_db
            .query("SELECT full_name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<_, String>("full_name"))
            .eq(expected));
    });

    test.run();
}

#[test]
fn alter_column_multiple() {
    let mut test = Test::new("Alter column value multiple times");

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
            name = "counter"
            type = "INTEGER"
            nullable = false
        "#,
    );

    test.second_migration(
        r#"
        name = "increment_counter_twice"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "counter"
        up = "counter + 1"
        down = "counter - 1"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "counter"
        up = "counter + 1"
        down = "counter - 1"
        "#,
    );

    test.after_first(|db| {
        // Insert some test data
        db.simple_query(
            "
            INSERT INTO users (id, counter) VALUES
                (1, 0),
                (2, 100);
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Check that the existing data has been updated
        let expected = vec![2, 102];
        let results: Vec<i32> = new_db
            .query("SELECT counter FROM users ORDER BY id", &[])
            .unwrap()
            .iter()
            .map(|row| row.get::<_, i32>("counter"))
            .collect();
        assert_eq!(expected, results);

        // Update data using old schema and make sure it was updated for the new schema
        old_db
            .query("UPDATE users SET counter = 50 WHERE id = 1", &[])
            .unwrap();
        let result: i32 = new_db
            .query("SELECT counter FROM users WHERE id = 1", &[])
            .unwrap()
            .iter()
            .map(|row| row.get("counter"))
            .nth(0)
            .unwrap();
        assert_eq!(52, result);

        // Update data using new schema and make sure it was updated for the old schema
        new_db
            .query("UPDATE users SET counter = 50 WHERE id = 1", &[])
            .unwrap();
        let result: i32 = old_db
            .query("SELECT counter FROM users WHERE id = 1", &[])
            .unwrap()
            .iter()
            .map(|row| row.get("counter"))
            .nth(0)
            .unwrap();
        assert_eq!(48, result);
    });

    test.run();
}

#[test]
fn alter_column_default() {
    let mut test = Test::new("Change default value for column");

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
            nullable = false
            default = "'DEFAULT'"
        "#,
    );

    test.second_migration(
        r#"
        name = "change_name_default"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"

            [actions.changes]
            default = "'NEW DEFAULT'"
        "#,
    );

    test.after_first(|db| {
        // Insert a test user
        db.simple_query(
            "
            INSERT INTO users (id) VALUES (1)
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Check that the existing users has the old default value
        let expected = vec!["DEFAULT"];
        assert!(new_db
            .query("SELECT name FROM users", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<_, String>("name"))
            .eq(expected));

        // Insert data using old schema and make those get the old default value
        old_db
            .simple_query("INSERT INTO users (id) VALUES (2)")
            .unwrap();
        let result = new_db
            .query_one("SELECT name from users WHERE id = 2", &[])
            .unwrap();
        assert_eq!("DEFAULT", result.get::<_, &str>("name"));

        // Insert data using new schema and make sure it gets the new default value
        new_db
            .simple_query("INSERT INTO users (id) VALUES (3)")
            .unwrap();
        let result = old_db
            .query_one("SELECT name from users WHERE id = 3", &[])
            .unwrap();
        assert_eq!("NEW DEFAULT", result.get::<_, &str>("name"));
    });

    test.run();
}

#[test]
fn alter_column_with_index() {
    let mut test = Test::new("Alter column with index");

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
            name = "first_name"
            type = "TEXT"

            [[actions.columns]]
            name = "last_name"
            type = "TEXT"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_name_idx"
            columns = ["first_name", "last_name"]
        "#,
    );

    test.second_migration(
        r#"
        name = "uppercase_last_name"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "last_name"
        up = "UPPER(last_name)"
        down = "LOWER(last_name)"
        "#,
    );

    test.after_completion(|db| {
        // Make sure index still exists
        let result: i64 = db
            .query(
                "
			SELECT COUNT(*)
			FROM pg_catalog.pg_index
			JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
			WHERE pg_class.relname = 'users_name_idx'
			",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get(0))
            .unwrap();
        assert_eq!(1, result, "expected index to still exist");
    });

    test.run();
}
