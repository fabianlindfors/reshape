mod common;
use common::Test;
use postgres::Client;
use postgres_native_tls::MakeTlsConnector;
use reshape::{migrations::Migration, Reshape};

#[test]
fn invalid_migration() {
    let mut test = Test::new("Invalid migration");

    test.first_migration(
        r#"
        name = "invalid_migration"

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
        name = "add_invalid_column"

        [[actions]]
        type = "add_column"
        table = "users"

        up = "INVALID SQL"

            [actions.column]
            name = "first"
            type = "TEXT"
        "#,
    );

    // Insert a test user
    test.after_first(|db| {
        db.simple_query(
            "
            INSERT INTO users (id) VALUES (1)
            ",
        )
        .unwrap();
    });

    test.expect_failure();
    test.run();
}

#[test]
fn completion_resumes_at_failed_action() {
    let connection_string = std::env::var("POSTGRES_CONNECTION_STRING")
        .unwrap_or("postgres://postgres:postgres@localhost/reshape_test".to_string());
    let tls_connector = native_tls::TlsConnector::new().unwrap();
    let mut db = Client::connect(
        &connection_string,
        MakeTlsConnector::new(tls_connector),
    )
    .unwrap();
    let mut reshape = Reshape::new(&connection_string).unwrap();

    reshape.remove().unwrap();
    db.batch_execute(
        "
        DROP TABLE IF EXISTS completion_before_retry;
        DROP TABLE IF EXISTS completion_retry_gate;
        DROP TABLE IF EXISTS completion_after_retry;
        ",
    )
    .unwrap();

    let migration: Migration = toml::from_str(
        r#"
        name = "completion_retry"

        [[actions]]
        type = "custom"
        complete = "CREATE TABLE completion_before_retry (id INTEGER);"

        [[actions]]
        type = "custom"
        complete = "INSERT INTO completion_retry_gate (id) VALUES (1);"

        [[actions]]
        type = "custom"
        complete = "CREATE TABLE completion_after_retry (id INTEGER);"
        "#,
    )
    .unwrap();

    reshape.migrate(vec![migration]).unwrap();
    assert!(reshape.complete().is_err());

    db.batch_execute("CREATE TABLE completion_retry_gate (id INTEGER);")
        .unwrap();
    reshape.complete().unwrap();

    let after_retry_exists: bool = db
        .query_one(
            "SELECT to_regclass('public.completion_after_retry') IS NOT NULL",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(after_retry_exists, "expected completion to resume after failure");

    db.batch_execute(
        "
        DROP TABLE completion_before_retry;
        DROP TABLE completion_retry_gate;
        DROP TABLE completion_after_retry;
        ",
    )
    .unwrap();
    reshape.remove().unwrap();
}
