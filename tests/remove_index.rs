use reshape::migrations::{AddIndex, ColumnBuilder, CreateTableBuilder, Migration, RemoveIndex};

mod common;

#[test]
fn remove_index() {
    let (mut reshape, _, mut db) = common::setup();

    let create_table_migration = Migration::new("create_user_table", None)
        .with_action(
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
        )
        .with_action(AddIndex {
            table: "users".to_string(),
            name: "name_idx".to_string(),
            columns: vec!["name".to_string()],
        });

    let remove_index_migration =
        Migration::new("remove_name_index", None).with_action(RemoveIndex {
            index: "name_idx".to_string(),
        });

    let first_migrations = vec![create_table_migration.clone()];
    let second_migrations = vec![
        create_table_migration.clone(),
        remove_index_migration.clone(),
    ];

    // Run migrations
    reshape.migrate(first_migrations.clone()).unwrap();

    // Ensure index is valid and ready
    let result: Vec<(bool, bool)> = db
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

    // Run migration to remove index
    reshape.migrate(second_migrations.clone()).unwrap();

    // Ensure index is still valid and ready during the migration
    let result: Vec<(bool, bool)> = db
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

    reshape.complete().unwrap();

    // Ensure index has been removed after the migration is complete
    let count: i64 = db
        .query(
            "
			SELECT COUNT(*)
			FROM pg_catalog.pg_index
			JOIN pg_catalog.pg_class ON pg_index.indexrelid = pg_class.oid
			WHERE pg_class.relname = 'name_idx'
			",
            &[],
        )
        .unwrap()
        .first()
        .map(|row| row.get(0))
        .unwrap();

    assert_eq!(0, count, "expected index to not exist");

    common::assert_cleaned_up(&mut db);
}
