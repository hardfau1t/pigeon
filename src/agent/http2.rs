use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Default, PartialEq, Eq, Clone, Serialize)]
pub struct Group {
    #[serde(default, rename = "environment")]
    environments: HashMap<String, Environment>,
    #[serde(default, rename = "group")]
    groups: HashMap<String, Group>,
    #[serde(default, rename = "query")]
    queries: HashMap<String, Query>,
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
pub struct Environment {
    scheme: Option<String>,
    host: Option<String>,
    port: Option<u16>,
}

impl Environment {
    pub fn apply(&mut self, other: &Self) {
        if let Some(parent_host) = &other.host {
            self.host.get_or_insert_with(|| parent_host.clone());
        }
        if let Some(parent_port) = &other.port {
            self.port.get_or_insert_with(|| parent_port.clone());
        }
    }

    /// Gives columns presennt in this structure
    /// this is used for formatting
    pub fn headers() -> &'static [&'static str] {
        &["scheme", "host", "port"]
    }

    pub fn into_row(&self) -> Vec<String> {
        let scheme = self.scheme.clone().unwrap_or_default();
        let host = self.host.clone().unwrap_or_default();
        let port = self.port.clone().map(|p| p.to_string()).unwrap_or_default();
        vec![scheme, host, port]
    }
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
pub struct Query {}

impl std::fmt::Display for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl Query {
    /// Gives columns presennt in this structure
    /// this is used for formatting
    pub fn headers() -> &'static [&'static str] {
        &[]
    }

    /// gives vec of cells, used for format printing queries
    pub fn into_row(&self) -> Vec<String> {
        vec![]
    }
}
