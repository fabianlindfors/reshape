mod common;
use common::{assert_invalid, Test};

#[test]
fn alter_column_invalid_up_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        up = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn alter_column_invalid_down_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        down = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn alter_column_invalid_default_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        [actions.changes]
        default = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn alter_column_default_with_column_reference() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        [actions.changes]
        default = "lower(name)"
        "#,
    );
}

#[test]
fn alter_column_up_invalid_column_reference() {
    let mut test = Test::new("Alter column with invalid up reference");

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
        name = "alter_column_with_bad_reference"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        up = "UPPER(non_existent)"
        down = "name"

            [actions.changes]
            type = "VARCHAR(255)"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn alter_column_down_invalid_column_reference() {
    let mut test = Test::new("Alter column with invalid down reference");

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
        name = "alter_column_with_bad_reference"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        up = "name"
        down = "non_existent"

            [actions.changes]
            type = "VARCHAR(255)"
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn alter_column_rename_down_uses_old_name() {
    let mut test = Test::new("Alter column rename with down using old name");

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

    // The column is available under its old name in both `up` and `down`, even though it
    // is exposed under the new name in the new schema
    test.second_migration(
        r#"
        name = "rename_name_to_full_name"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        up = "UPPER(name)"
        down = "LOWER(name)"

            [actions.changes]
            name = "full_name"
            type = "VARCHAR(255)"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, name) VALUES (1, 'john')")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        let full_name: String = new_db
            .query_one("SELECT full_name FROM users WHERE id = 1", &[])
            .unwrap()
            .get("full_name");
        assert_eq!("JOHN", full_name);

        new_db
            .simple_query("INSERT INTO users (id, full_name) VALUES (2, 'JANE')")
            .unwrap();
        let name: String = old_db
            .query_one("SELECT name FROM users WHERE id = 2", &[])
            .unwrap()
            .get("name");
        assert_eq!("jane", name);
    });

    test.run();
}

#[test]
fn alter_column_keeps_indexes_referencing_column() {
    let mut test = Test::new("Alter column keeps indexes referencing it");

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
            name = "status"
            type = "TEXT"

            [[actions.columns]]
            name = "name"
            type = "TEXT"

        # Indexes referencing the column as a key, in an expression, in a predicate and
        # with sort options. All of these must survive the column being replaced.
        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_active_idx"
            columns = ["id"]
            where = "status = 'active'"

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_lower_status_idx"
            columns = [{ expression = "lower(status)" }]

        [[actions]]
        type = "add_index"
        table = "users"

            [actions.index]
            name = "users_mixed_idx"
            unique = true
            columns = [
                "id",
                { expression = "lower(status)", direction = "DESC", nulls = "LAST" },
            ]
        "#,
    );

    test.second_migration(
        r#"
        name = "alter_status_type"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "status"
        up = "status"
        down = "status"

            [actions.changes]
            type = "VARCHAR(50)"
        "#,
    );

    test.intermediate(|db, _| {
        // Each index has been duplicated onto the temporary column
        let temp_definitions = index_definitions(db, "__reshape%");
        assert_eq!(3, temp_definitions.len(), "got: {:?}", temp_definitions);
        for definition in &temp_definitions {
            assert!(
                definition.contains("__reshape"),
                "expected temporary index to reference temporary column, got: {}",
                definition
            );
        }
    });

    test.after_completion(|db| {
        // The original indexes remain under their names, now on the new column
        let definitions = index_definitions(db, "users_%_idx");
        assert_eq!(3, definitions.len(), "got: {:?}", definitions);
        for definition in &definitions {
            assert!(
                definition.contains("status") && !definition.contains("__reshape"),
                "expected index to reference the final column, got: {}",
                definition
            );
        }

        let mixed = index_definitions(db, "users_mixed_idx").remove(0);
        assert!(mixed.starts_with("CREATE UNIQUE INDEX"), "got: {}", mixed);
        assert!(mixed.contains("DESC NULLS LAST"), "got: {}", mixed);

        assert!(index_definitions(db, "__reshape%").is_empty());
    });

    test.after_abort(|db| {
        assert_eq!(3, index_definitions(db, "users_%_idx").len());
        assert!(index_definitions(db, "__reshape%").is_empty());
    });

    test.run();
}

