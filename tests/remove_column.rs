use reshape::migrations::{Column, CreateTable, Migration, RemoveColumn};

mod common;

#[test]
fn remove_column() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_table_migration =
        Migration::new("create_users_table", None).with_action(CreateTable {
            name: "users".to_string(),
            primary_key: vec!["id".to_string()],
            foreign_keys: vec![],
            columns: vec![
                Column {
                    name: "id".to_string(),
                    data_type: "INTEGER".to_string(),
                    nullable: true,
                    default: None,
                    generated: None,
                },
                Column {
                    name: "name".to_string(),
                    data_type: "TEXT".to_string(),
                    nullable: false,
                    default: None,
                    generated: None,
                },
            ],
        });
    let remove_column_migration =
        Migration::new("remove_name_column", None).with_action(RemoveColumn {
            table: "users".to_string(),
            column: "name".to_string(),
            down: Some("'TEST_DOWN_VALUE'".to_string()),
        });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![
        create_table_migration.clone(),
        remove_column_migration.clone(),
    ];

    // Run migrations
    reshape.migrate(first_migrations.clone()).unwrap();
    reshape.migrate(second_migrations.clone()).unwrap();

    // Update schemas of Postgres connections
    let old_schema_query =
        reshape::schema_query_for_migration(&first_migrations.last().unwrap().name);
    let new_schema_query =
        reshape::schema_query_for_migration(&second_migrations.last().unwrap().name);
    old_db.simple_query(&old_schema_query).unwrap();
    new_db.simple_query(&new_schema_query).unwrap();

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

    reshape.complete_migration().unwrap();
}
