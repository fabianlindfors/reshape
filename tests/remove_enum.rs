mod common;
use common::Test;

#[test]
fn remove_enum() {
    let mut test = Test::new("Remove enum");

    test.first_migration(
        r#"
		name = "create_enum"

		[[actions]]
		type = "create_enum"
		name = "mood"
		values = ["happy", "ok", "sad"]
		"#,
    );

    test.second_migration(
        r#"
		name = "remove_enum"

		[[actions]]
		type = "remove_enum"
		enum = "mood"
		"#,
    );

    test.after_first(|db| {
        // Ensure enum was created
        let enum_exists = !db
            .query(
                "
				SELECT typname
				FROM pg_catalog.pg_type
				WHERE typcategory = 'E'
				AND typname = 'mood'
				",
                &[],
            )
            .unwrap()
            .is_empty();

        assert!(enum_exists, "expected mood enum to have been created");
    });

    test.after_completion(|db| {
        // Ensure enum was removed after completion
        let enum_does_not_exist = db
            .query(
                "
				SELECT typname
				FROM pg_catalog.pg_type
				WHERE typcategory = 'E'
				AND typname = 'mood'
				",
                &[],
            )
            .unwrap()
            .is_empty();

        assert!(
            enum_does_not_exist,
            "expected mood enum to have been removed"
        );
    });

    test.run();
}
