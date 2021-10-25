use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Column {
    pub name: String,

    #[serde(rename = "type")]
    pub data_type: String,

    #[serde(default = "nullable_default")]
    pub nullable: bool,

    pub default: Option<String>,
}

fn nullable_default() -> bool {
    true
}

pub fn add_is_new_column_query(table: &str) -> String {
    format!(
        "
        ALTER TABLE {table}
        ADD COLUMN IF NOT EXISTS __reshape_is_new BOOLEAN DEFAULT FALSE NOT NULL;
        ",
        table = table,
    )
}

pub fn drop_is_new_column_query(table: &str) -> String {
    format!(
        "
        ALTER TABLE {table}
        DROP COLUMN IF EXISTS __reshape_is_new;
        ",
        table = table,
    )
}
