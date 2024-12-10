use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
pub struct RestEnvironment {
    host: String,
    port: u16,
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
pub struct Query {}
