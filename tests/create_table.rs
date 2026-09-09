mod common;
use common::{assert_invalid, get_column_comment, get_comment, get_constraint_comment, Test};
use reshape::migrations::Migration;

#[test]
fn create_table_invalid_default_sql() {
    assert_invalid(
        r#"
        name = "test"
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
        default = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn create_table_invalid_check_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        [[actions.checks]]
        name = "id_positive"
        expression = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn create_table_default_with_column_reference() {
    assert_invalid(
        r#"
        name = "test"
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
        default = "lower(id)"
        "#,
    );
}

#[test]
fn create_table_check_invalid_column_reference() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        [[actions.checks]]
        expression = "non_existent > 0"
        "#,
    );
}

#[test]
fn create_table_up_values_invalid_column_reference() {
    let mut test = Test::new("Create table with invalid up reference");

    test.first_migration(
        r#"
        name = "create_tables"
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
        name = "create_profiles_with_bad_reference"

        [[actions]]
        type = "create_table"
        name = "profiles"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "name"
            type = "TEXT"

            [actions.up]
            table = "users"
            values = { id = "id", name = "non_existent" }
        "#,
    );

    test.expect_failure();
    test.run();
}

#[test]
fn create_table_then_change_it_in_same_migration() {
    let mut test = Test::new("Create table and change it in the same migration");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"
        "#,
    );

    // Every action after the first references columns of the table created by it, some
    // of which are added or altered by the actions in between. All of them resolve
    // through the schema as it looks right before each action.
    test.second_migration(
        r#"
        name = "create_and_change_profiles"

        [[actions]]
        type = "create_table"
        name = "profiles"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
            nullable = false

            [[actions.checks]]
            expression = "length(email) > 0"

        [[actions]]
        type = "add_column"
        table = "profiles"
        up = "lower(email)"

            [actions.column]
            name = "normalized_email"
            type = "TEXT"

        [[actions]]
        type = "alter_column"
        table = "profiles"
        column = "email"
        up = "email"
        down = "email"

            [actions.changes]
            type = "VARCHAR(255)"

        [[actions]]
        type = "add_index"
        table = "profiles"

            [actions.index]
            name = "profiles_email_idx"
            columns = [{ expression = "lower(normalized_email)" }]
            where = "email IS NOT NULL"
        "#,
    );

    test.intermediate(|_old_db, new_db| {
        // The old schema has no `profiles` table, so all writes come from the new schema
        new_db
            .simple_query(
                "INSERT INTO profiles (id, email, normalized_email) VALUES (1, 'John@Example.com', 'john@example.com')",
            )
            .unwrap();
        let normalized: Option<String> = new_db
            .query_one("SELECT normalized_email FROM profiles WHERE id = 1", &[])
            .unwrap()
            .get("normalized_email");
        assert_eq!(Some("john@example.com".to_string()), normalized);

        // The check constraint from the create_table action is in place
        assert!(new_db
            .simple_query("INSERT INTO profiles (id, email) VALUES (2, '')")
            .is_err());
    });

    test.after_completion(|db| {
        let definition: String = db
            .query_one(
                "SELECT indexdef FROM pg_indexes WHERE indexname = 'profiles_email_idx'",
                &[],
            )
            .unwrap()
            .get("indexdef");
        assert!(
            definition.contains("lower(normalized_email)") && !definition.contains("__reshape"),
            "got: {}",
            definition
        );
    });

    test.run();
}

#[test]
fn create_table_primary_key_unknown_column() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["user_id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        "#,
    );
}

#[test]
fn create_table_foreign_key_unknown_column() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        [[actions.foreign_keys]]
        columns = ["user_id"]
        referenced_table = "users"
        referenced_columns = ["id"]
        "#,
    );
}

#[test]
fn create_table_up_values_unknown_column() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "profiles"
        primary_key = ["id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        [actions.up]
        table = "users"
        values = { id = "id", non_existent = "name" }
        "#,
    );
}

#[test]
fn create_table_invalid_up_values_sql() {
    assert_invalid(
        r#"
        name = "test"
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]
        [[actions.columns]]
        name = "id"
        type = "INTEGER"
        [actions.up]
        table = "other"
        [actions.up.values]
        id = "INVALID $$$ SYNTAX"
        "#,
    );
}

