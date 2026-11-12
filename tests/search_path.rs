mod common;
use common::Test;
use postgres::Client;

// Applications can keep other schemas on their search path after the migration schema,
// for example `public` so that types owned by extensions still resolve. These tests make
// sure the old and new schema are still told apart when they do.

// Puts the schema of a migration first on the search path, followed by `public`
fn use_schema_followed_by_public(db: &mut Client, migration_name: &str) {
    db.simple_query(&format!(
        "SET search_path TO migration_{migration_name}, public"
    ))
    .unwrap();
}

fn is_new_schema(db: &mut Client, search_path: &str) -> bool {
    db.simple_query(&format!("SET search_path TO {search_path}"))
        .unwrap();
    db.query_one("SELECT reshape.is_new_schema() AS is_new_schema", &[])
        .unwrap()
        .get("is_new_schema")
}

fn user_name(db: &mut Client, id: i32) -> Option<String> {
    db.query_one("SELECT name FROM users WHERE id = $1", &[&id])
        .unwrap()
        .get("name")
}

#[test]
fn new_schema_is_the_first_schema_on_search_path() {
    let mut test = Test::new("New schema is the first schema on search path");

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
        name = "add_email"

        [[actions]]
        type = "add_column"
        table = "users"

            [actions.column]
            name = "email"
            type = "TEXT"
        "#,
    );

    test.intermediate(|_, db| {
        // The new schema on its own, which is what `reshape schema-query` generates
        assert!(is_new_schema(db, "migration_add_email"));

        // The new schema followed by other schemas
        assert!(is_new_schema(db, "migration_add_email, public"));
        assert!(is_new_schema(db, "\"migration_add_email\", public"));

        // Postgres skips schemas which don't exist, like "$user" in the default search path
        assert!(is_new_schema(db, "\"$user\", migration_add_email, public"));

        // The old schema, on its own and followed by other schemas
        assert!(!is_new_schema(db, "migration_create_user_table"));
        assert!(!is_new_schema(db, "migration_create_user_table, public"));

        // Another schema takes precedence over the new one
        assert!(!is_new_schema(db, "public"));
        assert!(!is_new_schema(db, "public, migration_add_email"));
        assert!(!is_new_schema(
            db,
            "migration_create_user_table, migration_add_email"
        ));

        // No effective schema must return false rather than SQL NULL.
        assert!(!is_new_schema(db, "''"));
        assert!(!is_new_schema(db, "reshape_nonexistent_schema"));
        assert!(is_new_schema(
            db,
            "reshape_nonexistent_schema, migration_add_email, public"
        ));

        // Internal backfills explicitly select the new-schema behavior, even when
        // no migration schema is on the connection's search path.
        db.simple_query("SET reshape.is_new_schema TO 'YES'")
            .unwrap();
        assert!(is_new_schema(db, "public"));
        assert!(is_new_schema(db, "''"));
        db.simple_query("RESET reshape.is_new_schema").unwrap();
        assert!(!is_new_schema(db, "public"));
        assert!(is_new_schema(db, "migration_add_email, public"));
    });

    test.run();
}

#[test]
fn alter_column_with_public_on_search_path() {
    let mut test = Test::new("Alter column with public on search path");

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
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'john doe'),
                (2, 'jane doe');
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        use_schema_followed_by_public(old_db, "create_user_table");
        use_schema_followed_by_public(new_db, "uppercase_name");

        // Insert using old schema and make sure the new schema gets the upgraded value
        old_db
            .simple_query("INSERT INTO users (id, name) VALUES (3, 'test testsson')")
            .unwrap();
        assert_eq!(Some("TEST TESTSSON".to_string()), user_name(new_db, 3));

        // Insert using new schema and make sure the value is kept, and that the old
        // schema gets the downgraded value
        new_db
            .simple_query("INSERT INTO users (id, name) VALUES (4, 'NEW USER')")
            .unwrap();
        assert_eq!(Some("NEW USER".to_string()), user_name(new_db, 4));
        assert_eq!(Some("new user".to_string()), user_name(old_db, 4));

        // Update using new schema and make sure the update is not overwritten by the
        // value in the old schema
        new_db
            .simple_query("UPDATE users SET name = 'JOHN SMITH' WHERE id = 1")
            .unwrap();
        assert_eq!(Some("JOHN SMITH".to_string()), user_name(new_db, 1));
        assert_eq!(Some("john smith".to_string()), user_name(old_db, 1));

        // Update using old schema and make sure the new schema gets the upgraded value
        old_db
            .simple_query("UPDATE users SET name = 'jane smith' WHERE id = 2")
            .unwrap();
        assert_eq!(Some("JANE SMITH".to_string()), user_name(new_db, 2));
        assert_eq!(Some("jane smith".to_string()), user_name(old_db, 2));
    });

    test.after_completion(|db| {
        assert_eq!(Some("JOHN SMITH".to_string()), user_name(db, 1));
        assert_eq!(Some("JANE SMITH".to_string()), user_name(db, 2));
        assert_eq!(Some("TEST TESTSSON".to_string()), user_name(db, 3));
        assert_eq!(Some("NEW USER".to_string()), user_name(db, 4));
    });

    test.after_abort(|db| {
        assert_eq!(Some("john smith".to_string()), user_name(db, 1));
        assert_eq!(Some("jane smith".to_string()), user_name(db, 2));
        assert_eq!(Some("test testsson".to_string()), user_name(db, 3));
        assert_eq!(Some("new user".to_string()), user_name(db, 4));
    });

    test.run();
}

