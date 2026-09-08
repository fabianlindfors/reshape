mod common;
use common::{assert_invalid, get_column_comment, Test};

#[test]
fn add_column_invalid_up_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "add_column"
        table = "users"
        up = "INVALID $$$ SYNTAX"
        [actions.column]
        name = "test"
        type = "TEXT"
        "#,
    );
}

#[test]
fn add_column_invalid_default_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "add_column"
        table = "users"
        [actions.column]
        name = "test"
        type = "TEXT"
        default = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn add_column_invalid_complex_up_value_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "add_column"
        table = "users"
        [actions.column]
        name = "test"
        type = "TEXT"
        [actions.up]
        table = "other"
        value = "INVALID $$$ SYNTAX"
        where = "users.id = other.id"
        "#,
    );
}

#[test]
fn add_column_invalid_complex_up_where_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "add_column"
        table = "users"
        [actions.column]
        name = "test"
        type = "TEXT"
        [actions.up]
        table = "other"
        value = "other.value"
        where = "INVALID $$$ SYNTAX"
        "#,
    );
}

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
fn add_column_generated_identity() {
    let mut test = Test::new("Add generated identity column");

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
        name = "add_generated_column"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "seq"
            type = "INTEGER"
            nullable = false
            generated = "ALWAYS AS IDENTITY"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id) VALUES (1), (2)")
            .unwrap();
    });

    test.intermediate(|_old_db, new_db| {
        // Existing rows are backfilled from the identity sequence
        let values: Vec<i32> = new_db
            .query("SELECT seq FROM users ORDER BY id", &[])
            .unwrap()
            .iter()
            .map(|row| row.get("seq"))
            .collect();
        assert_eq!(values, vec![1, 2]);

        // New rows get the next value automatically
        new_db
            .simple_query("INSERT INTO users (id) VALUES (3)")
            .unwrap();
        let seq: i32 = new_db
            .query_one("SELECT seq FROM users WHERE id = 3", &[])
            .unwrap()
            .get("seq");
        assert_eq!(seq, 3);
    });

    test.run();
}

#[test]
fn add_column_default_with_column_reference() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "add_column"
        table = "users"
        [actions.column]
        name = "name_copy"
        type = "TEXT"
        default = "lower(name)"
        "#,
    );
}

#[test]
fn add_column_up_invalid_column_reference() {
    let mut test = Test::new("Add column with invalid up reference");

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
            name = "name"
            type = "TEXT"

        "#,
    );

    test.second_migration(
        r#"
        name = "add_column_with_bad_reference"

        [[actions]]
        type = "add_column"
        table = "users"
        up = "UPPER(non_existent)"

            [actions.column]
            name = "upper_name"
            type = "TEXT"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn add_column_complex_up_invalid_column_reference() {
    let mut test = Test::new("Add column with invalid cross-table up reference");

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
            name = "name"
            type = "TEXT"

        [[actions]]
        type = "create_table"
        name = "profiles"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"

        "#,
    );

    // `where` may reference both tables but `value` references a column which exists on
    // neither
    test.second_migration(
        r#"
        name = "add_column_with_bad_reference"

        [[actions]]
        type = "add_column"
        table = "profiles"

            [actions.column]
            name = "name"
            type = "TEXT"

            [actions.up]
            table = "users"
            value = "users.non_existent"
            where = "user_id = id"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn add_column_complex_up_unqualified_reference() {
    let mut test = Test::new("Add column with unqualified cross-table reference");

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
            name = "name"
            type = "TEXT"

        [[actions]]
        type = "create_table"
        name = "profiles"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"
        "#,
    );

    // `user_id` exists on `profiles` but resolves to the wrong table in one of the two
    // triggers, so it has to be qualified
    test.second_migration(
        r#"
        name = "add_column_with_unqualified_reference"

        [[actions]]
        type = "add_column"
        table = "profiles"

            [actions.column]
            name = "name"
            type = "TEXT"

            [actions.up]
            table = "users"
            value = "users.name"
            where = "user_id = users.id"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn add_column_up_references_column_added_earlier() {
    let mut test = Test::new("Add column referencing column added in same migration");

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
            name = "name"
            type = "TEXT"

        "#,
    );

    // `display_name` references `nickname` which is added by the previous action and
    // only exists as a temporary column at this point
    test.second_migration(
        r#"
        name = "add_nickname_and_display_name"

        [[actions]]
        type = "add_column"
        table = "users"
        up = "LOWER(name)"

            [actions.column]
            name = "nickname"
            type = "TEXT"

        [[actions]]
        type = "add_column"
        table = "users"
        up = "COALESCE(nickname, name)"

            [actions.column]
            name = "display_name"
            type = "TEXT"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, name) VALUES (1, 'John')")
            .unwrap();
    });

    test.intermediate(|_old_db, new_db| {
        let display_name: String = new_db
            .query_one("SELECT display_name FROM users WHERE id = 1", &[])
            .unwrap()
            .get("display_name");
        assert_eq!("john", display_name);
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

#[test]
fn add_column_with_comment() {
    let mut test = Test::new("Add column with comment");

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
        name = "add_users_name_column"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "name"
            type = "TEXT"
            comment = "The user's display name"
        "#,
    );

    test.intermediate(|_, new_db| {
        // Ensure the comment is visible through the new schema's view while the
        // column is still backed by a temporary column
        assert_eq!(
            Some("The user's display name".to_string()),
            get_column_comment(new_db, "users", "name")
        );
    });

    test.after_completion(|db| {
        // Ensure the comment follows the column when it is renamed to its final name
        assert_eq!(
            Some("The user's display name".to_string()),
            get_column_comment(db, "public.users", "name")
        );
    });

    test.after_abort(|db| {
        // Ensure the column, and with it the comment, was removed
        assert_eq!(None, get_column_comment(db, "public.users", "name"));
    });

    test.run();
}
