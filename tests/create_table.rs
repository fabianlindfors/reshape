mod common;
use common::Test;

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