#[test]
fn alter_column_keeps_check_constraints_referencing_column() {
    let mut test = Test::new("Alter column keeps check constraints referencing it");

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
            name = "score"
            type = "INTEGER"

            [[actions.columns]]
            name = "max_score"
            type = "INTEGER"

            # One check on the column alone and one spanning another column. Both must
            # survive the column being replaced.
            [[actions.checks]]
            name = "users_score_positive"
            expression = "score >= 0"

            [[actions.checks]]
            name = "users_score_within_max"
            expression = "score <= max_score"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, score, max_score) VALUES (1, 5, 10)")
            .unwrap();
    });

    test.second_migration(
        r#"
        name = "alter_score"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "score"
        up = "score"
        down = "score"

            [actions.changes]
            name = "points"
            type = "BIGINT"
        "#,
    );

    test.intermediate(|old_db, new_db| {
        // Each check has been copied onto the temporary column and validated
        let temp_definitions = check_definitions(old_db, "__reshape%");
        assert_eq!(2, temp_definitions.len(), "got: {:?}", temp_definitions);
        for definition in &temp_definitions {
            assert!(
                definition.contains("__reshape"),
                "expected temporary check to reference temporary column, got: {}",
                definition
            );
        }
        assert!(
            check_validity(old_db, "__reshape%")
                .iter()
                .all(|valid| *valid),
            "expected temporary checks to be validated"
        );

        // The original checks are untouched
        assert_eq!(2, check_definitions(old_db, "users_score_%").len());

        // Both schemas still reject invalid rows
        assert!(old_db
            .simple_query("INSERT INTO users (id, score, max_score) VALUES (2, -1, 10)")
            .is_err());
        assert!(new_db
            .simple_query("INSERT INTO users (id, points, max_score) VALUES (2, 11, 10)")
            .is_err());
        new_db
            .simple_query("INSERT INTO users (id, points, max_score) VALUES (2, 10, 10)")
            .unwrap();
    });

    test.after_completion(|db| {
        // The checks remain under their original names, now on the new column
        let definitions = check_definitions(db, "users_score_%");
        assert_eq!(2, definitions.len(), "got: {:?}", definitions);
        for definition in &definitions {
            assert!(
                definition.contains("points") && !definition.contains("__reshape"),
                "expected check to reference the final column, got: {}",
                definition
            );
        }
        assert!(check_definitions(db, "__reshape%").is_empty());

        assert!(db
            .simple_query("INSERT INTO users (id, points, max_score) VALUES (3, -1, 10)")
            .is_err());
        assert!(db
            .simple_query("INSERT INTO users (id, points, max_score) VALUES (3, 11, 10)")
            .is_err());
        db.simple_query("INSERT INTO users (id, points, max_score) VALUES (3, 10, 10)")
            .unwrap();
    });

    test.after_abort(|db| {
        let definitions = check_definitions(db, "users_score_%");
        assert_eq!(2, definitions.len(), "got: {:?}", definitions);
        for definition in &definitions {
            assert!(
                definition.contains("score") && !definition.contains("__reshape"),
                "expected check to reference the original column, got: {}",
                definition
            );
        }
        assert!(check_definitions(db, "__reshape%").is_empty());

        assert!(db
            .simple_query("INSERT INTO users (id, score, max_score) VALUES (3, -1, 10)")
            .is_err());
    });

    test.run();
}

#[test]
fn alter_column_fails_when_up_breaks_check_constraint() {
    let mut test = Test::new("Alter column fails when up breaks a check constraint");

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
            name = "score"
            type = "INTEGER"

            [[actions.checks]]
            name = "users_score_positive"
            expression = "score >= 0"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, score) VALUES (1, 5)")
            .unwrap();
    });

    // The transformed values violate the existing check, which must fail the migration
    // rather than silently dropping the check on completion
    test.second_migration(
        r#"
        name = "alter_score"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "score"
        up = "score - 100"
        down = "score + 100"

            [actions.changes]
            type = "BIGINT"
        "#,
    );

    test.expect_failure();

    test.after_abort(|db| {
        assert_eq!(1, check_definitions(db, "users_score_%").len());
        assert!(check_definitions(db, "__reshape%").is_empty());
    });

    test.run();
}

fn check_definitions(db: &mut postgres::Client, name_pattern: &str) -> Vec<String> {
    db.query(
        "
        SELECT pg_get_constraintdef(c.oid) AS definition
        FROM pg_constraint c
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE c.contype = 'c' AND n.nspname = 'public' AND c.conname LIKE $1
        ORDER BY c.conname
        ",
        &[&name_pattern],
    )
    .unwrap()
    .iter()
    .map(|row| row.get("definition"))
    .collect()
}

