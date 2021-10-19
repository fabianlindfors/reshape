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
