use postgres::{error::SqlState, Client};
use reshape::{migrations::Migration, Reshape};
use std::{
    thread,
    time::{Duration, Instant},
};

fn connection_string() -> String {
    std::env::var("POSTGRES_CONNECTION_STRING")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/reshape_test".into())
}

fn connect() -> Client {
    let connector =
        postgres_native_tls::MakeTlsConnector::new(native_tls::TlsConnector::new().unwrap());
    Client::connect(&connection_string(), connector).unwrap()
}

fn initial() -> Migration {
    toml::from_str(
        r#"
name = "initial"
[[actions]]
type = "create_table"
name = "users"
primary_key = ["id"]
[[actions.columns]]
name = "id"
type = "INTEGER"
[[actions.columns]]
name = "value"
type = "INTEGER"
"#,
    )
    .unwrap()
}

fn index(unique: bool) -> Migration {
    toml::from_str(&format!(
        r#"
name = "indexed"
[[actions]]
type = "add_index"
table = "users"
[actions.index]
name = "users_value_idx"
columns = ["value"]
unique = {unique}
"#
    ))
    .unwrap()
}

fn setup() -> (Reshape, Client) {
    let mut reshape = Reshape::new(&connection_string()).unwrap();
    reshape.remove().unwrap();
    reshape.migrate(vec![initial()]).unwrap();
    reshape.complete().unwrap();
    let mut db = connect();
    db.batch_execute("INSERT INTO users VALUES (1, 1), (2, 1)")
        .unwrap();
    (reshape, db)
}

fn start_index() -> thread::JoinHandle<anyhow::Result<()>> {
    thread::spawn(|| {
        Reshape::new(&connection_string())
            .unwrap()
            .migrate(vec![initial(), index(false)])
    })
}

