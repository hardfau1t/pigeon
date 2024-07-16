use serde::Deserialize;
use std::{collections::HashMap, path::Path, rc::Rc};

mod parser;
use parser::ServiceModule;

#[derive(Debug)]
pub struct Bundle(HashMap<String, Module>);

impl Bundle {
    pub fn keys(&self) -> std::collections::hash_map::Keys<'_, std::string::String, Module> {
        self.0.keys()
    }

    pub fn open(file_path: &impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let config = parser::Config::open(file_path)?;
        Ok(config.populate()?.into())
    }

    pub fn view<'a, I>(&self, keys: I)
        where I: IntoIterator<Item = &'a str>
    {

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
