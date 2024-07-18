use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow, collections::HashMap, io::Write, marker::PhantomData, ops::Deref, path::Path,
    rc::Rc, str::FromStr,
};
use tracing::{debug, error, warn};

mod hook;
mod parser;
use parser::ServiceModule;

use crate::{constants, store::Store};

#[derive(Debug)]
pub struct Bundle {
    services: HashMap<String, Module>,
    package: String,
}

impl Bundle {
    pub fn open(file_path: &impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let config = parser::Config::open(file_path)?;
        let service_mods = config.populate()?;
        Ok(Self::build(&config.project, service_mods))
    }

    fn find(
        &self,
        keys: &[impl Borrow<str>],
    ) -> (
        Option<(&EndPoint<NotSubstituted>, &[Rc<Environment>])>,
        Option<&Module>,
    ) {
        let mut iterator = keys.iter();
        let Some(service_name) = iterator.next() else {
            eprintln!("Available services: {:#?}", self.services.keys());
            return (None, None);
        };
        let Some(root_service) = self.services.get(service_name.borrow()) else {
            error!(
                service = service_name.borrow(),
                "Couldn't find given service"
            );
            return (None, None);
        };
        let Ok((endpoint, last_service)) = iterator.try_fold(
            (None, Some(root_service)),
            |(_endpoints, sub_services), key| {
                if let Some(sub_service) = sub_services {
                    Ok(sub_service.get(&key.borrow()))
                } else {
                    debug!(key = key.borrow(), "Failed to find");
                    Err(())
                }
            },
        ) else {
            error!("Couldn't find given service or endpoint");
            return (None, None);
        };
        (endpoint, last_service)
    }

    pub fn view<T: Borrow<str>>(&self, keys: &[T]) {
        let (endpoint, last_service) = self.find(keys);
        eprintln!("Below are endpoints and services under {}", keys.join("."));
        if let Some(endpoint) = endpoint {
            println!("Endpoint: {:#?}", endpoint)
        }
        if let Some(service) = last_service {
            eprintln!(
                "api's under this module: {:?}",
                service
                    .endpoints
                    .iter()
                    .map(|ep| (&ep.name, &ep.alias))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "environments under this module: {:#?}",
                service.environments
            );
            eprintln!(
                "sub modules under this module: {:#?}",
                service.submodules.keys()
            );
        }
    }
    pub fn run<T: Borrow<str>>(&self, keys: &[T]) -> Result<(), anyhow::Error> {
        let (Some((endpoint, environments)), _) = self.find(keys) else {
            error!("couldn't find endpoint with {}", keys.join("."));
            return Ok(());
        };
        let mut config_store = Store::with_env(&self.package)?;
        let Some(current_env_name) = config_store.get(constants::KEY_CURRENT_ENVIRONMENT) else {
            bail!("missing {}", constants::KEY_CURRENT_ENVIRONMENT)
        };
        let Some(current_env) = environments
            .iter()
            .find(|env| &env.name == current_env_name)
        else {
            bail!(
                "{current_env_name} environment is not configured, available are: {}",
                environments
                    .iter()
                    .map(|env| env.name.as_str())
                    .collect::<Vec<_>>()
                    .as_slice()
                    .join(", ")
            )
        };
        current_env.store.iter().for_each(|(key, value)| {
            let entry = config_store.entry(key.clone());
            entry.or_insert(value.clone());
        });
        let built_endpoint = endpoint.substitute(&config_store)?;
        built_endpoint.execute(current_env.as_ref().try_into()?, &mut config_store)
    }

    fn build(package: &impl Borrow<str>, service_mods: HashMap<String, ServiceModule>) -> Self {
        let inner = service_mods
            .into_iter()
            .map(|(name, service_mod)| {
                let module = {
                    let ServiceModule {
                        environments: service_mod_environments,
                        endpoints,
                        submodules,
                    } = service_mod;
                    let environments = service_mod_environments
                        .into_iter()
                        .map(|environ| Rc::new(environ))
                        .collect::<Vec<_>>();

                    let submodules = submodules
                        .into_iter()
                        .map(|(name, sub_mod)| {
                            let module = sub_mod.into_module(&environments);
                            (name, module)
                        })
                        .collect();

                    Module {
                        environments,
                        endpoints,
                        submodules,
                    }
                };
                (name, module)
            })
            .collect::<HashMap<String, Module>>();
        Self {
            services: inner,
            package: package.borrow().to_string(),
        }
    }
}

