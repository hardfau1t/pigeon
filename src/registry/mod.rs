use color_eyre::eyre::{bail, Context};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow, collections::HashMap, marker::PhantomData, ops::Deref, path::Path, rc::Rc,
    str::FromStr,
};
use tracing::{debug, error, info, instrument, trace, warn};

mod hook;
mod parser;
use parser::ServiceModule;

use crate::{constants, store::Store};

/// Set of Services
#[derive(Debug, Serialize)]
pub struct Bundle {
    services: HashMap<String, Module>,
    package: String,
}

impl Bundle {
    #[instrument(skip(file_path))]
    pub fn open(file_path: &impl AsRef<Path>) -> Result<Self, color_eyre::Report> {
        let config = parser::Config::open(file_path)?;
        let service_mods = config.populate()?;
        Ok(Self::build(&config.project, service_mods))
    }

    /// finds endpoint or module pointed by series of keys
    ///
    /// * `keys`: list of keys pointing to endpoint or another submodule
    ///
    /// # returns
    /// 1. Optional endpoint with set of environments it contains
    /// 2. Optional submodule with given path
    #[instrument(skip(keys))]
    fn find(
        &self,
        keys: &[impl Borrow<str>],
    ) -> (
        Option<(&EndPoint<NotSubstituted>, &HashMap<String, Rc<Environment>>)>,
        Option<&Module>,
    ) {
        let mut key_iterator = keys.iter();
        let Some(service_name) = key_iterator.next() else {
            return (None, None);
        };
        // first key should be service and should exist
        let Some(root_service) = self.services.get(service_name.borrow()) else {
            error!(
                service = service_name.borrow(),
                "Couldn't find given service"
            );
            return (None, None);
        };
        let Ok((endpoint, last_service)) = key_iterator.try_fold(
            (None, Some(root_service)),
            |(_endpoints, current_service), key| {
                if let Some(sub_service) = current_service {
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

    #[instrument(skip(keys))]
    pub fn view<T: Borrow<str>>(&self, keys: &[T]) {
        let Some(last_key) = keys.last().map(|l| l.borrow()) else {
            // the list is empty so show only list of services
            eprintln!("Available services: {:#?}", self.services.keys());
            return;
        };
        let (endpoint, last_service) = self.find(keys);
        if let Some((endpoint, environ)) = endpoint {
            eprintln!("======== {} ======\n{}", last_key, endpoint);
            eprintln!("Environments:");
            environ.iter().for_each(|(env_name, env)| {
                eprintln!(
                    "\t{env_name}: {}\n\t\theaders: {:?}",
                    env.as_ref(),
                    env.headers
                )
            });
        }
        if let Some(service) = last_service {
            eprintln!("====== {last_key} ======\n{service}");
        }
    }

    /// run query pointed by keys
    ///
    /// * `keys`: path which points to given query
    /// * `flags`: flags for hooks `--` will separate flags into pre-hook and post-hook flags
    /// * `persistent_config`: whether to store changes to config back
    #[instrument(skip(hooks_flags, self))]
    pub fn run<T: Borrow<str> + std::fmt::Debug>(
        &self,
        keys: &[T],
        hooks_flags: &[impl Borrow<str>],
        persistent_config: bool,
        dry_run: bool,
        skip_prehook: bool,
        skip_posthook: bool,
    ) -> Result<Option<Vec<u8>>, color_eyre::Report> {
        trace!("running query");
        let (Some((endpoint, environments)), _) = self.find(keys) else {
            bail!("couldn't find endpoint with {}", keys.join("."));
        };
        let mut config_store = Store::with_env(&self.package)?;
        debug!("current config: {config_store:?}");
        config_store.persistent(persistent_config);
        let Some(current_env_name) = config_store.get(constants::KEY_CURRENT_ENVIRONMENT) else {
            bail!(
                "missing environment, set: {}",
                constants::KEY_CURRENT_ENVIRONMENT
            )
        };
        let Some(current_env) = environments.get(current_env_name) else {
            let a = environments
                .keys()
                .map(|key| key.as_str())
                .collect::<Vec<_>>()
                .as_slice()
                .join(", ");
            bail!(
                "{current_env_name} environment is not configured, available are: {}",
                a
            )
        };
        debug!("Current environment: {current_env:?}");
        current_env.store.iter().for_each(|(key, value)| {
            let entry = config_store.entry(key.clone());
            entry.or_insert(value.clone());
        });
        let built_endpoint = endpoint
            .substitute(&config_store, &current_env.as_ref().headers)
            .wrap_err("Failed to substitute key values in query")?;
        built_endpoint.execute(
            current_env.as_ref().try_into()?,
            &mut config_store,
            hooks_flags,
            dry_run,
            skip_prehook,
            skip_posthook,
        )
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
                        alias,
                        description,
                    } = service_mod;
                    let environments = service_mod_environments
                        .into_iter()
                        .map(|(name, environ)| (name, Rc::new(environ)))
                        .collect::<HashMap<_, _>>();

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
                        alias,
                        description,
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

#[derive(Debug, Serialize)]
struct Module {
    alias: Option<String>,
    description: Option<String>,
    environments: HashMap<String, std::rc::Rc<Environment>>,
    endpoints: HashMap<String, EndPoint<NotSubstituted>>,
    submodules: HashMap<String, Self>,
}

impl Module {
    fn get(
        &self,
        key: &impl AsRef<str>,
    ) -> (
        Option<(&EndPoint<NotSubstituted>, &HashMap<String, Rc<Environment>>)>,
        Option<&Self>,
    ) {
        let key = key.as_ref();
        let ep = self.endpoints.get(key).map(|ep| (ep, &self.environments));
        let subm = self.submodules.get(key);

        (ep, subm)
    }
}

impl std::fmt::Display for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(alias) = &self.alias {
            writeln!(f, "alias: {alias}")?;
        }
        if let Some(description) = &self.description {
            writeln!(f, "description: {description}")?;
        }
        writeln!(f, "environments:")?;
        for (env_name, environ) in &self.environments {
            writeln!(f, "\t* {env_name}: {}", environ.as_ref())?
        }
        if !self.endpoints.is_empty() {
            writeln!(f, "endpoints:")?;
            for (ep_name, ep) in &self.endpoints {
                write!(f, "\t- {ep_name}")?;
                if let Some(alias) = &ep.alias {
                    writeln!(f, " ({alias})")?;
                } else {
                    writeln!(f)?;
                }
            }
        }
        if !self.submodules.is_empty() {
            writeln!(f, "submodules:")?;
            for sub_mod in self.submodules.keys() {
                writeln!(f, "\t- {sub_mod}")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    #[serde(with = "serde_scheme")]
    scheme: http::uri::Scheme,
    host: String,
    port: Option<u16>,
    // this will be applied to path of endpoint
    prefix: Option<String>,
    // common headers which are applied to each query
    // headers in query has more priority than this
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    store: HashMap<String, String>,
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}://{}{}/{}",
            self.scheme,
            self.host,
            self.port
                .map(|port| format!(":{}", port))
                .unwrap_or("".to_string()),
            self.prefix.as_deref().unwrap_or(""),
        )
    }
}

impl TryInto<url::Url> for &Environment {
    type Error = url::ParseError;

    fn try_into(self) -> Result<url::Url, Self::Error> {
        let port_str = if let Some(port) = self.port {
            format!(":{port}")
        } else {
            "".to_string()
        };
        url::Url::from_str(&format!(
            "{}://{}:{}/{}",
            self.scheme.as_str(),
            self.host,
            port_str,
            self.prefix.as_deref().unwrap_or("")
        ))
    }
}

mod serde_scheme {
    use serde::{Deserialize, Serializer};

    pub(super) fn serialize<S>(scheme: &http::uri::Scheme, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(scheme.as_str())
    }
    /// deserialization function for uri scheme
    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<http::uri::Scheme, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str_val = String::deserialize(deserializer)?;
        <http::uri::Scheme as std::str::FromStr>::from_str(&str_val)
            .map_err(|e| serde::de::Error::custom(format!("Failed to parse uri: {e:?}")))
    }
}

#[derive(Debug)]
struct Substituted;
#[derive(Debug)]
struct NotSubstituted;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndPoint<T> {
    description: Option<String>,
    alias: Option<String>,
    method: Method,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Body>,
    pre_hook: Option<hook::Hook>,
    post_hook: Option<hook::Hook>,
    path: String,
    #[serde(skip)]
    _t: PhantomData<T>,
}

impl<T> std::fmt::Display for EndPoint<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} {}", self.method, self.path)?;
        if let Some(ref desc) = self.description {
            writeln!(f, "{desc}.")?
        }
        if let Some(ref alias) = self.alias {
            writeln!(f, "alias: {alias}")?
        }
        if !self.params.is_empty() {
            writeln!(f, "params: {:?}", self.params)?
        }
        if !self.headers.is_empty() {
            writeln!(f, "headers:")?;
            self.headers
                .iter()
                .try_fold((), |_, (header_name, header_values)| {
                    //format!()
                    writeln!(f, "\t{header_name}: {header_values:?}")?;
                    Ok(())
                })?;
        }
        if let Some(ref body) = self.body {
            writeln!(f, "body:\n{body:?}")?
        }
        Ok(())
    }
}
impl EndPoint<NotSubstituted> {
    #[instrument(skip(self, config_store))]
    fn substitute(
        &self,
        config_store: &Store,
        base_headers: &HashMap<String, String>,
    ) -> Result<EndPoint<Substituted>, subst::Error> {
        trace!("Constructing query by substing values from config_store");
        let key_val_store = config_store.deref();
        let url_path = subst::substitute(&self.path, key_val_store)?;
        let mut params = Vec::with_capacity(self.params.len());
        for (key, val) in &self.params {
            let key = subst::substitute(key, key_val_store)?;
            let val = subst::substitute(val, key_val_store)?;
            params.push((key, val))
        }
        let mut headers = HashMap::with_capacity(self.headers.len());
        for (key, value) in base_headers.iter().chain(&self.headers) {
            let values_subst = subst::substitute(value, key_val_store)?;
            let key = subst::substitute(key, key_val_store)?;
            headers.insert(key, values_subst);
        }
        Ok(EndPoint::<Substituted> {
            description: self.description.clone(),
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
    fn execute(
        self,
        base_url: url::Url,
        config_store: &mut Store,
        flags: &[impl Borrow<str>],
        dry_run: bool,
        skip_prehook: bool,
        skip_posthook: bool,
    ) -> color_eyre::Result<Option<Vec<u8>>> {
        trace!("executing query");
        let mut flags_iter = flags.split(|flag| flag.borrow() == "--");
        let request_hook_flags = flags_iter.next().unwrap_or(&[]);
        let response_hook_flags = flags_iter.next().unwrap_or(&[]);
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
            headers.insert("Content-Type".to_string(), kind);
            body
        });
        let request_object = RequestHookObject {
            headers,
            params,
            body,
            path,
            method,
            // HACK: I couldn't figure out how to send reference of hashmap to sereialize and take hashmap from deserialize
            config: config_store.deref().deref().clone(),
        };

        // run pre-hook if it is available
        let mut mapped_request_obj = pre_hook
            .filter(|_| !skip_prehook)
            .map(|hook| hook.run(&request_object, request_hook_flags))
            .transpose()?
            .map(|mut obj| {
                // pre hook was present so update the config store
                config_store.extend(obj.config.drain());
                obj
            })
            .unwrap_or(request_object);
        let body = mapped_request_obj.body.take();
        let request = mapped_request_obj.into_request(base_url)?;
        info!("Query {} {}", request.method(), request.url());
        info!("headers:\n{}", {
            let mut headers = request.header_names();
            headers.dedup();
            headers
                .iter()
                .map(|key| {
                    let value = request.all(key).join(",");
                    format!("> {key}: {value}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        });

        // generate request object
        let resp = if let Some(body) = body {
            match std::str::from_utf8(body.as_slice()) {
                Ok(str_body) => info!("request body: '{str_body}'"),
                Err(e) => {
                    warn!("Couldn't decode body as utf8 string: {e}");
                    info!("request body: {body:x?}");
                }
            }
            if dry_run {
                return Ok(None);
            }
            request.send_bytes(body.as_slice())
        } else {
            if dry_run {
                return Ok(None);
            }

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

        let post_hook_obj: ResponseHookObject =
            ResponseHookObject::from_response(response, config_store.deref().deref().clone());
        // display response
        info!(
            "response status: {} {}",
            post_hook_obj.status, post_hook_obj.status_text
        );

        info!(
            "headers:\n{}",
            post_hook_obj
                .headers
                .iter()
                .map(|(key, value)| {
                    format!("< {key}: {value}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        );
        let hook_response = post_hook
            .filter(|_| !skip_posthook)
            .map(|hook| hook.run(&post_hook_obj, response_hook_flags))
            .transpose()?
            .map(|mut obj| {
                config_store.extend(obj.config.drain());
                obj
            })
            .unwrap_or(post_hook_obj);
        Ok(hook_response.body)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RequestHookObject {
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    path: String,
    method: Method,
    #[serde(default)]
    config: HashMap<String, String>,
}

impl RequestHookObject {
    fn into_request(self, base_url: url::Url) -> Result<ureq::Request, url::ParseError> {
        let url = base_url.join(self.path.as_str())?;
        let request = ureq::request(&self.method.to_string(), url.as_str());

        // add headers
        let request = self
            .headers
            .iter()
            .fold(request, |request, (key, value)| {
                request.set(key.as_str(), value.as_str())
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
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    status: u16,
    status_text: String,
    #[serde(default)]
    config: HashMap<String, String>,
}

impl ResponseHookObject {
    fn from_response(response: ureq::Response, config: HashMap<String, String>) -> Self {
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
            config,
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

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct Body {
    kind: String,
    #[serde(flatten)]
    data: BodyData,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
enum BodyData {
    #[serde(rename = "data")]
    Inline(String),
    #[serde(rename = "file")]
    Path(std::path::PathBuf),
}
