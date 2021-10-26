use reshape::migrations::{AddIndex, Column, CreateTable, Migration};

mod common;

#[test]
fn add_index() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_table_migration =
        Migration::new("create_users_table", None).with_action(CreateTable {
            name: "users".to_string(),
            primary_key: None,
            foreign_keys: vec![],
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
                    nullable: false,
                    default: None,
                },
            ],
        });
    let add_index_migration = Migration::new("add_name_index", None).with_action(AddIndex {
        table: "users".to_string(),
        name: "name_idx".to_string(),
        columns: vec!["name".to_string()],
    });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![create_table_migration.clone(), add_index_migration.clone()];

    // Run migrations
    reshape.migrate(first_migrations.clone()).unwrap();
    reshape.migrate(second_migrations.clone()).unwrap();

    // Update schemas of Postgres connections
    let old_schema_query = reshape::generate_schema_query(&first_migrations.last().unwrap().name);
    let new_schema_query = reshape::generate_schema_query(&second_migrations.last().unwrap().name);
    println!("{}", new_schema_query);
    old_db.simple_query(&old_schema_query).unwrap();
    new_db.simple_query(&new_schema_query).unwrap();

    // Ensure index is valid and ready
    let result: Vec<(bool, bool)> = new_db
        .query(
            "
			SELECT pg_index.indisready, pg_index.indisvalid
			FROM pg_catalog.pg_index
			JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
			WHERE pg_class.relname = 'name_idx'
			",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| (row.get("indisready"), row.get("indisvalid")))
        .collect();

    assert_eq!(vec![(true, true)], result);

    reshape.complete_migration().unwrap();
}
