use serde::Deserialize;
use std::{borrow::Borrow, collections::HashMap, path::Path, rc::Rc};
use tracing::{debug, error};

mod parser;
use parser::ServiceModule;

#[derive(Debug)]
pub struct Bundle(HashMap<String, Module>);

impl Bundle {
    pub fn open(file_path: &impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let config = parser::Config::open(file_path)?;
        Ok(config.populate()?.into())
    }

    pub fn view<T: Borrow<str>>(&self, keys: &[T]) {
        let mut iterator = keys.iter();
        let Some(service_name) = iterator.next() else {
            eprintln!("Available services: {:#?}", self.0.keys());
            return;
        };
        let Some(root_service) = self.0.get(service_name.borrow()) else {
            error!(
                service = service_name.borrow(),
                "Couldn't find given service"
            );
            return;
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
            return;
        };

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
}

impl From<HashMap<String, ServiceModule>> for Bundle {
    fn from(value: HashMap<String, ServiceModule>) -> Self {
        let inner = value
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
        Self(inner)
    }
}

#[derive(Debug)]
struct Module {
    environments: Vec<std::rc::Rc<Environment>>,
    endpoints: Vec<EndPoint>,
    submodules: HashMap<String, Self>,
}

impl Module {
    fn get(&self, key: &impl AsRef<str>) -> (Option<&EndPoint>, Option<&Self>) {
        let key = key.as_ref();
        let ep = self
            .endpoints
            .iter()
            .find(|ep| ep.name == key || ep.alias.as_ref().is_some_and(|alias| alias == key));
        let subm = self.submodules.get(key);

        (ep, subm)
    }
}

#[derive(Debug, Deserialize)]
pub struct Environment {
    name: String,
    scheme: String,
    host: String,
    port: Option<u16>,
    store: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct EndPoint {
    name: String,
    pub alias: Option<String>,
    method: Method,
    #[serde(default)]
    headers: HashMap<String, Vec<String>>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Body>,
    pre_hook: Option<Hook>,
    post_hook: Option<Hook>,
    path: String,
}
///
/// Http Methods
#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
enum Hook {
    Closure(()),
    #[serde(rename = "script")]
    Path(std::path::PathBuf),
}