#[test]
fn create_table() {
    let mut test = Test::new("Create table");

    test.first_migration(
        r#"
        name = "create_users_table"
        
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"
            generated = "ALWAYS AS IDENTITY"

            [[actions.columns]]
            name = "name"
            type = "TEXT"

            [[actions.columns]]
            name = "created_at"
            type = "TIMESTAMP"
            nullable = false
            default = "NOW()"
        "#,
    );

    test.after_first(|db| {
        // Ensure table was created
        let result = db
            .query_opt(
                "
                SELECT table_name
                FROM information_schema.tables
                WHERE table_name = 'users' AND table_schema = 'public'",
                &[],
            )
            .unwrap();
        assert!(result.is_some());

        // Ensure table has the right columns
        let result = db
            .query(
                "
                SELECT column_name, column_default, is_nullable, data_type
                FROM information_schema.columns
                WHERE table_name = 'users' AND table_schema = 'public'
                ORDER BY ordinal_position",
                &[],
            )
            .unwrap();

        // id column
        let id_row = &result[0];
        assert_eq!("id", id_row.get::<_, String>("column_name"));
        assert!(id_row.get::<_, Option<String>>("column_default").is_none());
        assert_eq!("NO", id_row.get::<_, String>("is_nullable"));
        assert_eq!("integer", id_row.get::<_, String>("data_type"));

        // name column
        let name_row = &result[1];
        assert_eq!("name", name_row.get::<_, String>("column_name"));
        assert!(name_row
            .get::<_, Option<String>>("column_default")
            .is_none());
        assert_eq!("YES", name_row.get::<_, String>("is_nullable"));
        assert_eq!("text", name_row.get::<_, String>("data_type"));

        // created_at column
        let created_at_column = &result[2];
        assert_eq!(
            "created_at",
            created_at_column.get::<_, String>("column_name")
        );
        assert!(created_at_column
            .get::<_, Option<String>>("column_default")
            .is_some());
        assert_eq!("NO", created_at_column.get::<_, String>("is_nullable"));
        assert_eq!(
            "timestamp without time zone",
            created_at_column.get::<_, String>("data_type")
        );

        // Ensure the primary key has the right columns
        let primary_key_columns: Vec<String> = db
            .query(
                "
                SELECT a.attname AS column
                FROM pg_index i
                JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
                JOIN pg_class t ON t.oid = i.indrelid
                WHERE t.relname = 'users' AND i.indisprimary
                ",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.get("column"))
            .collect();

        assert_eq!(vec!["id"], primary_key_columns);
    });

    test.run();
}

#[test]
fn create_table_with_foreign_keys() {
    let mut test = Test::new("Create table");

    test.first_migration(
        r#"
        name = "create_users_table"
        
        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"
            generated = "ALWAYS AS IDENTITY"

            [[actions.columns]]
            name = "name"
            type = "TEXT"

            [[actions.columns]]
            name = "created_at"
            type = "TIMESTAMP"
            nullable = false
            default = "NOW()"

        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"
            nullable = false

            [[actions.foreign_keys]]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
        "#,
    );

    test.after_first(|db| {
        let foreign_key_columns: Vec<(String, String, String)> = db
            .query(
                "
                SELECT
                    kcu.column_name, 
                    ccu.table_name AS foreign_table_name,
                    ccu.column_name AS foreign_column_name 
                FROM 
                    information_schema.table_constraints AS tc 
                    JOIN information_schema.key_column_usage AS kcu
                    ON tc.constraint_name = kcu.constraint_name
                    AND tc.table_schema = kcu.table_schema
                    JOIN information_schema.constraint_column_usage AS ccu
                    ON ccu.constraint_name = tc.constraint_name
                    AND ccu.table_schema = tc.table_schema
                WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name='items';
                ",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row.get("column_name"),
                    row.get("foreign_table_name"),
                    row.get("foreign_column_name"),
                )
            })
            .collect();

        assert_eq!(
            vec![("user_id".to_string(), "users".to_string(), "id".to_string())],
            foreign_key_columns
        );
    });

    test.run();
}

