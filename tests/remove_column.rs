mod common;
use common::Test;

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
