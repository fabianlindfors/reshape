mod common;
use common::Test;

#[test]
fn remove_foreign_key() {
    let mut test = Test::new("Remove foreign key");

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
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"

            [[actions.foreign_keys]]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
        "#,
    );

    test.second_migration(
        r#"
        name = "remove_foreign_key"

        [[actions]]
        type = "remove_foreign_key"
        table = "items"
        foreign_key = "items_user_id_fkey"
        "#,
    );

    test.after_first(|db| {
        // Insert some test users
        db.simple_query("INSERT INTO users (id) VALUES (1), (2)")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Ensure items can't be inserted if they don't reference valid users
        // The foreign key is only removed when the migration is completed so
        // it should still be enforced for the new and old schema.
        let result = old_db.simple_query("INSERT INTO items (id, user_id) VALUES (3, 3)");
        assert!(
            result.is_err(),
            "expected insert against old schema to fail"
        );

        let result = new_db.simple_query("INSERT INTO items (id, user_id) VALUES (3, 3)");
        assert!(
            result.is_err(),
            "expected insert against new schema to fail"
        );
    });

    test.after_completion(|db| {
        // Ensure items can be inserted even if they don't reference valid users
        let result = db
            .simple_query("INSERT INTO items (id, user_id) VALUES (5, 3)")
            .map(|_| ());
        assert!(
            result.is_ok(),
            "expected insert to not fail, got {:?}",
            result
        );

        // Ensure foreign key doesn't exist
        let foreign_keys = db
            .query(
                "
                SELECT tc.constraint_name
                FROM information_schema.table_constraints AS tc 
                WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name='items';
                ",
                &[],
            )
            .unwrap();
        assert!(
            foreign_keys.is_empty(),
            "expected no foreign keys to exist on items table"
        );
    });

    test.after_abort(|db| {
        // Ensure items can't be inserted if they don't reference valid users
        let result = db.simple_query("INSERT INTO items (id, user_id) VALUES (3, 3)");
        assert!(result.is_err(), "expected insert to fail");

        // Ensure foreign key still exists
        let fk_exists = !db
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
        assert!(fk_exists);
    });

    test.run()
}
