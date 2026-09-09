mod common;
use common::{assert_invalid, Test};

#[test]
fn remove_column_invalid_down_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        down = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn remove_column_invalid_complex_down_value_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        [actions.down]
        table = "other"
        value = "INVALID $$$ SYNTAX"
        where = "users.id = other.id"
        "#,
    );
}

#[test]
fn remove_column_invalid_complex_down_where_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        [actions.down]
        table = "other"
        value = "other.value"
        where = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn remove_column_down_invalid_column_reference() {
    let mut test = Test::new("Remove column with invalid down reference");

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
        name = "remove_name_with_bad_reference"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        down = "non_existent"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn remove_column_down_references_removed_column() {
    let mut test = Test::new("Remove column with down referencing removed column");

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

    // `down` computes the value of the removed column, so it can't reference it
    test.second_migration(
        r#"
        name = "remove_name_referencing_itself"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "name"
        down = "UPPER(name)"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn remove_column_complex_down_with_renamed_source_column() {
    let mut test = Test::new("Cross-table down with renamed source column");

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
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
        "#,
    );

    // `user_id` is renamed and replaced by a temporary column, and referenced by its new
    // name from the cross-table transformation
    test.second_migration(
        r#"
        name = "rename_user_id_and_remove_user_email"

        [[actions]]
        type = "alter_column"
        table = "profiles"
        column = "user_id"
        up = "user_id"
        down = "user_id"

            [actions.changes]
            name = "owner_id"
            type = "BIGINT"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "email"

            [actions.down]
            table = "profiles"
            value = "profiles.email"
            where = "users.id = profiles.owner_id"
        "#,
    );

    test.after_first(|db| {
        db.simple_query(
            "
            INSERT INTO users (id, email) VALUES (1, 'a@example.com');
            INSERT INTO profiles (id, user_id, email) VALUES (10, 1, 'a@example.com');
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // A profile updated in the new schema writes the email back to the user
        new_db
            .simple_query("UPDATE profiles SET email = 'b@example.com' WHERE id = 10")
            .unwrap();
        let email: String = old_db
            .query_one("SELECT email FROM users WHERE id = 1", &[])
            .unwrap()
            .get("email");
        assert_eq!("b@example.com", email);

        // A user inserted in the new schema gets its email from the matching profile
        new_db
            .simple_query(
                "INSERT INTO profiles (id, owner_id, email) VALUES (20, 2, 'c@example.com')",
            )
            .unwrap();
        new_db
            .simple_query("INSERT INTO users (id) VALUES (2)")
            .unwrap();
        let email: String = old_db
            .query_one("SELECT email FROM users WHERE id = 2", &[])
            .unwrap()
            .get("email");
        assert_eq!("c@example.com", email);
    });

    test.run();
}

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

#[test]
fn remove_column_with_long_names() {
    let mut test = Test::new("Remove column with long names");

    // The table and column names are long enough that the generated names would exceed
    // Postgres' limit of 63 characters on identifiers, which used to make the forward,
    // reverse and NOT NULL triggers collapse into the same name
    test.first_migration(
        r#"
        name = "create_tables"

        [[actions]]
        type = "create_table"
        name = "organization_membership_profiles"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "primary_contact_email_address"
            type = "TEXT"
            nullable = false

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
        name = "remove_email"

        [[actions]]
        type = "remove_column"
        table = "organization_membership_profiles"
        column = "primary_contact_email_address"

            [actions.down]
            table = "profiles"
            value = "profiles.email"
            where = "organization_membership_profiles.id = profiles.user_id"
        "#,
    );

    test.after_first(|db| {
        db.simple_query(
            "INSERT INTO organization_membership_profiles (id, primary_contact_email_address) VALUES (1, 'test@example.com')",
        )
        .unwrap();
        db.simple_query("INSERT INTO profiles (user_id, email) VALUES (1, 'test@example.com')")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Every generated name should fit within the limit and be distinct
        let trigger_names: Vec<String> = old_db
            .query(
                "SELECT tgname::text FROM pg_trigger WHERE tgname LIKE '__reshape%' ORDER BY 1",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.get(0))
            .collect();
        assert_eq!(3, trigger_names.len());
        assert_ne!(trigger_names[0], trigger_names[1]);
        assert_ne!(trigger_names[1], trigger_names[2]);
        assert!(trigger_names.iter().all(|name| name.len() <= 63));

        new_db
            .simple_query("UPDATE profiles SET email = 'test2@example.com' WHERE user_id = 1")
            .unwrap();

        // Ensure new email was propagated to the old schema
        let email: String = old_db
            .query(
                "
                SELECT primary_contact_email_address
                FROM organization_membership_profiles
                WHERE id = 1
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get(0))
            .unwrap();
        assert_eq!("test2@example.com", email);

        // The NOT NULL check should still be enforced for the old schema
        let result = old_db.simple_query(
            "INSERT INTO organization_membership_profiles (id, primary_contact_email_address) VALUES (2, NULL)",
        );
        assert!(result.is_err(), "expected NULL insert to be rejected");
    });

    test.run();
}
