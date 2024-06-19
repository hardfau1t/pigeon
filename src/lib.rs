use serde::Deserialize;
use std::{io::Write, str::FromStr};
use tracing::{debug, info};

pub mod constants;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    version: String,
    #[serde(rename = "environment")]
    environments: Vec<Environment>,
    #[serde(rename = "service")]
    pub services: Vec<Service>,
}

impl Document {
    pub fn get_service(&self, service_name: &str) -> Option<&Service> {
        self.services.iter().find(|svc| {
            svc.name == service_name
                || svc
                    .alias
                    .as_ref()
                    .is_some_and(|alias| alias == service_name)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Environment {
    name: String,
    service: Vec<ServiceEnv>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceEnv {
    name: String,
    #[serde(deserialize_with = "deserialize_scheme")]
    scheme: http::uri::Scheme,
    host: String,
}

impl TryInto<url::Url> for &ServiceEnv {
    type Error = url::ParseError;

    fn try_into(self) -> Result<url::Url, Self::Error> {
        url::Url::from_str(&format!("{}://{}", self.scheme.as_str(), self.host))
    }
}

/// represents single microservice
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub alias: Option<String>,
    pub endpoint: Vec<EndPoint>,
}

impl Service {
    pub fn get_endpoint(&self, ep_name: &str) -> Option<&EndPoint> {
        self.endpoint.iter().find(|ep| {
            ep.name == ep_name || ep.alias.as_ref().is_some_and(|alias| alias == ep_name)
        })
    }
}

/// represents one endpoint of given microservice
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndPoint {
    pub name: String,
    pub alias: Option<String>,
    method: Method,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Body>,
    #[serde(default)]
    pre_hook: Vec<Hook>,
    #[serde(default)]
    post_hook: Vec<Hook>,
    #[serde(default)]
    flags: Vec<String>,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    kind: String,
    #[serde(flatten)]
    data: BodyData,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
enum BodyData {
    #[serde(rename = "data")]
    Inline(String),
    #[serde(rename = "file")]
    Path(std::path::PathBuf),
}

/// Http Methods
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
enum Method {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Options,
    Connect,
    Patch,
    Trace,
}

impl std::string::ToString for Method {
    fn to_string(&self) -> String {
        match self {
            Method::Get => "GET".to_string(),
            Method::Post => "POST".to_string(),
            Method::Put => "PUT".to_string(),
            Method::Delete => "DELETE".to_string(),
            Method::Head => "HEAD".to_string(),
            Method::Options => "OPTIONS".to_string(),
            Method::Connect => "CONNECT".to_string(),
            Method::Patch => "PATCH".to_string(),
            Method::Trace => "TRACE".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
enum Hook {
    Closure(String),
    #[serde(rename = "script")]
    Path(std::path::PathBuf),
}

/// deserialization function for uri scheme
fn deserialize_scheme<'de, D>(deserializer: D) -> Result<http::uri::Scheme, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let str_val = String::deserialize(deserializer)?;
    http::uri::Scheme::from_str(&str_val)
        .map_err(|e| serde::de::Error::custom(format!("Failed to parse uri: {e:?}")))
}

/// parses document and run given query
pub fn execute(
    document: &Document,
    service_name: &str,
    endpoint_name: &str,
) -> Result<(), anyhow::Error> {
    let current_env = std::env::var(constants::KEY_CURRENT_ENVIRONMENT)?;
    // get service with given service_name
    let service = document
        .services
        .iter()
        .find_map(|svc| {
            if svc.name == service_name
                || svc
                    .alias
                    .as_ref()
                    .is_some_and(|alias| alias == service_name)
            {
                Some(svc)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to get service with name/alias: {service_name}"))?;
    // get the service config of given service in given environment
    let env = document
        .environments
        .iter()
        .find_map(|env| {
            if env.name == current_env {
                Some(env)
            } else {
                None
            }
        })
        .and_then(|env| {
            env.service.iter().find_map(|service_env| {
                if service_env.name == service.name {
                    Some(service_env)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Failed to get service: {service_name} from env: {current_env}")
        })?;
    // seach for given endpoint
    let endpoint = service
        .endpoint
        .iter()
        .find_map(|ep| {
            if ep.name == endpoint_name
                || ep
                    .alias
                    .as_ref()
                    .is_some_and(|alias| alias == endpoint_name)
            {
                Some(ep)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Couldn't find such endpoint: {endpoint_name} in service {service_name}"
            )
        })?;
    let host = env.try_into()?;
    call_request(host, endpoint)
}

fn call_request(host: url::Url, endpoint: &EndPoint) -> anyhow::Result<()> {
    let uri = host.join(&endpoint.path)?;
    let req = ureq::request(&endpoint.method.to_string(), uri.as_str());
    debug!(headers = ?endpoint.headers);
    let req = endpoint
        .headers
        .iter()
        .fold(req, |req, header| req.set(&header.0, &header.1))
        // set query params
        .query_pairs(
            endpoint
                .params
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
    let response = if let Some(ref body) = endpoint.body {
        let data = match &body.data {
            BodyData::Path(file_path) => std::fs::read_to_string(file_path)?,
            BodyData::Inline(data) => data.clone(),
        };
        debug!(uri=?uri, request= ?req, "sending request");
        req.set("Content-Type", &body.kind).send_string(&data)
    } else {
        debug!(uri=?uri, request= ?req, "sending request");
        req.call()
    }?;
    std::io::stdout().write_all(response.into_string()?.as_bytes())?;
    Ok(())
}
