use reshape::migrations::{AddIndex, ColumnBuilder, CreateTableBuilder, Index, Migration};

mod common;

#[test]
fn add_index() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_table_migration = Migration::new("create_user_table", None).with_action(
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
    let add_index_migration = Migration::new("add_name_index", None).with_action(AddIndex {
        table: "users".to_string(),
        index: Index {
            name: "name_idx".to_string(),
            columns: vec!["name".to_string()],
            unique: true,
        },
    });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![create_table_migration.clone(), add_index_migration.clone()];

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

    // Ensure index is valid and ready
    let (is_ready, is_valid): (bool, bool) = new_db
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
        .first()
        .map(|row| (row.get("indisready"), row.get("indisvalid")))
        .unwrap();

    assert!(is_ready, "expected index to be ready");
    assert!(is_valid, "expected index to be valid");

    reshape.complete().unwrap();
    common::assert_cleaned_up(&mut new_db);
}

#[test]
fn add_index_unique() {
    let (mut reshape, mut old_db, mut new_db) = common::setup();

    let create_table_migration = Migration::new("create_user_table", None).with_action(
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
    let add_index_migration = Migration::new("add_name_index", None).with_action(AddIndex {
        table: "users".to_string(),
        index: Index {
            name: "name_idx".to_string(),
            columns: vec!["name".to_string()],
            unique: true,
        },
    });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![create_table_migration.clone(), add_index_migration.clone()];

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

    // Ensure index is valid, ready and unique
    let (is_ready, is_valid, is_unique): (bool, bool, bool) = new_db
        .query(
            "
			SELECT pg_index.indisready, pg_index.indisvalid, pg_index.indisunique
			FROM pg_catalog.pg_index
			JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
			WHERE pg_class.relname = 'name_idx'
			",
            &[],
        )
        .unwrap()
        .first()
        .map(|row| {
            (
                row.get("indisready"),
                row.get("indisvalid"),
                row.get("indisunique"),
            )
        })
        .unwrap();

    assert!(is_ready, "expected index to be ready");
    assert!(is_valid, "expected index to be valid");
    assert!(is_unique, "expected index to be unique");

    reshape.complete().unwrap();
    common::assert_cleaned_up(&mut new_db);
}
