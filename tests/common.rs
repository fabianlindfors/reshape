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
