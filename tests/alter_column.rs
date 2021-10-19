use reshape::migrations::{AlterColumn, Column, ColumnChanges, CreateTable, Migration};
use reshape::Status;

mod common;

#[test]
fn alter_column_data() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(CreateTable {
        name: "users".to_string(),
        columns: vec![
            Column {
                name: "id".to_string(),
                data_type: "SERIAL".to_string(),
                nullable: true,
                default: None,
            },
            Column {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
                default: None,
            },
        ],
    });
    let uppercase_name = Migration::new("uppercase_name", None).with_action(AlterColumn {
        table: "users".to_string(),
        column: "name".to_string(),
        up: Some("UPPER(name)".to_string()),
        down: Some("LOWER(name)".to_string()),
        changes: ColumnChanges {
            data_type: None,
            nullable: None,
            name: None,
        },
    });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), uppercase_name.clone()];

    // Run first migration, should automatically finish
    reshape.migrate(first_migrations.clone()).unwrap();
    assert!(matches!(reshape.state.status, Status::Idle));
    assert_eq!(
        Some(&create_users_table.name),
        reshape.state.current_migration.as_ref()
    );

    // Update search paths
    old_db
        .simple_query(&reshape::generate_schema_query(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert some test users
    old_db
        .simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'john Doe'),
                (2, 'jane Doe');
            ",
        )
        .unwrap();

    // Run second migration
    reshape.migrate(second_migrations.clone()).unwrap();
    new_db
        .simple_query(&reshape::generate_schema_query(
            &second_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Check that the existing users has the altered data
    let expected = vec!["JOHN DOE", "JANE DOE"];
    assert!(new_db
        .query("SELECT name FROM users ORDER BY id", &[],)
        .unwrap()
        .iter()
        .map(|row| row.get::<_, String>("name"))
        .eq(expected));

    // Insert data using old schema and make sure the new schema gets correct values
    old_db
        .simple_query("INSERT INTO users (id, name) VALUES (3, 'test testsson')")
        .unwrap();
    let result = new_db
        .query_one("SELECT name from users WHERE id = 3", &[])
        .unwrap();
    assert_eq!("TEST TESTSSON", result.get::<_, &str>("name"));

    // Insert data using new schema and make sure the old schema gets correct values
    new_db
        .simple_query("INSERT INTO users (id, name) VALUES (4, 'TEST TESTSSON')")
        .unwrap();
    let result = old_db
        .query_one("SELECT name from users WHERE id = 4", &[])
        .unwrap();
    assert_eq!("test testsson", result.get::<_, &str>("name"));

    reshape.complete_migration().unwrap();
}

#[test]
fn alter_column_set_not_null() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(CreateTable {
        name: "users".to_string(),
        columns: vec![
            Column {
                name: "id".to_string(),
                data_type: "SERIAL".to_string(),
                nullable: true,
                default: None,
            },
            Column {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                nullable: true,
                default: None,
            },
        ],
    });
    let set_name_not_null = Migration::new("set_name_not_null", None).with_action(AlterColumn {
        table: "users".to_string(),
        column: "name".to_string(),
        up: Some("COALESCE(name, 'TEST_DEFAULT_VALUE')".to_string()),
        down: Some("name".to_string()),
        changes: ColumnChanges {
            data_type: None,
            nullable: Some(false),
            name: None,
        },
    });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), set_name_not_null.clone()];

    // Run first migration, should automatically finish
    reshape.migrate(first_migrations.clone()).unwrap();
    assert!(matches!(reshape.state.status, Status::Idle));
    assert_eq!(
        Some(&create_users_table.name),
        reshape.state.current_migration.as_ref()
    );

    // Update search paths
    old_db
        .simple_query(&reshape::generate_schema_query(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert some test users
    old_db
        .simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe'),
                (2, NULL);
            ",
        )
        .unwrap();

    // Run second migration
    reshape.migrate(second_migrations.clone()).unwrap();
    new_db
        .simple_query(&reshape::generate_schema_query(
            &second_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Check that existing users got the correct values
    let expected = vec!["John Doe", "TEST_DEFAULT_VALUE"];
    assert!(new_db
        .query("SELECT name FROM users ORDER BY id", &[],)
        .unwrap()
        .iter()
        .map(|row| row.get::<_, String>("name"))
        .eq(expected));

    // Insert data using old schema and make sure the new schema gets correct values
    old_db
        .simple_query("INSERT INTO users (id, name) VALUES (3, NULL)")
        .unwrap();
    let result = new_db
        .query_one("SELECT name from users WHERE id = 3", &[])
        .unwrap();
    assert_eq!("TEST_DEFAULT_VALUE", result.get::<_, &str>("name"));

    // Insert data using new schema and make sure the old schema gets correct values
    new_db
        .simple_query("INSERT INTO users (id, name) VALUES (4, 'Jane Doe')")
        .unwrap();
    let result = old_db
        .query_one("SELECT name from users WHERE id = 4", &[])
        .unwrap();
    assert_eq!("Jane Doe", result.get::<_, &str>("name"));

    reshape.complete_migration().unwrap();
}

#[test]
fn alter_column_rename() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(CreateTable {
        name: "users".to_string(),
        columns: vec![
            Column {
                name: "id".to_string(),
                data_type: "SERIAL".to_string(),
                nullable: true,
                default: None,
            },
            Column {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                nullable: true,
                default: None,
            },
        ],
    });
    let rename_to_full_name =
        Migration::new("rename_to_full_name", None).with_action(AlterColumn {
            table: "users".to_string(),
            column: "name".to_string(),
            up: None, // up and down are not required when only renaming a column
            down: None,
            changes: ColumnChanges {
                data_type: None,
                nullable: None,
                name: Some("full_name".to_string()),
            },
        });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), rename_to_full_name.clone()];

    // Run first migration, should automatically finish
    reshape.migrate(first_migrations.clone()).unwrap();
    assert!(matches!(reshape.state.status, Status::Idle));
    assert_eq!(
        Some(&create_users_table.name),
        reshape.state.current_migration.as_ref()
    );

    // Update search paths
    old_db
        .simple_query(&reshape::generate_schema_query(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert some test data
    old_db
        .simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe'),
                (2, 'Jane Doe');
            ",
        )
        .unwrap();

    // Run second migration
    reshape.migrate(second_migrations.clone()).unwrap();
    new_db
        .simple_query(&reshape::generate_schema_query(
            &second_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Check that existing values can be queried using new column name
    let expected = vec!["John Doe", "Jane Doe"];
    assert!(new_db
        .query("SELECT full_name FROM users ORDER BY id", &[],)
        .unwrap()
        .iter()
        .map(|row| row.get::<_, String>("full_name"))
        .eq(expected));

    reshape.complete_migration().unwrap();
}
