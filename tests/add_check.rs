mod common;
use common::{assert_invalid, check_constraint_definitions, get_constraint_comment, Test};

#[test]
fn add_check() {
    let mut test = Test::new("Add check");

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
            name = "age"
            type = "INTEGER"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_age_check"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= 0"
        "#,
    );

    test.after_first(|db| {
        // Rows which satisfy the check, including a NULL which always passes
        db.simple_query("INSERT INTO users (id, age) VALUES (1, 10), (2, NULL)")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // The check is enforced for both schemas as soon as the migration is applied
        old_db
            .simple_query("INSERT INTO users (id, age) VALUES (3, 30)")
            .unwrap();
        new_db
            .simple_query("INSERT INTO users (id, age) VALUES (4, 40)")
            .unwrap();

        let result = old_db.simple_query("INSERT INTO users (id, age) VALUES (5, -1)");
        assert!(
            result.is_err(),
            "expected insert against old schema to fail"
        );

        let result = new_db.simple_query("INSERT INTO users (id, age) VALUES (5, -1)");
        assert!(
            result.is_err(),
            "expected insert against new schema to fail"
        );

        // The temporary constraint has been validated
        let definitions = check_constraint_definitions(old_db, "__reshape%");
        assert_eq!(1, definitions.len(), "got: {:?}", definitions);
    });

    test.after_completion(|db| {
        let result = db.simple_query("INSERT INTO users (id, age) VALUES (5, -1)");
        assert!(result.is_err(), "expected insert to fail");

        // The check exists under its final name
        let definitions = check_constraint_definitions(db, "users_age_check");
        assert_eq!(1, definitions.len(), "got: {:?}", definitions);
        assert!(
            definitions[0].contains("age >= 0"),
            "got: {}",
            definitions[0]
        );
        assert!(check_constraint_definitions(db, "__reshape%").is_empty());
    });

    test.after_abort(|db| {
        // The check doesn't exist
        db.simple_query("INSERT INTO users (id, age) VALUES (5, -1)")
            .unwrap();
        assert!(check_constraint_definitions(db, "users_age_check").is_empty());
        assert!(check_constraint_definitions(db, "__reshape%").is_empty());
    });

    test.run()
}

#[test]
fn add_check_with_comment() {
    let mut test = Test::new("Add check with comment");

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
            name = "age"
            type = "INTEGER"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_age_check"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= 0"
            comment = "Ages can't be negative"
        "#,
    );

    test.after_completion(|db| {
        // The comment follows the constraint when it's renamed to its final name
        assert_eq!(
            Some("Ages can't be negative".to_string()),
            get_constraint_comment(db, "users", "users_age_check")
        );
    });

    test.after_abort(|db| {
        // The constraint, and with it the comment, was removed
        assert!(check_constraint_definitions(db, "users_age_check").is_empty());
    });

    test.run()
}

#[test]
fn add_check_with_violating_rows() {
    let mut test = Test::new("Add check with rows which violate it");

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
            name = "age"
            type = "INTEGER"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_age_check"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= 0"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, age) VALUES (1, 10), (2, -5)")
            .unwrap();
    });

    test.expect_failure();

    test.intermediate(|old_db, _| {
        // The check is dropped again when validation fails, so the old schema is
        // unaffected
        old_db
            .simple_query("INSERT INTO users (id, age) VALUES (3, -1)")
            .unwrap();
        assert!(check_constraint_definitions(old_db, "__reshape%").is_empty());
    });

    test.run()
}

#[test]
fn add_check_with_existing_name() {
    let mut test = Test::new("Add check with the name of an existing check");

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
            name = "age"
            type = "INTEGER"

            [[actions.checks]]
            name = "users_age_check"
            expression = "age >= 0"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_age_check"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= 18"
        "#,
    );

    test.expect_failure();

    test.intermediate(|old_db, _| {
        assert!(check_constraint_definitions(old_db, "__reshape%").is_empty());
    });

    test.run()
}

#[test]
fn add_check_on_altered_column() {
    let mut test = Test::new("Add check on a column altered in the same migration");

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
            name = "age"
            type = "INTEGER"
        "#,
    );

    // The check references the column by its new name, which is backed by a temporary
    // column during the migration
    test.second_migration(
        r#"
        name = "rename_age_and_add_check"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "age"
        up = "age"
        down = "age"

            [actions.changes]
            name = "years"
            type = "BIGINT"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_years_check"
            expression = "years >= 0"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, age) VALUES (1, 10)")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        let result = old_db.simple_query("INSERT INTO users (id, age) VALUES (2, -1)");
        assert!(
            result.is_err(),
            "expected insert against old schema to fail"
        );

        let result = new_db.simple_query("INSERT INTO users (id, years) VALUES (2, -1)");
        assert!(
            result.is_err(),
            "expected insert against new schema to fail"
        );

        new_db
            .simple_query("INSERT INTO users (id, years) VALUES (2, 20)")
            .unwrap();
    });

    test.after_completion(|db| {
        let definitions = check_constraint_definitions(db, "users_years_check");
        assert_eq!(1, definitions.len(), "got: {:?}", definitions);
        assert!(
            definitions[0].contains("years >= 0"),
            "got: {}",
            definitions[0]
        );

        let result = db.simple_query("INSERT INTO users (id, years) VALUES (3, -1)");
        assert!(result.is_err(), "expected insert to fail");
    });

    test.after_abort(|db| {
        db.simple_query("INSERT INTO users (id, age) VALUES (3, -1)")
            .unwrap();
        assert!(check_constraint_definitions(db, "%check").is_empty());
    });

    test.run()
}

#[test]
fn add_check_with_unknown_column() {
    let mut test = Test::new("Add check referencing an unknown column");

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
        name = "add_age_check"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= 0"
        "#,
    );

    test.expect_failure();
    test.run()
}

#[test]
fn add_check_invalid_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= )"
        "#,
    );
}
