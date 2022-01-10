use reshape::migrations::{ColumnBuilder, CreateTableBuilder, Migration, RenameTable};

mod common;

#[test]
fn rename_table() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_table_migration = Migration::new("create_user_table", None).with_action(
        CreateTableBuilder::default()
            .name("users")
            .primary_key(vec!["id".to_string()])
            .columns(vec![ColumnBuilder::default()
                .name("id")
                .data_type("INTEGER")
                .build()
                .unwrap()])
            .build()
            .unwrap(),
    );
    let rename_table_migration = Migration::new("rename_users_table_to_customers", None)
        .with_action(RenameTable {
            table: "users".to_string(),
            new_name: "customers".to_string(),
        });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![
        create_table_migration.clone(),
        rename_table_migration.clone(),
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

    reshape.complete_migration().unwrap();
    common::assert_cleaned_up(&mut new_db);
}
