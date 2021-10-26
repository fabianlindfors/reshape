use reshape::{
    migrations::{Column, CreateTable, Migration},
    Status,
};

mod common;

#[test]
fn create_table() {
    let (mut reshape, mut db, _) = common::setup();

    let create_table_migration =
        Migration::new("create_users_table", None).with_action(CreateTable {
            name: "users".to_string(),
            primary_key: Some("id".to_string()),
            columns: vec![
                Column {
                    name: "id".to_string(),
                    data_type: "SERIAL".to_string(),
                    nullable: true, // Will be ignored by Postgres as the column is a SERIAL
                    default: None,
                },
                Column {
                    name: "name".to_string(),
                    data_type: "TEXT".to_string(),
                    nullable: true,
                    default: None,
                },
                Column {
                    name: "created_at".to_string(),
                    data_type: "TIMESTAMP".to_string(),
                    nullable: false,
                    default: Some("NOW()".to_string()),
                },
            ],
        });

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
    assert!(id_row.get::<_, Option<String>>("column_default").is_some());
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
}
