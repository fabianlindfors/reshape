mod common;
use common::{assert_invalid_sql, Test};
use reshape::migrations::Migration;

#[test]
fn create_table_invalid_default_sql() {
    assert_invalid_sql(
        r#"
        name = "test"
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
        default = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn create_table_invalid_up_values_sql() {
    assert_invalid_sql(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        [actions.up]
        table = "other"
        [actions.up.values]
        id = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn create_table() {
    let mut test = Test::new("Create table");

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
            generated = "ALWAYS AS IDENTITY"

            [[actions.columns]]
            name = "name"
            type = "TEXT"

            [[actions.columns]]
            name = "created_at"
            type = "TIMESTAMP"
            nullable = false
            default = "NOW()"
        "#,
    );

    test.after_first(|db| {
        // Ensure table was created
        let result = db
            .query_opt(
                "
                SELECT table_name
                FROM information_schema.tables
                WHERE table_name = 'users' AND table_schema = 'public'",
                &[],
            )
            .unwrap();
        assert!(result.is_some());

        // Ensure table has the right columns
        let result = db
            .query(
                "
                SELECT column_name, column_default, is_nullable, data_type
                FROM information_schema.columns
                WHERE table_name = 'users' AND table_schema = 'public'
                ORDER BY ordinal_position",
                &[],
            )
            .unwrap();

        // id column
        let id_row = &result[0];
        assert_eq!("id", id_row.get::<_, String>("column_name"));
        assert!(id_row.get::<_, Option<String>>("column_default").is_none());
        assert_eq!("NO", id_row.get::<_, String>("is_nullable"));
        assert_eq!("integer", id_row.get::<_, String>("data_type"));

        // name column
        let name_row = &result[1];
        assert_eq!("name", name_row.get::<_, String>("column_name"));
        assert!(name_row
            .get::<_, Option<String>>("column_default")
            .is_none());
        assert_eq!("YES", name_row.get::<_, String>("is_nullable"));
        assert_eq!("text", name_row.get::<_, String>("data_type"));

        // created_at column
        let created_at_column = &result[2];
        assert_eq!(
            "created_at",
            created_at_column.get::<_, String>("column_name")
        );
        assert!(created_at_column
            .get::<_, Option<String>>("column_default")
            .is_some());
        assert_eq!("NO", created_at_column.get::<_, String>("is_nullable"));
        assert_eq!(
            "timestamp without time zone",
            created_at_column.get::<_, String>("data_type")
        );

        // Ensure the primary key has the right columns
        let primary_key_columns: Vec<String> = db
            .query(
                "
                SELECT a.attname AS column
                FROM pg_index i
                JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
                JOIN pg_class t ON t.oid = i.indrelid
                WHERE t.relname = 'users' AND i.indisprimary
                ",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.get("column"))
            .collect();

        assert_eq!(vec!["id"], primary_key_columns);
    });

    test.run();
}

#[test]
fn create_table_with_foreign_keys() {
    let mut test = Test::new("Create table");

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
            generated = "ALWAYS AS IDENTITY"

            [[actions.columns]]
            name = "name"
            type = "TEXT"

            [[actions.columns]]
            name = "created_at"
            type = "TIMESTAMP"
            nullable = false
            default = "NOW()"

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
            nullable = false

            [[actions.foreign_keys]]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
        "#,
    );

    test.after_first(|db| {
        let foreign_key_columns: Vec<(String, String, String)> = db
            .query(
                "
                SELECT
                    kcu.column_name, 
                    ccu.table_name AS foreign_table_name,
                    ccu.column_name AS foreign_column_name 
                FROM 
                    information_schema.table_constraints AS tc 
                    JOIN information_schema.key_column_usage AS kcu
                    ON tc.constraint_name = kcu.constraint_name
                    AND tc.table_schema = kcu.table_schema
                    JOIN information_schema.constraint_column_usage AS ccu
                    ON ccu.constraint_name = tc.constraint_name
                    AND ccu.table_schema = tc.table_schema
                WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name='items';
                ",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row.get("column_name"),
                    row.get("foreign_table_name"),
                    row.get("foreign_column_name"),
                )
            })
            .collect();

        assert_eq!(
            vec![("user_id".to_string(), "users".to_string(), "id".to_string())],
            foreign_key_columns
        );
    });

    test.run();
}

#[test]
fn create_table_with_referential_actions() {
    let mut test = Test::new("Create table with referential actions");

    test.first_migration(
        r#"
        name = "create_users_and_items_tables"

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
            on_delete = "CASCADE"
            on_update = "SET NULL"
        "#,
    );

    test.after_first(|db| {
        // Ensure the referential actions were applied to the constraint
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
        assert_eq!(b'n' as i8, on_update, "expected ON UPDATE SET NULL");

        db.simple_query("INSERT INTO users (id) VALUES (1), (2)")
            .unwrap();
        db.simple_query("INSERT INTO items (id, user_id) VALUES (1, 1), (2, 2)")
            .unwrap();

        // Ensure updates set the referencing column to NULL
        db.simple_query("UPDATE users SET id = 20 WHERE id = 1")
            .unwrap();
        let user_id: Option<i32> = db
            .query("SELECT user_id FROM items WHERE id = 1", &[])
            .unwrap()
            .first()
            .map(|row| row.get("user_id"))
            .unwrap();
        assert_eq!(None, user_id, "expected user_id to be set to NULL");

        // Ensure deletes cascade
        db.simple_query("DELETE FROM users WHERE id = 2").unwrap();
        let remaining = db
            .query("SELECT id FROM items WHERE id = 2", &[])
            .unwrap()
            .len();
        assert_eq!(0, remaining, "expected item to be deleted by cascade");
    });

    test.run()
}

#[test]
fn create_table_invalid_referential_action() {
    let result = toml::from_str::<Migration>(
        r#"
        name = "create_items_table"

        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.foreign_keys]]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
            on_delete = "MAYBE"
        "#,
    );

    assert!(
        result.is_err(),
        "expected an unknown referential action to be rejected"
    );
}
