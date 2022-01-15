use reshape::migrations::{AddColumn, Column, ColumnBuilder, CreateTableBuilder, Migration};

mod common;

#[test]
fn add_column() {
    let mut test = common::Test::new("Add column");

    test.first_migration(
        r#"
        name = "create_user_table"

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
        "#,
    );

    test.second_migration(
        r#"
        name = "add_first_and_last_name_columns"

        [[actions]]
        type = "add_column"
        table = "users"

        up = "(STRING_TO_ARRAY(name, ' '))[1]"

            [actions.column]
            name = "first"
            type = "TEXT"
            nullable = false

        [[actions]]
        type = "add_column"
        table = "users"

        up = "(STRING_TO_ARRAY(name, ' '))[2]"

            [actions.column]
            name = "last"
            type = "TEXT"
            nullable = false
        "#,
    );

    test.after_first(|db| {
        // Insert some test users
        db.simple_query(
            "
            INSERT INTO users (id, name) VALUES
                (1, 'John Doe'),
                (2, 'Jane Doe');
            ",
        )
        .unwrap();
    });

    test.intermediate(|old_db, new_db| {
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
    });

    test.after_completion(|db| {
        let expected = vec![("John", "Doe"), ("Jane", "Doe"), ("Test", "Testsson")];
        assert!(db
            .query("SELECT first, last FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| (row.get("first"), row.get("last")))
            .eq(expected));
    });

    test.after_abort(|db| {
        let expected = vec![("John Doe"), ("Jane Doe"), ("Test Testsson")];
        assert!(db
            .query("SELECT name FROM users ORDER BY id", &[],)
            .unwrap()
            .iter()
            .map(|row| row.get::<'_, _, String>("name"))
            .eq(expected));
    });

    test.run()
}

#[test]
fn add_column_nullable() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_user_table", None).with_action(
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
    let add_name_column = Migration::new("add_nullable_name_column", None).with_action(AddColumn {
        table: "users".to_string(),
        column: Column {
            name: "name".to_string(),
            data_type: "TEXT".to_string(),
            nullable: true,
            default: None,
            generated: None,
        },
        up: None,
    });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), add_name_column.clone()];

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
        .simple_query(&reshape::schema_query_for_migration(
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
        .simple_query("INSERT INTO users (id, name) VALUES (4, 'Test Testsson'), (5, NULL)")
        .unwrap();

    reshape.complete().unwrap();
    common::assert_cleaned_up(&mut new_db);
}

#[test]
fn add_column_with_default() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_users_table = Migration::new("create_user_table", None).with_action(
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
    let add_name_column =
        Migration::new("add_name_column_with_default", None).with_action(AddColumn {
            table: "users".to_string(),
            column: Column {
                name: "name".to_string(),
                data_type: "TEXT".to_string(),
                nullable: false,
                default: Some("'DEFAULT'".to_string()),
                generated: None,
            },
            up: None,
        });

    let first_migrations = vec![create_users_table.clone()];
    let second_migrations = vec![create_users_table.clone(), add_name_column.clone()];

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

    // Insert some test values
    new_db
        .simple_query("INSERT INTO users (id) VALUES (1), (2)")
        .unwrap();

    // Run second migration
    reshape.migrate(second_migrations.clone()).unwrap();
    new_db
        .simple_query(&reshape::schema_query_for_migration(
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

    reshape.complete().unwrap();
    common::assert_cleaned_up(&mut new_db);
}
