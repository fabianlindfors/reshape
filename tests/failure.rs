use reshape::migrations::{AddColumn, Column, ColumnBuilder, CreateTableBuilder, Migration};

mod common;

#[test]
fn invalid_migration() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(
        CreateTableBuilder::default()
            .name("users")
            .primary_key(vec!["id".to_string()])
            .columns(vec![
                ColumnBuilder::default()
                    .name("id")
                    .data_type("INTEGER")
                    .build()
                    .unwrap(),
                ColumnBuilder::default()
                    .name("name")
                    .data_type("TEXT")
                    .build()
                    .unwrap(),
            ])
            .build()
            .unwrap(),
    );
    let add_first_column = Migration::new("add_first_column", None).with_action(AddColumn {
        table: "users".to_string(),
        column: Column {
            name: "first".to_string(),
            data_type: "TEXT".to_string(),
            nullable: false,
            default: None,
            generated: None,
        },
        up: Some("INVALID SQL".to_string()),
    });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), add_first_column.clone()];

    // Run first migration, should automatically finish
    reshape.migrate(first_migrations.clone()).unwrap();

    // Update search paths
    old_db
        .simple_query(&reshape::schema_query_for_migration(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();
    new_db
        .simple_query(&reshape::schema_query_for_migration(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert a test user
    new_db
        .simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe')
            ",
        )
        .unwrap();

    // Run second migration and ensure it fails
    assert!(
        reshape.migrate(second_migrations.clone()).is_err(),
        "invalid migration succeeded"
    );

    common::assert_cleaned_up(&mut new_db);
}
