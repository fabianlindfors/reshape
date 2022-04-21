mod common;
use common::Test;

#[test]
fn custom_enable_extension() {
    let mut test = Test::new("Custom migration");

    test.clear(|db| {
        db.simple_query(
            "
			DROP EXTENSION IF EXISTS bloom;
			DROP EXTENSION IF EXISTS btree_gin;
			DROP EXTENSION IF EXISTS btree_gist;
            ",
        )
        .unwrap();
    });

    test.first_migration(
        r#"
		name = "empty_migration"

		[[actions]]
		type = "custom"
		"#,
    );

    test.second_migration(
        r#"
		name = "enable_extensions"

		[[actions]]
		type = "custom"

		start = """
			CREATE EXTENSION IF NOT EXISTS bloom;
			CREATE EXTENSION IF NOT EXISTS btree_gin;
		"""

        complete = "CREATE EXTENSION IF NOT EXISTS btree_gist"

		abort = """
			DROP EXTENSION IF EXISTS bloom;
			DROP EXTENSION IF EXISTS btree_gin;
		"""
		"#,
    );

    test.intermediate(|db, _| {
        let bloom_activated = !db
            .query("SELECT * FROM pg_extension WHERE extname = 'bloom'", &[])
            .unwrap()
            .is_empty();
        assert!(bloom_activated);

        let btree_gin_activated = !db
            .query(
                "SELECT * FROM pg_extension WHERE extname = 'btree_gin'",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(btree_gin_activated);

        let btree_gist_activated = !db
            .query(
                "SELECT * FROM pg_extension WHERE extname = 'btree_gist'",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(!btree_gist_activated);
    });

    test.after_completion(|db| {
        let bloom_activated = !db
            .query("SELECT * FROM pg_extension WHERE extname = 'bloom'", &[])
            .unwrap()
            .is_empty();
        assert!(bloom_activated);

        let btree_gin_activated = !db
            .query(
                "SELECT * FROM pg_extension WHERE extname = 'btree_gin'",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(btree_gin_activated);

        let btree_gist_activated = !db
            .query(
                "SELECT * FROM pg_extension WHERE extname = 'btree_gist'",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(btree_gist_activated);
    });

    test.after_abort(|db| {
        let bloom_activated = !db
            .query("SELECT * FROM pg_extension WHERE extname = 'bloom'", &[])
            .unwrap()
            .is_empty();
        assert!(!bloom_activated);

        let btree_gin_activated = !db
            .query(
                "SELECT * FROM pg_extension WHERE extname = 'btree_gin'",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(!btree_gin_activated);

        let btree_gist_activated = !db
            .query(
                "SELECT * FROM pg_extension WHERE extname = 'btree_gist'",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(!btree_gist_activated);
    });

    test.run();
}
