mod common;
use common::Test;

#[test]
fn rename_table() {
    let mut test = Test::new("Rename table");

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
        name = "rename_users_table_to_customers"

        [[actions]]
        type = "rename_table"
        table = "users"
        new_name = "customers"
        "#,
    );

    test.intermediate(|old_db, new_db| {
        // Make sure inserts work using both the old and new name
        old_db
            .simple_query("INSERT INTO users(id) VALUES (1)")
            .unwrap();
        new_db
            .simple_query("INSERT INTO customers(id) VALUES (2)")
            .unwrap();

        // Ensure the table can be queried using both the old and new name
        let expected: Vec<i32> = vec![1, 2];
        assert_eq!(
            expected,
            old_db
                .query("SELECT id FROM users ORDER BY id", &[])
                .unwrap()
                .iter()
                .map(|row| row.get::<_, i32>("id"))
                .collect::<Vec<i32>>()
        );
        assert_eq!(
            expected,
            new_db
                .query("SELECT id FROM customers ORDER BY id", &[])
                .unwrap()
                .iter()
                .map(|row| row.get::<_, i32>("id"))
                .collect::<Vec<i32>>()
        );

        // Ensure the table can't be queried using the wrong name for the schema
        assert!(old_db.simple_query("SELECT id FROM customers").is_err());
        assert!(new_db.simple_query("SELECT id FROM users").is_err());
    });

    test.run();
}