#[derive(Debug)]
struct Module {
    environments: Vec<std::rc::Rc<Environment>>,
    endpoints: Vec<EndPoint<NotSubstituted>>,
    submodules: HashMap<String, Self>,
}

impl Module {
    fn get(
        &self,
        key: &impl AsRef<str>,
    ) -> (
        Option<(&EndPoint<NotSubstituted>, &[Rc<Environment>])>,
        Option<&Self>,
    ) {
        let key = key.as_ref();
        let ep = self
            .endpoints
            .iter()
            .find(|ep| ep.name == key || ep.alias.as_ref().is_some_and(|alias| alias == key))
            .map(|ep| (ep, self.environments.as_slice()));
        let subm = self.submodules.get(key);

        (ep, subm)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    name: String,
    #[serde(deserialize_with = "deserialize_scheme")]
    scheme: http::uri::Scheme,
    host: String,
    port: Option<u16>,
    store: HashMap<String, String>,
}

impl TryInto<url::Url> for &Environment {
    type Error = url::ParseError;

    fn try_into(self) -> Result<url::Url, Self::Error> {
        if let Some(port) = self.port {
            url::Url::from_str(&format!(
                "{}://{}:{}",
                self.scheme.as_str(),
                self.host,
                port
            ))
        } else {
            url::Url::from_str(&format!("{}://{}", self.scheme.as_str(), self.host))
        }
    }
}

/// deserialization function for uri scheme
fn deserialize_scheme<'de, D>(deserializer: D) -> Result<http::uri::Scheme, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let str_val = String::deserialize(deserializer)?;
    <http::uri::Scheme as std::str::FromStr>::from_str(&str_val)
        .map_err(|e| serde::de::Error::custom(format!("Failed to parse uri: {e:?}")))
}

#[derive(Debug)]
struct Substituted;
#[derive(Debug)]
struct NotSubstituted;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndPoint<T> {
    name: String,
    pub alias: Option<String>,
    method: Method,
    #[serde(default)]
    headers: HashMap<String, Vec<String>>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Body>,
    pre_hook: Option<hook::Hook>,
    post_hook: Option<hook::Hook>,
    path: String,
    #[serde(skip)]
    _t: PhantomData<T>,
}
impl EndPoint<NotSubstituted> {
    fn substitute(&self, config_store: &Store) -> Result<EndPoint<Substituted>, subst::Error> {
        let key_val_store = config_store.deref();
        let url_path = subst::substitute(&self.path, key_val_store)?;
        let mut params = Vec::with_capacity(self.params.len());
        for (key, val) in &self.params {
            let key = subst::substitute(key, key_val_store)?;
            let val = subst::substitute(val, key_val_store)?;
            params.push((key, val))
        }
        let mut headers = HashMap::with_capacity(self.headers.len());
        for (key, values) in &self.headers {
            let mut values_subst = Vec::with_capacity(values.len());
            for val in values {
                let val = subst::substitute(val, key_val_store)?;
                values_subst.push(val)
            }
            let key = subst::substitute(key, key_val_store)?;
            headers.insert(key, values_subst);
        }
        Ok(EndPoint::<Substituted> {
            name: self.name.clone(),
            alias: self.alias.clone(),
            method: self.method,
            headers,
            params,
            body: self.body.clone(),
            pre_hook: self.pre_hook.clone(),
            post_hook: self.post_hook.clone(),
            path: url_path,
            _t: PhantomData,
        })
    }
}