fn wait_for_query(db: &mut Client, prefix: &str) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rows = db
            .query(
                "SELECT pid FROM pg_stat_activity WHERE datname = current_database()
             AND pid <> pg_backend_pid() AND state = 'active' AND wait_event_type = 'Lock'
             AND query LIKE $1",
                &[&format!("{prefix}%")],
            )
            .unwrap();
        if let Some(row) = rows.first() {
            return row.get(0);
        }
        assert!(
            Instant::now() < deadline,
            "did not observe blocked query: {prefix}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn assert_clean(db: &mut Client) {
    let count: i64 = db
        .query_one(
            "SELECT count(*) FROM reshape.data WHERE key LIKE '%_add_index'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(count, 0);
    let count: i64 = db
        .query_one(
            "SELECT count(*) FROM pg_class WHERE relname LIKE '__reshape_idx_%'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(count, 0);
}

fn oid(db: &mut Client, name: &str) -> Option<u32> {
    db.query_one("SELECT to_regclass($1)::oid", &[&name])
        .unwrap()
        .get(0)
}

#[test]
fn retries_after_blocker_clears_and_removes_invalid_index() {
    let (mut reshape, mut db) = setup();
    let mut blocker = connect();
    blocker
        .batch_execute("BEGIN; UPDATE users SET value = value WHERE id = 1")
        .unwrap();
    let worker = start_index();
    wait_for_query(&mut db, "CREATE  INDEX CONCURRENTLY");
    // Wait until the first timeout has left an invalid index and cleanup has begun.
    wait_for_query(&mut db, "DROP INDEX CONCURRENTLY");
    let invalid: i64 = db
        .query_one(
            "SELECT count(*) FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
         WHERE c.relname LIKE '__reshape_idx_%' AND NOT i.indisvalid",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(invalid, 1);
    blocker.batch_execute("COMMIT").unwrap();
    worker.join().unwrap().unwrap();
    let valid: bool = db
        .query_one(
            "SELECT indisvalid FROM pg_index WHERE indexrelid = 'users_value_idx'::regclass",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(valid);
    reshape.complete().unwrap();
    assert_clean(&mut db);
}

#[test]
fn persistent_blocker_exhausts_retries_with_original_error() {
    let (_reshape, mut db) = setup();
    let mut blocker = connect();
    blocker
        .batch_execute("BEGIN; LOCK TABLE users IN SHARE UPDATE EXCLUSIVE MODE")
        .unwrap();
    // This conflicts with concurrent CREATE before it creates a catalog entry.
    let pid: i32 = blocker
        .query_one("SELECT pg_backend_pid()", &[])
        .unwrap()
        .get(0);
    let error = start_index().join().unwrap().unwrap_err();
    assert_eq!(
        error.downcast_ref::<postgres::Error>().unwrap().code(),
        Some(&SqlState::LOCK_NOT_AVAILABLE)
    );
    let message = format!("{error:#}");
    assert!(message.contains("10 attempt(s)"), "{message}");
    assert!(message.contains("cleanup succeeded"), "{message}");
    assert!(message.contains(&format!("pid {pid}")), "{message}");
    // The application transaction is still alive and has not been cancelled.
    blocker.batch_execute("SELECT 1; COMMIT").unwrap();
    assert_clean(&mut db);
}

#[test]
fn failed_cleanup_can_be_aborted_later() {
    let (_reshape, mut db) = setup();
    let mut blocker = connect();
    blocker
        .batch_execute("BEGIN; UPDATE users SET value = value WHERE id = 1")
        .unwrap();
    let error = start_index().join().unwrap().unwrap_err();
    assert_eq!(
        error.downcast_ref::<postgres::Error>().unwrap().code(),
        Some(&SqlState::LOCK_NOT_AVAILABLE)
    );
    let message = format!("{error:#}");
    assert!(message.contains("cleanup failed"), "{message}");
    let count: i64 = db
        .query_one(
            "SELECT count(*) FROM reshape.data WHERE key LIKE '%_add_index'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(count, 1);
    blocker.batch_execute("COMMIT").unwrap();
    let mut recovered = Reshape::new(&connection_string()).unwrap();
    recovered.abort().unwrap();
    recovered.abort().unwrap();
    assert_clean(&mut db);
}

#[test]
fn connection_loss_reconciles_unrecorded_oid_on_abort() {
    recover_connection_loss(false);
}

#[test]
fn connection_loss_reconciles_unrecorded_oid_on_retry() {
    recover_connection_loss(true);
}

fn recover_connection_loss(retry: bool) {
    let (_reshape, mut db) = setup();
    let mut blocker = connect();
    blocker
        .batch_execute("BEGIN; UPDATE users SET value = value WHERE id = 1")
        .unwrap();
    let worker = start_index();
    let pid = wait_for_query(&mut db, "CREATE  INDEX CONCURRENTLY");
    // The unique name is durable, but CREATE has not returned an OID yet.
    let metadata: serde_json::Value = db
        .query_one(
            "SELECT value FROM reshape.data WHERE key LIKE '%_add_index'",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(metadata["index_oid"].is_null());
    db.query_one("SELECT pg_terminate_backend($1)", &[&pid])
        .unwrap();
    let error = worker.join().unwrap().unwrap_err();
    assert!(
        format!("{error:#}").contains("outcome unknown"),
        "{error:#}"
    );
    blocker.batch_execute("COMMIT").unwrap();
    let mut recovered = Reshape::new(&connection_string()).unwrap();
    if retry {
        recovered.migrate(vec![initial(), index(false)]).unwrap();
        recovered.complete().unwrap();
        assert!(oid(&mut db, "users_value_idx").is_some());
    } else {
        recovered.abort().unwrap();
    }
    assert_clean(&mut db);
}

#[test]
fn unique_failure_is_not_retried_and_leaves_no_index() {
    let (mut reshape, mut db) = setup();
    let error = reshape.migrate(vec![initial(), index(true)]).unwrap_err();
    assert_eq!(
        error.downcast_ref::<postgres::Error>().unwrap().code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    assert!(format!("{error:#}").contains("1 attempt(s)"));
    assert!(oid(&mut db, "users_value_idx").is_none());
    assert_clean(&mut db);
    db.batch_execute("UPDATE users SET value = id").unwrap();
    reshape.migrate(vec![initial(), index(true)]).unwrap();
    reshape.complete().unwrap();
    assert_clean(&mut db);
}

#[test]
fn preexisting_index_survives_conflict_and_abort() {
    let (mut reshape, mut db) = setup();
    db.batch_execute("CREATE INDEX users_value_idx ON users (id)")
        .unwrap();
    let original = oid(&mut db, "users_value_idx");
    let error = reshape.migrate(vec![initial(), index(false)]).unwrap_err();
    assert!(format!("{error:#}").contains("index name conflict"));
    reshape.abort().unwrap();
    assert_eq!(oid(&mut db, "users_value_idx"), original);
    assert_clean(&mut db);
}

#[test]
fn abort_preserves_replacement_index_with_same_name() {
    let (mut reshape, mut db) = setup();
    reshape.migrate(vec![initial(), index(false)]).unwrap();
    db.batch_execute("DROP INDEX users_value_idx; CREATE INDEX users_value_idx ON users (id)")
        .unwrap();
    let replacement = oid(&mut db, "users_value_idx");
    reshape.abort().unwrap();
    assert_eq!(oid(&mut db, "users_value_idx"), replacement);
    assert_clean(&mut db);
}

#[test]
fn interrupted_apply_reuses_successful_index() {
    let (mut reshape, mut db) = setup();
    reshape.migrate(vec![initial(), index(false)]).unwrap();
    let original = oid(&mut db, "users_value_idx");
    // Simulate interruption between action publication and the final state save.
    db.batch_execute("UPDATE reshape.data SET value = jsonb_set(value, '{state}', '\"applying\"') WHERE key = 'state'").unwrap();
    Reshape::new(&connection_string())
        .unwrap()
        .migrate(vec![initial(), index(false)])
        .unwrap();
    assert_eq!(oid(&mut db, "users_value_idx"), original);
    reshape.complete().unwrap();
    assert_clean(&mut db);
}

#[test]
fn new_renamed_table_does_not_wait_for_old_snapshots() {
    let (_reshape, mut db) = setup();
    let mut snapshot = connect();
    snapshot
        .batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ; SELECT * FROM users")
        .unwrap();
    let migration: Migration = toml::from_str(
        r#"
name = "private_index"
[[actions]]
type = "create_table"
name = "new_users"
primary_key = ["id"]
[[actions.columns]]
name = "id"
type = "INTEGER"
[[actions]]
type = "rename_table"
table = "new_users"
new_name = "customers"
[[actions]]
type = "add_index"
table = "customers"
[actions.index]
name = "customers_idx"
columns = ["id"]
where = "id > 0"
"#,
    )
    .unwrap();
    let migration = serde_json::to_string(&migration).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let result = Reshape::new(&connection_string())
            .unwrap()
            .migrate(vec![initial(), serde_json::from_str(&migration).unwrap()]);
        tx.send(result).unwrap();
    });
    let result = rx.recv_timeout(Duration::from_secs(5));
    snapshot.batch_execute("COMMIT").unwrap();
    worker.join().unwrap();
    result
        .expect("private table index waited for an unrelated snapshot")
        .unwrap();
    Reshape::new(&connection_string())
        .unwrap()
        .complete()
        .unwrap();
    assert!(oid(&mut db, "customers_idx").is_some());
    assert_clean(&mut db);
}

#[test]
fn conflict_during_publication_preserves_unrelated_index() {
    let (_reshape, mut db) = setup();
    db.batch_execute("CREATE TABLE other_users (id INTEGER)")
        .unwrap();
    let mut blocker = connect();
    blocker
        .batch_execute("BEGIN; UPDATE users SET value = value WHERE id = 1")
        .unwrap();
    let worker = start_index();
    wait_for_query(&mut db, "CREATE  INDEX CONCURRENTLY");
    db.batch_execute("CREATE INDEX users_value_idx ON other_users (id)")
        .unwrap();
    let original = oid(&mut db, "users_value_idx");
    blocker.batch_execute("COMMIT").unwrap();
    let error = worker.join().unwrap().unwrap_err();
    assert!(
        format!("{error:#}").contains("index name conflict"),
        "{error:#}"
    );
    assert_eq!(oid(&mut db, "users_value_idx"), original);
    assert_clean(&mut db);
}

#[test]
fn recovery_publishes_valid_temporary_index() {
    let (mut reshape, mut db) = setup();
    reshape.migrate(vec![initial(), index(false)]).unwrap();
    let original = oid(&mut db, "users_value_idx");
    let metadata: serde_json::Value = db
        .query_one(
            "SELECT value FROM reshape.data WHERE key LIKE '%_add_index'",
            &[],
        )
        .unwrap()
        .get(0);
    let temporary = metadata["temporary_name"].as_str().unwrap();
    // Reproduce the durable boundary just after CREATE commits but before its
    // result is recorded and the final name is published.
    db.batch_execute(&format!(
        "ALTER INDEX users_value_idx RENAME TO {temporary}"
    ))
    .unwrap();
    db.batch_execute("UPDATE reshape.data SET value = value || '{\"index_oid\": null, \"published\": false}' WHERE key LIKE '%_add_index';
        UPDATE reshape.data SET value = jsonb_set(value, '{state}', '\"applying\"') WHERE key = 'state'").unwrap();
    reshape.migrate(vec![initial(), index(false)]).unwrap();
    assert_eq!(oid(&mut db, "users_value_idx"), original);
    reshape.complete().unwrap();
    assert_clean(&mut db);
}

#[test]
fn preexisting_invalid_index_is_not_owned_by_this_action() {
    let (mut reshape, mut db) = setup();
    let error = db
        .batch_execute("CREATE UNIQUE INDEX CONCURRENTLY users_value_idx ON users (value)")
        .unwrap_err();
    assert_eq!(error.code(), Some(&SqlState::UNIQUE_VIOLATION));
    let original = oid(&mut db, "users_value_idx");
    assert!(original.is_some());
    let error = reshape.migrate(vec![initial(), index(false)]).unwrap_err();
    assert!(format!("{error:#}").contains("index name conflict"));
    reshape.abort().unwrap();
    assert_eq!(oid(&mut db, "users_value_idx"), original);
    assert_clean(&mut db);
}

#[test]
fn table_receiving_live_transformed_writes_uses_concurrent_index() {
    let (mut reshape, mut db) = setup();
    let migration: Migration = toml::from_str(
        r#"
name = "transformed"
[[actions]]
type = "create_table"
name = "profiles"
primary_key = ["id"]
[[actions.columns]]
name = "id"
type = "INTEGER"
[actions.up]
table = "users"
values = { id = "id" }
[[actions]]
type = "add_index"
table = "profiles"
[actions.index]
name = "profiles_idx"
columns = ["id"]
"#,
    )
    .unwrap();
    reshape.migrate(vec![initial(), migration]).unwrap();
    let metadata: serde_json::Value = db
        .query_one(
            "SELECT value FROM reshape.data WHERE key LIKE '%_add_index'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(metadata["concurrent"], true);
    reshape.abort().unwrap();
    assert_clean(&mut db);
}
