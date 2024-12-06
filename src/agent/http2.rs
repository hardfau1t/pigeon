use serde::Deserialize;

#[derive(Debug, Deserialize, Hash, PartialEq, Eq)]
pub struct RestEnvironment {
    host: String,
    port: u16,
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq)]
pub struct Query {}
