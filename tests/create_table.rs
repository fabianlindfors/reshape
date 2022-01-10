use reshape::{
    migrations::{ColumnBuilder, CreateTableBuilder, ForeignKey, Migration},
    Status,
};

mod common;

#[test]
fn create_table() {
    let (mut reshape, mut db, _) = common::setup();

    let create_table_migration = Migration::new("create_users_table", None).with_action(
        CreateTableBuilder::default()
            .name("users")
            .primary_key(vec!["id".to_string()])
            .columns(vec![
                ColumnBuilder::default()
                    .name("id")
                    .data_type("INTEGER")
                    .generated("ALWAYS AS IDENTITY")
                    .build()
                    .unwrap(),
                ColumnBuilder::default()
                    .name("name")
                    .data_type("TEXT")
                    .build()
                    .unwrap(),
                ColumnBuilder::default()
                    .name("created_at")
                    .data_type("TIMESTAMP")
                    .nullable(false)
                    .default_value("NOW()")
                    .build()
                    .unwrap(),
            ])
            .build()
            .unwrap(),
    );

    reshape
        .migrate(vec![create_table_migration.clone()])
        .unwrap();
    assert!(matches!(reshape.state.status, Status::Idle));
    assert_eq!(
        Some(&create_table_migration.name),
        reshape.state.current_migration.as_ref()
    );

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
            WHERE i.indrelid = 'users'::regclass AND i.indisprimary
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get("column"))
        .collect();

    assert_eq!(vec!["id"], primary_key_columns);
    common::assert_cleaned_up(&mut db);
}

#[test]
fn create_table_with_foreign_keys() {
    let (mut reshape, mut db, _) = common::setup();

    let create_table_migration = Migration::new("create_users_table", None).with_action(
        CreateTableBuilder::default()
            .name("users")
            .primary_key(vec!["id".to_string()])
            .columns(vec![ColumnBuilder::default()
                .name("id")
                .data_type("INTEGER")
                .generated("ALWAYS AS IDENTITY")
                .build()
                .unwrap()])
            .build()
            .unwrap(),
    );

    let create_second_table_migration = Migration::new("create_items_table", None).with_action(
        CreateTableBuilder::default()
            .name("items")
            .primary_key(vec!["id".to_string()])
            .foreign_keys(vec![ForeignKey {
                columns: vec!["user_id".to_string()],
                referenced_table: "users".to_string(),
                referenced_columns: vec!["id".to_string()],
            }])
            .columns(vec![
                ColumnBuilder::default()
                    .name("id")
                    .data_type("INTEGER")
                    .generated("ALWAYS AS IDENTITY")
                    .build()
                    .unwrap(),
                ColumnBuilder::default()
                    .name("user_id")
                    .data_type("INTEGER")
                    .nullable(false)
                    .build()
                    .unwrap(),
            ])
            .build()
            .unwrap(),
    );

    reshape
        .migrate(vec![
            create_table_migration.clone(),
            create_second_table_migration.clone(),
        ])
        .unwrap();

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

    common::assert_cleaned_up(&mut db);
}
