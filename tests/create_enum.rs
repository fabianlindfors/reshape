mod common;
use common::Test;

#[test]
fn create_enum() {
    let mut test = Test::new("Create enum");

    test.first_migration(
        r#"
		name = "create_enum_and_table"

		[[actions]]
		type = "create_enum"
		name = "mood"
		values = ["happy", "ok", "sad"]

		[[actions]]
		type = "create_table"
		name = "updates"
		primary_key = ["id"]

			[[actions.columns]]
			name = "id"
			type = "INTEGER"

			[[actions.columns]]
			name = "status"
			type = "mood"
		"#,
    );

    test.after_first(|db| {
        // Valid enum values should succeed
        db.simple_query(
            "INSERT INTO updates (id, status) VALUES (1, 'happy'), (2, 'ok'), (3, 'sad')",
        )
        .unwrap();
    });

    test.run();
}
