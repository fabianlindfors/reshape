mod common;
use common::Test;

#[test]
fn invalid_migration() {
    let mut test = Test::new("Invalid migration");

    test.first_migration(
        r#"
        name = "invalid_migration"

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
        name = "add_invalid_column"

        [[actions]]
        type = "add_column"
        table = "users"

        up = "INVALID SQL"

            [actions.column]
            name = "first"
            type = "TEXT"
        "#,
    );

    // Insert a test user
    test.after_first(|db| {
        db.simple_query(
            "
            INSERT INTO users (id) VALUES (1)
            ",
        )
        .unwrap();
    });

    test.expect_failure();
    test.run();
}