#[test]
fn create_table_with_referential_actions() {
    let mut test = Test::new("Create table with referential actions");

    test.first_migration(
        r#"
        name = "create_users_and_items_tables"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "user_id"
            type = "INTEGER"

            [[actions.foreign_keys]]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
            on_delete = "CASCADE"
            on_update = "SET NULL"
        "#,
    );

    test.after_first(|db| {
        // Ensure the referential actions were applied to the constraint
        let (on_delete, on_update): (i8, i8) = db
            .query(
                "
                SELECT confdeltype, confupdtype
                FROM pg_constraint
                WHERE contype = 'f' AND conrelid = 'public.items'::regclass
                ",
                &[],
            )
            .unwrap()
            .first()
            .map(|row| (row.get("confdeltype"), row.get("confupdtype")))
            .unwrap();
        assert_eq!(b'c' as i8, on_delete, "expected ON DELETE CASCADE");
        assert_eq!(b'n' as i8, on_update, "expected ON UPDATE SET NULL");

        db.simple_query("INSERT INTO users (id) VALUES (1), (2)")
            .unwrap();
        db.simple_query("INSERT INTO items (id, user_id) VALUES (1, 1), (2, 2)")
            .unwrap();

        // Ensure updates set the referencing column to NULL
        db.simple_query("UPDATE users SET id = 20 WHERE id = 1")
            .unwrap();
        let user_id: Option<i32> = db
            .query("SELECT user_id FROM items WHERE id = 1", &[])
            .unwrap()
            .first()
            .map(|row| row.get("user_id"))
            .unwrap();
        assert_eq!(None, user_id, "expected user_id to be set to NULL");

        // Ensure deletes cascade
        db.simple_query("DELETE FROM users WHERE id = 2").unwrap();
        let remaining = db
            .query("SELECT id FROM items WHERE id = 2", &[])
            .unwrap()
            .len();
        assert_eq!(0, remaining, "expected item to be deleted by cascade");
    });

    test.run()
}

#[test]
fn create_table_invalid_referential_action() {
    let result = toml::from_str::<Migration>(
        r#"
        name = "create_items_table"

        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.foreign_keys]]
            columns = ["user_id"]
            referenced_table = "users"
            referenced_columns = ["id"]
            on_delete = "MAYBE"
        "#,
    );

    assert!(
        result.is_err(),
        "expected an unknown referential action to be rejected"
    );
}

#[test]
fn create_table_with_checks() {
    let mut test = Test::new("Create table with check constraints");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"

            [[actions.columns]]
            name = "kind"
            type = "TEXT"

            [[actions.columns]]
            name = "age"
            type = "INTEGER"

            [[actions.checks]]
            name = "users_email_check"
            expression = "email ~ '^[^@]+@[^@]+$'"

            [[actions.checks]]
            name = "users_kind_check"
            expression = "kind IN ('admin', 'user')"

            # Unnamed checks get a name generated by Postgres
            [[actions.checks]]
            expression = "age >= 0 AND age < 150"
        "#,
    );

    test.after_first(|db| {
        // Ensure all three check constraints were created
        let checks: Vec<String> = db
            .query(
                "
                SELECT conname
                FROM pg_constraint
                WHERE contype = 'c' AND conrelid = 'public.users'::regclass
                ORDER BY conname
                ",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| row.get("conname"))
            .collect();
        assert_eq!(
            vec!["users_age_check", "users_email_check", "users_kind_check"],
            checks
        );

        // Ensure a row satisfying every check can be inserted
        db.simple_query(
            "INSERT INTO users (id, email, kind, age) VALUES (1, 'someone@example.com', 'admin', 30)",
        )
        .unwrap();

        // Ensure each check is enforced
        let result = db.simple_query(
            "INSERT INTO users (id, email, kind, age) VALUES (2, 'not-an-email', 'admin', 30)",
        );
        assert!(result.is_err(), "expected email check to reject insert");

        let result = db.simple_query(
            "INSERT INTO users (id, email, kind, age) VALUES (3, 'someone@example.com', 'wizard', 30)",
        );
        assert!(result.is_err(), "expected kind check to reject insert");

        let result = db.simple_query(
            "INSERT INTO users (id, email, kind, age) VALUES (4, 'someone@example.com', 'admin', -1)",
        );
        assert!(result.is_err(), "expected age check to reject insert");
    });

    test.run();
}

