use reshape::migrations::{Column, CreateTable, Migration, RemoveTable};

mod common;

#[test]
fn remove_table() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_table_migration =
        Migration::new("create_users_table", None).with_action(CreateTable {
            name: "users".to_string(),
            primary_key: vec!["id".to_string()],
            foreign_keys: vec![],
            columns: vec![Column {
                name: "id".to_string(),
                data_type: "INTEGER".to_string(),
                nullable: false,
                default: None,
            }],
        });
    let remove_table_migration =
        Migration::new("remove_users_table", None).with_action(RemoveTable {
            table: "users".to_string(),
        });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![
        create_table_migration.clone(),
        remove_table_migration.clone(),
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

    // Make sure inserts work against the old schema
    old_db
        .simple_query("INSERT INTO users(id) VALUES (1)")
        .unwrap();

    // Ensure the table is not accessible through the new schema
    assert!(new_db.query("SELECT id FROM users", &[]).is_err());

    reshape.complete_migration().unwrap();
}
