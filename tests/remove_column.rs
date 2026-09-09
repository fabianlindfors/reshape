mod common;
use common::{assert_invalid, Test};
use postgres::{error::SqlState, Client};

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
        // Forward, reverse, immediate NOT NULL and deferred NOT NULL triggers
        assert_eq!(4, trigger_names.len());
        let mut unique_names = trigger_names.clone();
        unique_names.dedup();
        assert_eq!(trigger_names.len(), unique_names.len());
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

// Checks for a NOT NULL column removed with a cross-table down, for tests where the
// column exists in the old schema
fn check_not_null_column_with_complex_down(old_db: &mut Client, new_db: &mut Client) {
    // The old schema must still reject NULL in the removed column, immediately
    let error = old_db
        .simple_query("INSERT INTO users (id, email) VALUES (2, NULL)")
        .unwrap_err();
    assert_eq!(Some(&SqlState::NOT_NULL_VIOLATION), error.code());

    // The new schema doesn't have the column. A user without a profile can't be
    // committed as the column would be left empty for the old schema
    let error = new_db
        .simple_query("INSERT INTO users (id) VALUES (3)")
        .unwrap_err();
    assert_eq!(Some(&SqlState::NOT_NULL_VIOLATION), error.code());
    let count: i64 = old_db
        .query_one("SELECT COUNT(*) FROM users WHERE id = 3", &[])
        .unwrap()
        .get(0);
    assert_eq!(0, count);

    // Within a transaction, the user can be inserted before the profile it takes its
    // email from, as the check is made when the transaction commits
    new_db
        .batch_execute(
            "
            BEGIN;
            INSERT INTO users (id) VALUES (4);
            INSERT INTO profiles (user_id, email) VALUES (4, 'four@example.com');
            COMMIT;
            ",
        )
        .unwrap();
    let email: String = old_db
        .query_one("SELECT email FROM users WHERE id = 4", &[])
        .unwrap()
        .get("email");
    assert_eq!("four@example.com", email);

    // A transaction which leaves the column empty fails when committing
    let error = new_db
        .batch_execute(
            "
            BEGIN;
            INSERT INTO users (id) VALUES (5);
            COMMIT;
            ",
        )
        .unwrap_err();
    assert_eq!(Some(&SqlState::NOT_NULL_VIOLATION), error.code());
    let count: i64 = old_db
        .query_one("SELECT COUNT(*) FROM users WHERE id = 5", &[])
        .unwrap()
        .get(0);
    assert_eq!(0, count);

    // Writes to the source table in the new schema fill in the removed column
    new_db
        .simple_query("UPDATE profiles SET email = 'test2@example.com' WHERE user_id = 1")
        .unwrap();
    let email: String = old_db
        .query_one("SELECT email FROM users WHERE id = 1", &[])
        .unwrap()
        .get("email");
    assert_eq!("test2@example.com", email);
}

// Asserts that NOT NULL is back on users.email once the migration has been aborted
fn assert_email_not_null(db: &mut Client) {
    let is_nullable: String = db
        .query_one(
            "
            SELECT is_nullable
            FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = 'users' AND column_name = 'email'
            ",
            &[],
        )
        .unwrap()
        .get("is_nullable");
    assert_eq!("NO", is_nullable);
}

#[test]
fn remove_column_not_null_with_complex_down() {
    let mut test = Test::new("Remove NOT NULL column with complex down");

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

    test.intermediate(check_not_null_column_with_complex_down);
    test.after_abort(assert_email_not_null);

    test.run();
}

#[test]
fn remove_column_not_null_from_earlier_in_flight_alter_with_complex_down() {
    let mut test = Test::new("Remove NOT NULL column altered earlier in the batch, complex down");

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

    // The column is altered first, so remove_column sees it backed by the nullable
    // temporary column rather than the NOT NULL real one
    test.second_migration(
        r#"
        name = "change_default_then_remove_users_email_column"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "email"

            [actions.changes]
            default = "'unknown@example.com'"

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

    test.intermediate(check_not_null_column_with_complex_down);
    test.after_abort(assert_email_not_null);

    test.run();
}

#[test]
fn remove_column_not_null_from_in_flight_add_column_with_complex_down() {
    let mut test = Test::new("Remove NOT NULL column added earlier in the batch, complex down");

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

    // The column only exists as the temporary column of add_column, so there is no
    // real column for remove_column to lift NOT NULL from or to reinstate it on
    test.second_migration(
        r#"
        name = "add_then_remove_users_email_column"

        [[actions]]
        type = "add_column"
        table = "users"
        up = "'added@example.com'"

            [actions.column]
            name = "email"
            type = "TEXT"
            nullable = false

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
        db.simple_query("INSERT INTO users (id) VALUES (1)")
            .unwrap();
        db.simple_query("INSERT INTO profiles (user_id, email) VALUES (1, 'test@example.com')")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // The old schema doesn't have the column and `up` fills it in
        old_db
            .simple_query("INSERT INTO users (id) VALUES (2)")
            .unwrap();

        // The new schema doesn't have the column either, and must insert a profile to take
        // the value from before the transaction commits
        let error = new_db
            .simple_query("INSERT INTO users (id) VALUES (3)")
            .unwrap_err();
        assert_eq!(Some(&SqlState::NOT_NULL_VIOLATION), error.code());

        new_db
            .batch_execute(
                "
                BEGIN;
                INSERT INTO users (id) VALUES (4);
                INSERT INTO profiles (user_id, email) VALUES (4, 'four@example.com');
                COMMIT;
                ",
            )
            .unwrap();
    });

    test.run();
}
