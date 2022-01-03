use postgres::{Client, NoTls};
use reshape::Reshape;

pub fn setup() -> (Reshape, Client, Client) {
    let connection_string = std::env::var("POSTGRES_CONNECTION_STRING")
        .unwrap_or("postgres://postgres:postgres@localhost/reshape_test".to_string());

    let old_db = Client::connect(&connection_string, NoTls).unwrap();
    let new_db = Client::connect(&connection_string, NoTls).unwrap();

    let mut reshape = Reshape::new(&connection_string).unwrap();
    reshape.remove().unwrap();

    (reshape, old_db, new_db)
}

pub fn assert_cleaned_up(db: &mut Client) {
    // Make sure no temporary columns remain
    let temp_columns: Vec<String> = db
        .query(
            "
            SELECT column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
            AND column_name LIKE '__reshape%'
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        temp_columns.is_empty(),
        "expected no temporary columns to exist, found: {}",
        temp_columns.join(", ")
    );

    // Make sure no triggers remain
    let triggers: Vec<String> = db
        .query(
            "
            SELECT trigger_name
            FROM information_schema.triggers
            WHERE trigger_schema = 'public'
            AND trigger_name LIKE '__reshape%'
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        triggers.is_empty(),
        "expected no triggers to exist, found: {}",
        triggers.join(", ")
    );

    // Make sure no functions remain
    let functions: Vec<String> = db
        .query(
            "
            SELECT routine_name
            FROM information_schema.routines
            WHERE routine_schema = 'public'
            AND routine_name LIKE '__reshape%'
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        functions.is_empty(),
        "expected no functions to exist, found: {}",
        functions.join(", ")
    );
}