#[test]
fn add_column_with_public_on_search_path() {
    let mut test = Test::new("Add column with public on search path");

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
        name = "add_nickname"

        [[actions]]
        type = "add_column"
        table = "users"

        up = "name"

            [actions.column]
            name = "nickname"
            type = "TEXT"
        "#,
    );

    test.intermediate(|old_db, new_db| {
        use_schema_followed_by_public(old_db, "create_user_table");
        use_schema_followed_by_public(new_db, "add_nickname");

        fn nickname(db: &mut Client, id: i32) -> Option<String> {
            db.query_one("SELECT nickname FROM users WHERE id = $1", &[&id])
                .unwrap()
                .get("nickname")
        }

        // Insert using old schema and make sure the new column is populated
        old_db
            .simple_query("INSERT INTO users (id, name) VALUES (1, 'John Doe')")
            .unwrap();
        assert_eq!(Some("John Doe".to_string()), nickname(new_db, 1));

        // Insert using new schema and make sure the value of the new column is kept
        new_db
            .simple_query("INSERT INTO users (id, name, nickname) VALUES (2, 'Jane Doe', 'JD')")
            .unwrap();
        assert_eq!(Some("JD".to_string()), nickname(new_db, 2));

        // Update using new schema and make sure the value of the new column is kept
        new_db
            .simple_query("UPDATE users SET nickname = 'Johnny' WHERE id = 1")
            .unwrap();
        assert_eq!(Some("Johnny".to_string()), nickname(new_db, 1));
    });

    test.run();
}

#[test]
fn remove_column_with_public_on_search_path() {
    let mut test = Test::new("Remove column with public on search path");

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
        use_schema_followed_by_public(old_db, "create_user_table");
        use_schema_followed_by_public(new_db, "remove_name_column");

        // Insert using old schema and ensure it can be retrieved through new schema
        old_db
            .simple_query("INSERT INTO users (id, name) VALUES (1, 'John Doe')")
            .unwrap();
        let results = new_db
            .query("SELECT id FROM users WHERE id = 1", &[])
            .unwrap();
        assert_eq!(1, results.len());

        // The removed column still can't be NULL when written from the old schema
        assert!(old_db
            .simple_query("INSERT INTO users (id, name) VALUES (2, NULL)")
            .is_err());

        // Insert using new schema and ensure the down function is applied
        new_db
            .simple_query("INSERT INTO users (id) VALUES (3)")
            .unwrap();
        assert_eq!(Some("TEST_DOWN_VALUE".to_string()), user_name(old_db, 3));
    });

    test.run();
}

#[test]
fn create_table_with_public_on_search_path() {
    let mut test = Test::new("Create table with public on search path");

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

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    test.second_migration(
        r#"
        name = "create_profiles_table"

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

            [actions.up]
            table = "users"
            values = { user_id = "id", email = "email" }
            where = "profiles.user_id = users.id"
        "#,
    );

    test.after_first(|db| {
        db.simple_query(
            "INSERT INTO users (id, name, email) VALUES (1, 'John Doe', 'john@doe.com')",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        use_schema_followed_by_public(old_db, "create_user_table");
        use_schema_followed_by_public(new_db, "create_profiles_table");

        fn profile_email(db: &mut Client, user_id: i32) -> Option<String> {
            db.query_one("SELECT email FROM profiles WHERE user_id = $1", &[&user_id])
                .unwrap()
                .get("email")
        }

        // Ensure the new table was backfilled
        assert_eq!(Some("john@doe.com".to_string()), profile_email(new_db, 1));

        // Update using old schema and make sure the new table is updated
        old_db
            .simple_query("UPDATE users SET email = 'john.doe@doe.com' WHERE id = 1")
            .unwrap();
        assert_eq!(
            Some("john.doe@doe.com".to_string()),
            profile_email(new_db, 1)
        );

        // Update the new table using the new schema. A later write to the source table
        // from the new schema should not overwrite it with the value in the old schema.
        new_db
            .simple_query("UPDATE profiles SET email = 'new@doe.com' WHERE user_id = 1")
            .unwrap();
        new_db
            .simple_query("UPDATE users SET name = 'John Smith' WHERE id = 1")
            .unwrap();
        assert_eq!(Some("new@doe.com".to_string()), profile_email(new_db, 1));
    });

    test.run();
}
