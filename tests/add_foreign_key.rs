mod common;
use common::Test;
use reshape::migrations::Migration;

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

#[test]
fn add_foreign_key_with_referential_actions() {
    let mut test = Test::new("Add foreign key with referential actions");

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
            on_delete = "CASCADE"
            on_update = "CASCADE"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO users (id) VALUES (1), (2), (3)")
            .unwrap();
        db.simple_query("INSERT INTO items (id, user_id) VALUES (1, 1), (2, 2), (3, 3)")
            .unwrap();
    });

    test.intermediate(|db, _| {
        // Ensure the referential actions are in place as soon as the migration has started
        let (on_delete, on_update): (i8, i8) = db
            .query(
                "
                SELECT confdeltype, confupdtype
                FROM pg_constraint
                WHERE contype = 'f' AND conrelid = 'public.items'::regclass
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| (row.get("confdeltype"), row.get("confupdtype")))
            .unwrap();
        assert_eq!(b'c' as i8, on_delete, "expected ON DELETE CASCADE");
        assert_eq!(b'c' as i8, on_update, "expected ON UPDATE CASCADE");

        // Ensure deletes cascade
        db.simple_query("DELETE FROM users WHERE id = 1").unwrap();
        let remaining = db
            .query("SELECT id FROM items WHERE id = 1", &[])
            .unwrap()
            .len();
        assert_eq!(0, remaining, "expected item to be deleted by cascade");

        // Ensure updates cascade
        db.simple_query("UPDATE users SET id = 20 WHERE id = 2")
            .unwrap();
        let user_id: i32 = db
            .query("SELECT user_id FROM items WHERE id = 2", &[])
            .unwrap()
            .first()
            .map(|row| row.get("user_id"))
            .unwrap();
        assert_eq!(20, user_id, "expected item to be updated by cascade");
    });

    test.after_completion(|db| {
        // Ensure the referential actions survive the constraint being renamed
        let (name, on_delete, on_update): (String, i8, i8) = db
            .query(
                "
                SELECT conname, confdeltype, confupdtype
                FROM pg_constraint
                WHERE contype = 'f' AND conrelid = 'public.items'::regclass
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| {
                (
                    row.get("conname"),
                    row.get("confdeltype"),
                    row.get("confupdtype"),
                )
            })
            .unwrap();
        assert_eq!("items_user_id_fkey", name);
        assert_eq!(b'c' as i8, on_delete, "expected ON DELETE CASCADE");
        assert_eq!(b'c' as i8, on_update, "expected ON UPDATE CASCADE");

        db.simple_query("DELETE FROM users WHERE id = 3").unwrap();
        let remaining = db
            .query("SELECT id FROM items WHERE id = 3", &[])
            .unwrap()
            .len();
        assert_eq!(0, remaining, "expected item to be deleted by cascade");
    });

    test.after_abort(|db| {
        // Ensure the foreign key, and with it the cascade, is gone
        let fk_does_not_exist = db
            .query(
                "
                SELECT conname
                FROM pg_constraint
                WHERE contype = 'f' AND conrelid = 'public.items'::regclass
                ",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(fk_does_not_exist);

        db.simple_query("DELETE FROM users WHERE id = 3").unwrap();
        let remaining = db
            .query("SELECT id FROM items WHERE id = 3", &[])
            .unwrap()
            .len();
        assert_eq!(1, remaining, "expected item to be left untouched");
    });

    test.run()
}

#[test]
fn add_foreign_key_invalid_referential_action() {
    let result = toml::from_str::<Migration>(
        r#"
        name = "add_foreign_key"

        [[actions]]
        type = "add_foreign_key"
        table = "items"

            [actions.foreign_key]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
            on_delete = "DROP EVERYTHING"
        "#,
    );

    assert!(
        result.is_err(),
        "expected an unknown referential action to be rejected"
    );
}
