use reshape::migrations::{AddColumn, Column, CreateTable, Migration};
use reshape::Status;

mod common;

#[test]
fn add_column() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(CreateTable {
        name: "users".to_string(),
        primary_key: None,
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
    let add_first_last_name_columns = Migration::new("add_first_and_last_name_columns", None)
        .with_action(AddColumn {
            table: "users".to_string(),
            column: Column {
                name: "first".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
                default: None,
            },
            up: Some("(STRING_TO_ARRAY(name, ' '))[1]".to_string()),
        })
        .with_action(AddColumn {
            table: "users".to_string(),
            column: Column {
                name: "last".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
                default: None,
            },
            up: Some("(STRING_TO_ARRAY(name, ' '))[2]".to_string()),
        });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![
        create_users_table.clone(),
        add_first_last_name_columns.clone(),
    ];

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
    new_db
        .simple_query(&reshape::generate_schema_query(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert some test users
    new_db
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

    // Check that the existing users have the new columns populated
    let expected = vec![("John", "Doe"), ("Jane", "Doe")];
    assert!(new_db
        .query("SELECT first, last FROM users ORDER BY id", &[],)
        .unwrap()
        .iter()
        .map(|row| (row.get("first"), row.get("last")))
        .eq(expected));

    // Insert data using old schema and make sure the new columns are populated
    old_db
        .simple_query("INSERT INTO users (id, name) VALUES (3, 'Test Testsson')")
        .unwrap();
    let (first_name, last_name): (String, String) = new_db
        .query_one("SELECT first, last from users WHERE id = 3", &[])
        .map(|row| (row.get("first"), row.get("last")))
        .unwrap();
    assert_eq!(
        ("Test", "Testsson"),
        (first_name.as_ref(), last_name.as_ref())
    );

    reshape.complete_migration().unwrap();
}

#[test]
fn add_column_nullable() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(CreateTable {
        name: "users".to_string(),
        primary_key: None,
        columns: vec![Column {
            name: "id".to_string(),
            data_type: "SERIAL".to_string(),
            nullable: true,
            default: None,
        }],
    });
    let add_name_column = Migration::new("add_nullable_name_column", None).with_action(AddColumn {
        table: "users".to_string(),
        column: Column {
            name: "name".to_string(),
            data_type: "TEXT".to_string(),
            nullable: true,
            default: None,
        },
        up: None,
    });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), add_name_column.clone()];

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
    new_db
        .simple_query(&reshape::generate_schema_query(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert some test values
    new_db
        .simple_query(
            "
            INSERT INTO users (id) VALUES
                (1),
                (2);
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

    // Ensure existing data got updated
    let expected: Vec<Option<String>> = vec![None, None];
    assert!(new_db
        .query("SELECT name FROM users ORDER BY id", &[],)
        .unwrap()
        .iter()
        .map(|row| row.get::<_, Option<String>>("name"))
        .eq(expected));

    // Insert data using old schema and ensure new column is NULL
    old_db
        .simple_query("INSERT INTO users (id) VALUES (3)")
        .unwrap();
    let name: Option<String> = new_db
        .query_one("SELECT name from users WHERE id = 3", &[])
        .map(|row| (row.get("name")))
        .unwrap();
    assert_eq!(None, name);

    // Ensure data can be inserted against new schema
    new_db
        .simple_query("INSERT INTO users (id, name) VALUES (3, 'Test Testsson'), (4, NULL)")
        .unwrap();

    reshape.complete_migration().unwrap();
}

#[test]
fn add_column_with_default() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_users_table", None).with_action(CreateTable {
        name: "users".to_string(),
        columns: vec![Column {
            name: "id".to_string(),
            data_type: "SERIAL".to_string(),
            nullable: true,
            default: None,
        }],
        primary_key: None,
    });
    let add_name_column =
        Migration::new("add_name_column_with_default", None).with_action(AddColumn {
            table: "users".to_string(),
            column: Column {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
                default: Some("'DEFAULT'".to_string()),
            },
            up: None,
        });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), add_name_column.clone()];

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
    new_db
        .simple_query(&reshape::generate_schema_query(
            &first_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Insert some test values
    new_db
        .simple_query("INSERT INTO users (id) VALUES (1), (2)")
        .unwrap();

    // Run second migration
    reshape.migrate(second_migrations.clone()).unwrap();
    new_db
        .simple_query(&reshape::generate_schema_query(
            &second_migrations.last().unwrap().name,
        ))
        .unwrap();

    // Ensure existing data got updated with defaults
    let expected = vec!["DEFAULT".to_string(), "DEFAULT".to_string()];
    assert!(new_db
        .query("SELECT name FROM users ORDER BY id", &[],)
        .unwrap()
        .iter()
        .map(|row| row.get::<_, String>("name"))
        .eq(expected));

    // Insert data using old schema and ensure new column gets the default value
    old_db
        .simple_query("INSERT INTO users (id) VALUES (3)")
        .unwrap();
    let name: String = new_db
        .query_one("SELECT name from users WHERE id = 3", &[])
        .map(|row| row.get("name"))
        .unwrap();
    assert_eq!("DEFAULT", name);

    reshape.complete_migration().unwrap();
}
