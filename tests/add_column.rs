mod common;
use common::Test;

#[test]
fn add_column() {
    let mut test = Test::new("Add column");

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
        name = "add_first_and_last_name_columns"

        [[actions]]
        type = "add_column"
        table = "users"

        up = "(STRING_TO_ARRAY(name, ' '))[1]"

            [actions.column]
            name = "first"
            type = "TEXT"
            nullable = false

        [[actions]]
        type = "add_column"
        table = "users"

        up = "(STRING_TO_ARRAY(name, ' '))[2]"

            [actions.column]
            name = "last"
            type = "TEXT"
            nullable = false
        "#,
    );

    test.after_first(|db| {
        // Insert some test users
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe'),
                (2, 'Jane Doe');
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Check that the existing users have the new columns populated
        let expected = vec![("John", "Doe"), ("Jane", "Doe")];
        assert!(new_db
            .query("SELECT first, last FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| (row.get("first"), row.get("last")))
            .eq(expected));

        // Insert data using old schema and make sure the new columns are populated
        old_db
            .simple_query("INSERT INTO users (id, name) VALUES (3, 'Test Testsson')")
            .unwrap();
        let (first_name, last_name): (String, String) = new_db
            .query_one("SELECT first, last from users WHERE id = 3", &[])
            .map(|row| (row.get("first"), row.get("last")))
            .unwrap();
        assert_eq!(
            ("Test", "Testsson"),
            (first_name.as_ref(), last_name.as_ref())
        );
    });

    test.after_completion(|db| {
        let expected = vec![("John", "Doe"), ("Jane", "Doe"), ("Test", "Testsson")];
        assert!(db
            .query("SELECT first, last FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| (row.get("first"), row.get("last")))
            .eq(expected));
    });

    test.after_abort(|db| {
        let expected = vec![("John Doe"), ("Jane Doe"), ("Test Testsson")];
        assert!(db
            .query("SELECT name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<'_, _, String>("name"))
            .eq(expected));
    });

    test.run()
}

#[test]
fn add_column_nullable() {
    let mut test = Test::new("Add nullable column");

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

    test.second_migration(
        r#"
        name = "add_nullable_name_column"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "name"
            type = "TEXT"
        "#,
    );

    test.after_first(|db| {
        // Insert some test values
        db.simple_query(
            "
            INSERT INTO users (id) VALUES (1), (2);
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Ensure existing data got updated
        let expected: Vec<Option<String>> = vec![None, None];
        assert!(new_db
            .query("SELECT name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<_, Option<String>>("name"))
            .eq(expected));

        // Insert data using old schema and ensure new column is NULL
        old_db
            .simple_query("INSERT INTO users (id) VALUES (3)")
            .unwrap();
        let name: Option<String> = new_db
            .query_one("SELECT name from users WHERE id = 3", &[])
            .map(|row| (row.get("name")))
            .unwrap();
        assert_eq!(None, name);

        // Ensure data can be inserted against new schema
        new_db
            .simple_query("INSERT INTO users (id, name) VALUES (4, 'Test Testsson'), (5, NULL)")
            .unwrap();
    });

    test.after_completion(|db| {
        let expected: Vec<Option<String>> =
            vec![None, None, None, Some("Test Testsson".to_string()), None];
        let result: Vec<Option<String>> = db
            .query("SELECT id, name FROM users ORDER BY id", &[])
            .unwrap()
            .iter()
            .map(|row| row.get("name"))
            .collect();

        assert_eq!(result, expected);
    });

    test.run();
}

#[test]
fn add_column_with_default() {
    let mut test = Test::new("Add column with default value");

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

    test.second_migration(
        r#"
        name = "add_name_column_with_default"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "name"
            type = "TEXT"
            nullable = false
            default = "'DEFAULT'"
        "#,
    );

    test.after_first(|db| {
        // Insert some test values
        db.simple_query("INSERT INTO users (id) VALUES (1), (2)")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Ensure existing data got updated with defaults
        let expected = vec!["DEFAULT".to_string(), "DEFAULT".to_string()];
        assert!(new_db
            .query("SELECT name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<_, String>("name"))
            .eq(expected));

        // Insert data using old schema and ensure new column gets the default value
        old_db
            .simple_query("INSERT INTO users (id) VALUES (3)")
            .unwrap();
        let name: String = new_db
            .query_one("SELECT name from users WHERE id = 3", &[])
            .map(|row| row.get("name"))
            .unwrap();
        assert_eq!("DEFAULT", name);
    });

    test.run();
}

#[test]
fn add_column_with_complex_up() {
    let mut test = Test::new("Add column complex");

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
        "#,
    );

    test.second_migration(
        r#"
        name = "add_profiles_email_column"

        [[actions]]
        type = "add_column"
        table = "profiles"

            [actions.column]
            name = "email"
            type = "TEXT"
            nullable = false

            [actions.up]
            table = "users"
            value = "users.email"
            where = "profiles.user_id = users.id"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, email) VALUES (1, 'test@example.com')")
            .unwrap();
        db.simple_query("INSERT INTO profiles (user_id) VALUES (1)")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Ensure email was backfilled on profiles
        let email: String = new_db
            .query(
                "
                SELECT email
                FROM profiles
                WHERE user_id = 1
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get("email"))
            .unwrap();
        assert_eq!("test@example.com", email);

        // Ensure email change in old schema is propagated to profiles table in new schema
        old_db
            .simple_query("UPDATE users SET email = 'test2@example.com' WHERE id = 1")
            .unwrap();
        let email: String = new_db
            .query(
                "
                SELECT email
                FROM profiles
                WHERE user_id = 1
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
