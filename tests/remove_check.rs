mod common;
use common::{check_constraint_definitions, Test};

#[test]
fn remove_check() {
    let mut test = Test::new("Remove check");

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
        name = "remove_age_check"

        [[actions]]
        type = "remove_check"
        table = "users"
        check = "users_age_check"
        "#,
    );

    test.intermediate(|old_db, new_db| {
        // The check is only removed when the migration is completed so it should still
        // be enforced for the new and old schema
        let result = old_db.simple_query("INSERT INTO users (id, age) VALUES (1, -1)");
        assert!(
            result.is_err(),
            "expected insert against old schema to fail"
        );

        let result = new_db.simple_query("INSERT INTO users (id, age) VALUES (1, -1)");
        assert!(
            result.is_err(),
            "expected insert against new schema to fail"
        );
    });

    test.after_completion(|db| {
        db.simple_query("INSERT INTO users (id, age) VALUES (1, -1)")
            .unwrap();
        assert!(check_constraint_definitions(db, "users_age_check").is_empty());
    });

    test.after_abort(|db| {
        let result = db.simple_query("INSERT INTO users (id, age) VALUES (1, -1)");
        assert!(result.is_err(), "expected insert to fail");
        assert_eq!(1, check_constraint_definitions(db, "users_age_check").len());
    });

    test.run()
}

#[test]
fn remove_nonexistent_check() {
    let mut test = Test::new("Remove check which doesn't exist");

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
        name = "remove_age_check"

        [[actions]]
        type = "remove_check"
        table = "users"
        check = "users_age_check"
        "#,
    );

    test.expect_failure();
    test.run()
}

#[test]
fn replace_check_with_same_name() {
    let mut test = Test::new("Replace check by removing and adding it with the same name");

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

    // Widen the check to allow negative ages down to -10
    test.second_migration(
        r#"
        name = "widen_age_check"

        [[actions]]
        type = "remove_check"
        table = "users"
        check = "users_age_check"

        [[actions]]
        type = "add_check"
        table = "users"

            [actions.check]
            name = "users_age_check"
            expression = "age >= -10"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, age) VALUES (1, 10)")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Both checks exist during the migration: the old one until completion and the
        // new one from the start, so writes must satisfy both
        assert_eq!(
            1,
            check_constraint_definitions(old_db, "users_age_check").len()
        );
        assert_eq!(1, check_constraint_definitions(old_db, "__reshape%").len());

        let result = old_db.simple_query("INSERT INTO users (id, age) VALUES (2, -5)");
        assert!(
            result.is_err(),
            "expected insert against old schema to fail"
        );

        let result = new_db.simple_query("INSERT INTO users (id, age) VALUES (2, -5)");
        assert!(
            result.is_err(),
            "expected insert against new schema to fail"
        );

        let result = new_db.simple_query("INSERT INTO users (id, age) VALUES (2, -20)");
        assert!(
            result.is_err(),
            "expected insert against new schema to fail"
        );

        new_db
            .simple_query("INSERT INTO users (id, age) VALUES (2, 0)")
            .unwrap();
    });

    test.after_completion(|db| {
        // Only the new check remains, under the original name
        let definitions = check_constraint_definitions(db, "users_age_check");
        assert_eq!(1, definitions.len(), "got: {:?}", definitions);
        assert!(
            definitions[0].contains("-10"),
            "expected widened check, got: {}",
            definitions[0]
        );
        assert!(check_constraint_definitions(db, "__reshape%").is_empty());

        db.simple_query("INSERT INTO users (id, age) VALUES (3, -5)")
            .unwrap();
        let result = db.simple_query("INSERT INTO users (id, age) VALUES (4, -20)");
        assert!(result.is_err(), "expected insert to fail");
    });

    test.after_abort(|db| {
        // The original check is untouched
        let definitions = check_constraint_definitions(db, "users_age_check");
        assert_eq!(1, definitions.len(), "got: {:?}", definitions);
        assert!(
            definitions[0].contains("age >= 0"),
            "expected original check, got: {}",
            definitions[0]
        );
        assert!(check_constraint_definitions(db, "__reshape%").is_empty());

        let result = db.simple_query("INSERT INTO users (id, age) VALUES (3, -5)");
        assert!(result.is_err(), "expected insert to fail");
    });

    test.run()
}

#[test]
fn remove_check_and_alter_column() {
    let mut test = Test::new("Remove check and alter the column it references");

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

    // The removed check must not be carried over to the altered column
    test.second_migration(
        r#"
        name = "remove_age_check_and_alter_age"

        [[actions]]
        type = "remove_check"
        table = "users"
        check = "users_age_check"

        [[actions]]
        type = "alter_column"
        table = "users"
        column = "age"
        up = "age"
        down = "age"

            [actions.changes]
            type = "BIGINT"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id, age) VALUES (1, 10)")
            .unwrap();
    });

    test.intermediate(|old_db, _| {
        assert!(check_constraint_definitions(old_db, "__reshape%").is_empty());
    });

    test.after_completion(|db| {
        assert!(check_constraint_definitions(db, "%check").is_empty());
        db.simple_query("INSERT INTO users (id, age) VALUES (2, -1)")
            .unwrap();
    });

    test.after_abort(|db| {
        assert_eq!(1, check_constraint_definitions(db, "users_age_check").len());
        let result = db.simple_query("INSERT INTO users (id, age) VALUES (2, -1)");
        assert!(result.is_err(), "expected insert to fail");
    });

    test.run()
}