fn check_validity(db: &mut postgres::Client, name_pattern: &str) -> Vec<bool> {
    db.query(
        "
        SELECT c.convalidated AS valid
        FROM pg_constraint c
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE c.contype = 'c' AND n.nspname = 'public' AND c.conname LIKE $1
        ORDER BY c.conname
        ",
        &[&name_pattern],
    )
    .unwrap()
    .iter()
    .map(|row| row.get("valid"))
    .collect()
}

fn index_definitions(db: &mut postgres::Client, name_pattern: &str) -> Vec<String> {
    db.query(
        "
        SELECT indexdef
        FROM pg_indexes
        WHERE schemaname = 'public' AND indexname LIKE $1
        ORDER BY indexname
        ",
        &[&name_pattern],
    )
    .unwrap()
    .iter()
    .map(|row| row.get("indexdef"))
    .collect()
}

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

        // Ensure NULL can't be inserted using the new schema
        let result = new_db.simple_query("INSERT INTO users (id, name) VALUES (5, NULL)");
        assert!(result.is_err(), "expected insert to fail");
    });

    test.after_completion(|db| {
        // Ensure NULL can't be inserted
        let result = db.simple_query("INSERT INTO users (id, name) VALUES (5, NULL)");
        assert!(result.is_err(), "expected insert to fail");

        common::assert_not_null_constraint_name(db, "users", "name");
    });

    test.after_abort(|db| {
        // Ensure NULL can be inserted
        let result = db.simple_query("INSERT INTO users (id, name) VALUES (5, NULL)");
        assert!(result.is_ok(), "expected insert to succeed");
    });

    test.run();
}