impl EndPoint<Substituted> {
    fn execute(self, base_url: url::Url, config_store: &mut Store) -> anyhow::Result<()> {
        let request_hook_flags = Option::<&str>::None.as_slice();
        let response_hook_flags = Option::<&str>::None.as_slice();
        let Self {
            method,
            mut headers,
            params,
            body,
            pre_hook,
            post_hook,
            path,
            ..
        } = self;

        let body = body
            .map(|body| match body.data {
                BodyData::Inline(d) => Ok((body.kind, d.into_bytes())),
                BodyData::Path(path) => std::fs::read(path).map(|content| (body.kind, content)),
            })
            .transpose()?;
        let body = body.map(|(kind, body)| {
            headers.insert("Content-Type".to_string(), vec![kind]);
            body
        });
        let request_object = RequestHookObject {
            headers,
            params,
            body,
            path,
            method,
        };
        let mapped_request_obj = pre_hook
            .map(|hook| hook.run(&request_object, request_hook_flags))
            .transpose()?
            .unwrap_or(request_object);
        let request = mapped_request_obj.into_request(base_url)?;

        // generate request object
        let resp = if let Some(body) = mapped_request_obj.body {
            request.send_bytes(body.as_slice())
        } else {
            request.call()
        };
        let response = match resp {
            Ok(ok_val) => ok_val,
            Err(e) => match e {
                ureq::Error::Status(code, response) => {
                    warn!("Request Failed with code: {code}");
                    response
                }
                ureq::Error::Transport(e) => {
                    bail!("Transport error occurred during processing of request: {e}")
                }
            },
        };
        let post_hook_obj: ResponseHookObject = response.into();
        let hook_response = post_hook
            .map(|hook| hook.run(&post_hook_obj, response_hook_flags))
            .transpose()?
            .unwrap_or(post_hook_obj);
        if let Some(body) = hook_response.body {
            std::io::stdout().write_all(&body)?
        };
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RequestHookObject {
    #[serde(default)]
    headers: HashMap<String, Vec<String>>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    path: String,
    method: Method,
}

impl RequestHookObject {
    fn into_request(&self, base_url: url::Url) -> Result<ureq::Request, url::ParseError> {
        let url = base_url.join(self.path.as_str())?;
        let request = ureq::request(&self.method.to_string(), url.as_str());

        // add headers
        let request = self
            .headers
            .iter()
            .fold(request, |request, (key, values)| {
                values.into_iter().fold(request, |request, value| {
                    request.set(key.as_str(), value.as_str())
                })
            })
            .query_pairs(
                self.params
                    .iter()
                    .map(|(key, val)| (key.as_str(), val.as_str())),
            );
        Ok(request)
    }
}

/// this will be given to prehook script
#[derive(Debug, Deserialize, Serialize)]
struct ResponseHookObject {
    headers: HashMap<String, Vec<String>>,
    body: Option<Vec<u8>>,
    status: u16,
    status_text: String,
}

impl From<ureq::Response> for ResponseHookObject {
    fn from(response: ureq::Response) -> Self {
        let mut body = Vec::new();
        let header_keys = response.headers_names();
        let status = response.status();
        let status_text = response.status_text().to_string();
        let headers: HashMap<_, _> = header_keys
            .into_iter()
            .map(|header_name| {
                let vals = response
                    .all(&header_name)
                    .iter()
                    .map(|val_ref| val_ref.to_string())
                    .collect();
                (header_name, vals)
            })
            .collect();
        if let Err(e) = response.into_reader().read_to_end(&mut body) {
            warn!("Error while reading response body: {e}, truncate body");
            body.clear();
        };
        ResponseHookObject {
            headers,
            body: Some(body),
            status,
            status_text,
        }
    }
}

///
/// Http Methods
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub enum Method {
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

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str_repr = match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Connect => "CONNECT",
            Method::Patch => "PATCH",
            Method::Trace => "TRACE",
        };
        f.write_str(str_repr)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Body {
    kind: String,
    #[serde(flatten)]
    data: BodyData,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
enum BodyData {
    #[serde(rename = "data")]
    Inline(String),
    #[serde(rename = "file")]
    Path(std::path::PathBuf),
}
