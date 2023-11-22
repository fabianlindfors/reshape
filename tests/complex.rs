mod common;
use common::Test;

#[test]
fn extract_relation_into_new_table() {
    let mut test = Test::new("Extract relation into new table");

    test.first_migration(
        r#"
        name = "create_tables"

        [[actions]]
        type = "create_table"
        name = "accounts"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "account_id"
            type = "INTEGER"

            [[actions.columns]]
            name = "account_role"
            type = "TEXT"
        "#,
    );

    test.second_migration(
        r#"
        name = "add_account_user_connection"

        [[actions]]
        type = "create_table"
        name = "user_account_connections"
        primary_key = ["account_id", "user_id"]

            [[actions.columns]]
            name = "account_id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"

            [[actions.columns]]
            name = "role"
            type = "TEXT"

            [actions.up]
            table = "users"
            values = { user_id = "id", account_id = "account_id", role = "UPPER(account_role)" }
            where = "user_id = id"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "account_id"

            [actions.down]
            table = "user_account_connections"
            value = "account_id"
            where = "id = user_id"

        [[actions]]
        type = "remove_column"
        table = "users"
        column = "account_role"

            [actions.down]
            table = "user_account_connections"
            value = "LOWER(role)"
            where = "id = user_id"
        "#,
    );

    test.after_first(|db| {
        db.simple_query("INSERT INTO accounts (id) VALUES (1)")
            .unwrap();
        db.simple_query("INSERT INTO users (id, account_id, account_role) VALUES (1, 1, 'admin')")
            .unwrap();
    });

    test.intermediate(|old_db, new_db| {
        // Ensure connections was backfilled
        let rows: Vec<(i32, i32, String)> = new_db
            .query(
                "
                SELECT account_id, user_id, role
                FROM user_account_connections
                ",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| (row.get("account_id"), row.get("user_id"), row.get("role")))
            .collect();
        assert_eq!(1, rows.len());

        let row = rows.first().unwrap();
        assert_eq!(1, row.0);
        assert_eq!(1, row.1);
        assert_eq!("ADMIN", row.2);

        // Ensure inserted user in old schema creates a new connection
        old_db
            .simple_query(
                "INSERT INTO users (id, account_id, account_role) VALUES (2, 1, 'developer')",
            )
            .unwrap();
        assert!(
            new_db
                .query(
                    "
                    SELECT account_id, user_id, role
                    FROM user_account_connections
                    WHERE account_id = 1 AND user_id = 2 AND role = 'DEVELOPER'
                    ",
                    &[],
                )
                .unwrap()
                .len()
                == 1
        );

        // Ensure updated user role in old schema updates connection in new schema
        old_db
            .simple_query("UPDATE users SET account_role = 'admin' WHERE id = 2")
            .unwrap();
        assert!(
            new_db
                .query(
                    "
                    SELECT account_id, user_id, role
                    FROM user_account_connections
                    WHERE account_id = 1 AND user_id = 2 AND role = 'ADMIN'
                    ",
                    &[],
                )
                .unwrap()
                .len()
                == 1
        );

        // Ensure updated connection in new schema updates old schema user
        new_db
            .simple_query(
                "UPDATE user_account_connections SET role = 'DEVELOPER' WHERE account_id = 1 AND user_id = 2",
            )
            .unwrap();
        assert!(
            old_db
                .query(
                    "
                    SELECT id
                    FROM users
                    WHERE id = 2 AND account_id = 1 AND account_role = 'developer'
                    ",
                    &[],
                )
                .unwrap()
                .len()
                == 1
        );

        // Ensure insert of user with connection through new schema updates user in old schema
        new_db
            .simple_query(
                r#"
                BEGIN;
                INSERT INTO users (id) VALUES (3);
                INSERT INTO user_account_connections (user_id, account_id, role) VALUES (3, 1, 'DEVELOPER');
                COMMIT;
                "#,
            )
            .unwrap();
        new_db
            .simple_query(
                "",
            )
            .unwrap();
        assert!(
            old_db
                .query(
                    "
                    SELECT id
                    FROM users
                    WHERE id = 3 AND account_id = 1 AND account_role = 'developer'
                    ",
                    &[],
                )
                .unwrap()
                .len()
                == 1
        );
    });

    test.run();
}