#[test]
fn alter_column_set_nullable() {
    let mut test = Test::new("Set column nullable");

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
        name = "set_name_nullable"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "name"
        down = "COALESCE(name, 'TEST_DEFAULT_VALUE')"

            [actions.changes]
            nullable = true
        "#,
    );

    test.after_first(|db| {
        // Insert a test user
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe')
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Insert data using new schema and make sure the old schema gets correct values
        new_db
            .simple_query("INSERT INTO users (id, name) VALUES (2, NULL)")
            .unwrap();
        let result = old_db
            .query_one("SELECT name from users WHERE id = 2", &[])
            .unwrap();
        assert_eq!("TEST_DEFAULT_VALUE", result.get::<_, &str>("name"));

        // Ensure NULL can't be inserted using the old schema
        let result = old_db.simple_query("INSERT INTO users (id, name) VALUES (3, NULL)");
        assert!(result.is_err(), "expected insert to fail");

        // Ensure NULL can be inserted using the new schema
        let result = new_db.simple_query("INSERT INTO users (id, name) VALUES (4, NULL)");
        assert!(result.is_ok(), "expected insert to succeed");
    });

    test.after_completion(|db| {
        // Ensure NULL can be inserted
        let result = db.simple_query("INSERT INTO users (id, name) VALUES (5, NULL)");
        assert!(result.is_ok(), "expected insert to succeed");
    });

    test.after_abort(|db| {
        // Ensure NULL can't be inserted
        let result = db.simple_query("INSERT INTO users (id, name) VALUES (5, NULL)");
        assert!(result.is_err(), "expected insert to fail");
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
            nullable = false
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

    test.after_completion(|db| {
        common::assert_not_null_constraint_name(db, "users", "full_name");
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
            .next()
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
            .next()
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

#[test]
fn alter_column_with_unique_index() {
    let mut test = Test::new("Alter column with unique index");

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
            unique = true
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
        db.simple_query("INSERT INTO users (id, name) VALUES (1, 'Test')")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Try inserting a value which duplicates the uppercase value of an existing row
        let result = new_db.simple_query("INSERT INTO users (id, name) VALUES (2, 'TEST')");
        assert!(
            result.is_err(),
            "expected duplicate insert to new schema to fail"
        );

        // Try inserting a value which duplicates the lowercase value of an existing row
        new_db
            .simple_query("INSERT INTO users (id, name) VALUES (2, 'JOHN')")
            .unwrap();
        let result = old_db.simple_query("INSERT INTO users (id, name) VALUES (3, 'john')");
        assert!(
            result.is_err(),
            "expected duplicate insert to old schema to fail"
        );
    });

    test.after_completion(|db| {
        // Make sure index still exists
        let is_unique: bool = db
            .query(
                "
                SELECT pg_index.indisunique
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                WHERE pg_class.relname = 'name_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get("indisunique"))
            .unwrap();
        assert!(is_unique, "expected index to still be unique");
    });

    test.run();
}

#[test]
fn alter_column_rename_and_change_type() {
    let mut test = Test::new("Rename column and change type");

    test.first_migration(
        r#"
        name = "create_accounts_table"

        [[actions]]
        type = "create_table"
        name = "accounts"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "BIGINT"

            [[actions.columns]]
            name = "balance_cents"
            type = "BIGINT"
            nullable = false
        "#,
    );

    test.second_migration(
        r#"
        name = "balance_amount"

        [[actions]]
        type = "alter_column"
        table = "accounts"
        column = "balance_cents"
        up = "balance_cents::numeric / 100"
        down = "CASE WHEN balance_cents = round(balance_cents, 2) THEN (balance_cents * 100)::bigint ELSE NULL::bigint END"

            [actions.changes]
            name = "balance_amount"
            type = "NUMERIC(20,2)"
        "#,
    );

    test.after_first(|db| {
        db.simple_query(
            "
            INSERT INTO accounts (id, balance_cents) VALUES
                (1, 1000),
                (2, 250);
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // The new schema should expose the renamed column with the new type
        let expected = vec!["10.00", "2.50"];
        assert!(
            new_db
                .query("SELECT balance_amount::TEXT FROM accounts ORDER BY id", &[],)
                .unwrap()
                .iter()
                .map(|row| row.get::<_, String>("balance_amount"))
                .eq(expected),
            "expected new schema to expose balance_amount"
        );

        // The old schema should still expose the old column with the old type
        let expected = vec![1000i64, 250];
        assert!(old_db
            .query("SELECT balance_cents FROM accounts ORDER BY id", &[])
            .unwrap()
            .iter()
            .map(|row| row.get::<_, i64>("balance_cents"))
            .eq(expected));

        // The old column name shouldn't be available in the new schema
        assert!(
            new_db
                .query("SELECT balance_cents FROM accounts", &[])
                .is_err(),
            "expected balance_cents to not exist in new schema"
        );

        // Insert using the old schema and check the new schema gets the right value
        old_db
            .simple_query("INSERT INTO accounts (id, balance_cents) VALUES (3, 1999)")
            .unwrap();
        let result = new_db
            .query_one(
                "SELECT balance_amount::TEXT FROM accounts WHERE id = 3",
                &[],
            )
            .unwrap();
        assert_eq!("19.99", result.get::<_, &str>("balance_amount"));

        // Insert using the new schema and check the old schema gets the right value
        new_db
            .simple_query("INSERT INTO accounts (id, balance_amount) VALUES (4, 5.25)")
            .unwrap();
        let result = old_db
            .query_one("SELECT balance_cents FROM accounts WHERE id = 4", &[])
            .unwrap();
        assert_eq!(525i64, result.get::<_, i64>("balance_cents"));
    });

    test.after_completion(|db| {
        let (name, data_type): (String, String) = db
            .query_one(
                "
                SELECT column_name, data_type
                FROM information_schema.columns
                WHERE table_schema = 'public'
                AND table_name = 'accounts'
                AND column_name != 'id'
                ",
                &[],
            )
            .map(|row| (row.get("column_name"), row.get("data_type")))
            .unwrap();
        assert_eq!("balance_amount", name);
        assert_eq!("numeric", data_type);

        common::assert_not_null_constraint_name(db, "accounts", "balance_amount");
    });

    test.after_abort(|db| {
        let (name, data_type): (String, String) = db
            .query_one(
                "
                SELECT column_name, data_type
                FROM information_schema.columns
                WHERE table_schema = 'public'
                AND table_name = 'accounts'
                AND column_name != 'id'
                ",
                &[],
            )
            .map(|row| (row.get("column_name"), row.get("data_type")))
            .unwrap();
        assert_eq!("balance_cents", name);
        assert_eq!("bigint", data_type);
    });

    test.run();
}

#[test]
fn alter_column_with_hash_index() {
    let mut test = Test::new("Alter column with custom index type");

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
            type = "hash"
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

    test.after_completion(|db| {
        // Make sure index still has type GIN
        let index_type: String = db
            .query(
                "
                SELECT pg_am.amname
                FROM pg_catalog.pg_index
                JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
                JOIN pg_catalog.pg_am ON pg_class.relam = pg_am.oid
                WHERE pg_class.relname = 'name_idx'
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get("amname"))
            .unwrap();
        assert_eq!("hash", index_type);
    });

    test.run();
}