#[test]
fn create_table_with_checks_during_migration() {
    let mut test = Test::new("Create table with check constraints during migration");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"
        "#,
    );

    test.second_migration(
        r#"
        name = "create_documents_table"

        [[actions]]
        type = "create_table"
        name = "documents"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "data"
            type = "BYTEA"

            [[actions.columns]]
            name = "size_bytes"
            type = "INTEGER"

            # A check can span multiple columns
            [[actions.checks]]
            name = "documents_size_bytes_check"
            expression = "size_bytes = octet_length(data)"
        "#,
    );

    test.intermediate(|_, new_db| {
        // Ensure the check is enforced as soon as the migration has started
        new_db
            .simple_query("INSERT INTO documents (id, data, size_bytes) VALUES (1, 'abc', 3)")
            .unwrap();

        let result = new_db
            .simple_query("INSERT INTO documents (id, data, size_bytes) VALUES (2, 'abc', 100)");
        assert!(result.is_err(), "expected check to reject insert");
    });

    test.after_completion(|db| {
        // Ensure the check survives completion
        let check_exists = !db
            .query(
                "
                SELECT conname
                FROM pg_constraint
                WHERE contype = 'c'
                    AND conrelid = 'public.documents'::regclass
                    AND conname = 'documents_size_bytes_check'
                ",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(check_exists, "expected check constraint to still exist");

        let result =
            db.simple_query("INSERT INTO documents (id, data, size_bytes) VALUES (3, 'abc', 100)");
        assert!(result.is_err(), "expected check to reject insert");
    });

    test.after_abort(|db| {
        // Ensure the whole table, and with it the check, was removed
        let table_exists = !db
            .query(
                "
                SELECT table_name
                FROM information_schema.tables
                WHERE table_name = 'documents' AND table_schema = 'public'
                ",
                &[],
            )
            .unwrap()
            .is_empty();
        assert!(!table_exists, "expected table to have been removed");
    });

    test.run();
}

#[test]
fn create_table_with_comments() {
    let mut test = Test::new("Create table with comments");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]
        comment = "People who can sign in"

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.columns]]
            name = "email"
            type = "TEXT"
            comment = "Primary contact address. Don't share it"

            [[actions.checks]]
            name = "users_email_not_empty"
            expression = "email <> ''"
            comment = "Addresses can't be blank"
        "#,
    );

    test.after_first(|db| {
        // Ensure the comments were set on the underlying table
        assert_eq!(
            Some("People who can sign in".to_string()),
            get_comment(db, "public.users")
        );
        assert_eq!(
            Some("Primary contact address. Don't share it".to_string()),
            get_column_comment(db, "public.users", "email")
        );

        // Ensure the comments are also visible through the view the application uses.
        // The connection's search path points at the migration schema, so the
        // unqualified name resolves to the view.
        assert_eq!(
            Some("People who can sign in".to_string()),
            get_comment(db, "users")
        );
        assert_eq!(
            Some("Primary contact address. Don't share it".to_string()),
            get_column_comment(db, "users", "email")
        );

        // Ensure a column without a comment doesn't get one
        assert_eq!(None, get_column_comment(db, "users", "id"));

        // Ensure the comment was set on the check constraint
        assert_eq!(
            Some("Addresses can't be blank".to_string()),
            get_constraint_comment(db, "users", "users_email_not_empty")
        );
    });

    test.run();
}

#[test]
fn create_table_with_comment_on_unnamed_check() {
    let mut test = Test::new("Create table with comment on unnamed check");

    test.first_migration(
        r#"
        name = "create_users_table"

        [[actions]]
        type = "create_table"
        name = "users"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"
        "#,
    );

    // A comment can only be set on a check which the migration names, as Postgres
    // generates a name for an unnamed one
    test.second_migration(
        r#"
        name = "create_items_table"

        [[actions]]
        type = "create_table"
        name = "items"
        primary_key = ["id"]

            [[actions.columns]]
            name = "id"
            type = "INTEGER"

            [[actions.checks]]
            expression = "id > 0"
            comment = "Ids are positive"
        "#,
    );

    test.expect_failure();
    test.run();
}
