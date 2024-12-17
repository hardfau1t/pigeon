use serde::{Deserialize, Serialize};
use yansi::Paint;

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Query {
    description: Option<String>,
    path: String,
    method: String,
}

impl std::fmt::Display for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        static KEY_STYLE: yansi::Style = yansi::Color::Yellow.bold();
        if let Some(description) = &self.description {
            writeln!(f, "{}: {}", "description".paint(KEY_STYLE), description)?;
        }
        writeln!(f, "{}: {}", "method".paint(KEY_STYLE), self.method)?;
        writeln!(f, "{}: {}", "path".paint(KEY_STYLE), self.path)
    }
}

impl Query {
    /// Gives columns presennt in this structure
    /// this is used for formatting
    pub fn headers() -> &'static [&'static str] {
        &["method", "path"]
    }

    /// gives vec of cells, used for format printing queries
    pub fn into_row(&self) -> Vec<String> {
        vec![self.method.clone(), self.path.clone()]
    }
}
