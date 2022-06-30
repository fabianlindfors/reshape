mod common;
use common::Test;

#[test]
fn remove_table() {
    let mut test = Test::new("Remove table");

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
        name = "remove_users_table"

        [[actions]]
        type = "remove_table"
        table = "users"
        "#,
    );

    test.intermediate(|old_db, new_db| {
        // Make sure inserts work against the old schema
        old_db
            .simple_query("INSERT INTO users(id) VALUES (1)")
            .unwrap();

        // Ensure the table is not accessible through the new schema
        assert!(new_db.query("SELECT id FROM users", &[]).is_err());
    });

    test.run();
}
