mod common;
use common::Test;

#[test]
fn add_foreign_key() {
    let mut test = Test::new("Add foreign key");

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
        
        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_foreign_key"

        [[actions]]
        type = "add_foreign_key"
        table = "items"

            [actions.foreign_key]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
        "#,
    );

    test.after_first(|db| {
        // Insert some test users
        db.simple_query("INSERT INTO users (id) VALUES (1), (2)")
            .unwrap();
    });

    test.intermediate(|db, _| {
        // Ensure items can be inserted if they reference valid users
        db.simple_query("INSERT INTO items (id, user_id) VALUES (1, 1), (2, 2)")
            .unwrap();

        // Ensure items can't be inserted if they don't reference valid users
        let result = db.simple_query("INSERT INTO items (id, user_id) VALUES (3, 3)");
        assert!(result.is_err(), "expected insert to fail");
    });

    test.after_completion(|db| {
        // Ensure items can be inserted if they reference valid users
        db.simple_query("INSERT INTO items (id, user_id) VALUES (3, 1), (4, 2)")
            .unwrap();

        // Ensure items can't be inserted if they don't reference valid users
        let result = db.simple_query("INSERT INTO items (id, user_id) VALUES (5, 3)");
        assert!(result.is_err(), "expected insert to fail");

        // Ensure foreign key exists with the right name
        let foreign_key_name: Option<String> = db
            .query(
                "
                SELECT tc.constraint_name
                FROM information_schema.table_constraints AS tc 
                WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name='items';
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| row.get(0));
        assert_eq!(Some("items_user_id_fkey".to_string()), foreign_key_name);
    });

    test.after_abort(|db| {
        // Ensure foreign key doesn't exist
        let fk_does_not_exist = db
            .query(
                "
                SELECT tc.constraint_name
                FROM information_schema.table_constraints AS tc 
                WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name='items';
                ",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(fk_does_not_exist);
    });

    test.run()
}

#[test]
fn add_invalid_foreign_key() {
    let mut test = Test::new("Add invalid foreign key");

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
        
        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_foreign_key"

        [[actions]]
        type = "add_foreign_key"
        table = "items"

            [actions.foreign_key]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
        "#,
    );

    test.after_first(|db| {
        // Insert some items which don't reference a valid user
        db.simple_query("INSERT INTO items (id, user_id) VALUES (1, 1), (2, 2)")
            .unwrap();
    });

    test.expect_failure();
    test.run()
}
