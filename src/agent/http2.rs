use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
pub struct RestEnvironment {
    host: Option<String>,
    port: Option<u16>,
}

impl RestEnvironment {
    pub fn apply(&mut self, other: &Self) {
        if let Some(parent_host) = &other.host {
            self.host.get_or_insert_with(|| parent_host.clone());
        }
        if let Some(parent_port) = &other.port {
            self.port.get_or_insert_with(|| parent_port.clone());
        }
    }
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
pub struct Query {}
