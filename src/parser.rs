use miette::{bail, ensure, Context, IntoDiagnostic};
use semver::Version;
use serde::Deserialize;
use serde::{de::DeserializeOwned, Serialize};
use std::ops::Deref;
use std::{
    collections::HashMap,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
};
use tracing::{debug, error, instrument, trace, warn};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Given config is not supported")]
    UnSupportedVersion,
    #[error("Failed to read config file content")]
    CouldntReadFile(#[from] std::io::Error),
    #[error("Failed to deserialize config file")]
    InvalidConfigFile(#[from] toml::de::Error),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: Version,
    /// To distinguish different versions of identifiers
    pub project: String,
    /// where to find for api's
    api_directory: PathBuf,
}

impl Config {
    /// read the config file and do the version check and parse the config file
    pub fn open(file_path: &impl AsRef<Path>) -> Result<Self, ConfigError> {
        let current_package_version =
            Version::parse(env!("CARGO_PKG_VERSION")).expect("cargo pkg is not semver?");
        debug!(version=?current_package_version, "current binary version");
        let config = toml::from_str::<Self>(&std::fs::read_to_string(file_path.as_ref())?)?;

        if current_package_version.major != config.version.major {
            error!(binary_version=?current_package_version, config_version=?config.version, "major versions of binary and config are not matching");
            return Err(ConfigError::UnSupportedVersion);
        }

        if current_package_version.major == 0
            && current_package_version.minor != config.version.minor
        {
            // 0 major version is beta stage so breaking changes are expected at minor versions
            error!(binary_version=?current_package_version, config_version=?config.version, "binary version is beta version and minor versions are not matching");
            return Err(ConfigError::UnSupportedVersion);
        }
        if current_package_version < config.version {
            warn!(binary_version=?current_package_version, config_version=?config.version, "binary version is smaller than config, things may not work as expected");
        }
        Ok(config)
    }

    pub fn populate(&self) -> miette::Result<HashMap<String, ServiceBuilder>> {
        let services = self
            .api_directory
            .read_dir()
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't read api directory {:?}", self.api_directory))?
            .map(|file| {
                let dir_entry = file
                    .into_diagnostic()
                    .wrap_err("Failed to read entry of service directory")?;
                let ft = dir_entry
                    .file_type()
                    .into_diagnostic()
                    .wrap_err("Couldn't evaluate file type")?;

                if ft.is_file() {
                    trace!(file=?dir_entry.path(), "parsing as file");
                    let mapped_module = parse_file::<ServiceBuilder>(&dir_entry.path())?;
                    Ok(mapped_module)
                } else if ft.is_dir() {
                    trace!(file=?dir_entry.path(), "parsing as directory");

                    let Some(module_name) =
                        dir_entry.file_name().to_str().map(|name| name.to_string())
                    else {
                        bail!(
                            "service name {:?} is not valid utf-8, please change it utf-8 string",
                            dir_entry.file_name()
                        );
                    };
                    let mapped_module = ServiceBuilder::from_dir(&dir_entry.path())
                        .map(|sm| (module_name, sm))
                        .wrap_err_with(|| format!("Couldn't read directory {:?}", dir_entry))?;

                    Ok(mapped_module)
                } else {
                    bail!(
                        "direntry {:?} is neither file or directory, its not handled yet skipping",
                        dir_entry.path()
                    )
                }
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(services)
    }
}

/// Set of Services
#[derive(Debug, Serialize)]
pub struct Bundle {
    pub services: HashMap<String, Service>,
    pub package: String,
}

impl Bundle {
    #[instrument(skip(file_path))]
    pub fn open(file_path: &impl AsRef<Path>) -> miette::Result<Self> {
        let config = Config::open(file_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to gather config from {:?}", file_path.as_ref()))?;
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
    pub fn find(
        &self,
        keys: &[impl std::borrow::Borrow<str>],
    ) -> (
        Option<(&RawEndpoint, &HashMap<String, Rc<Environment>>)>,
        Option<&Service>,
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
    pub fn view<T: std::borrow::Borrow<str>>(&self, keys: &[T]) {
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

    fn build(
        package: &impl std::borrow::Borrow<str>,
        service_mods: HashMap<String, ServiceBuilder>,
    ) -> Self {
        let inner = service_mods
            .into_iter()
            .map(|(name, service_mod)| {
                let module = {
                    let ServiceBuilder {
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

                    Service {
                        environments,
                        endpoints,
                        submodules,
                        alias,
                        description,
                    }
                };
                (name, module)
            })
            .collect::<HashMap<String, Service>>();
        Self {
            services: inner,
            package: package.borrow().to_string(),
        }
    }
}

type RawEndpoint = EndPoint<NotSubstituted>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceBuilder {
    #[serde(default)]
    pub alias: Option<String>,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(rename = "environment")]
    pub environments: HashMap<String, Environment>,

    #[serde(default)]
    #[serde(rename = "endpoint")]
    pub endpoints: HashMap<String, RawEndpoint>,

    #[serde(default)]
    #[serde(rename = "submodule")]
    pub submodules: HashMap<String, SubModule>,
}

#[derive(Debug, Serialize)]
pub struct Service {
    alias: Option<String>,
    description: Option<String>,
    environments: HashMap<String, std::rc::Rc<Environment>>,
    endpoints: HashMap<String, RawEndpoint>,
    submodules: HashMap<String, Self>,
}

impl Service {
    fn get(
        &self,
        key: &impl AsRef<str>,
    ) -> (
        Option<(&RawEndpoint, &HashMap<String, Rc<Environment>>)>,
        Option<&Self>,
    ) {
        let key = key.as_ref();
        let ep = self.endpoints.get(key).map(|ep| (ep, &self.environments));
        let subm = self.submodules.get(key);

        (ep, subm)
    }
}

impl std::fmt::Display for Service {
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
    pub scheme: http::uri::Scheme,
    pub host: String,
    pub port: Option<u16>,
    // this will be applied to path of endpoint
    pub prefix: Option<String>,
    // common headers which are applied to each query
    // headers in query has more priority than this
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub store: HashMap<String, String>,
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
            "{}://{}{}/{}/",
            self.scheme.as_str(),
            self.host,
            port_str,
            self.prefix
                .as_deref()
                .map(|prefix| prefix.trim_matches('/'))
                .unwrap_or("")
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

fn parse_file<T: DeserializeOwned>(path_ref: &impl AsRef<Path>) -> miette::Result<(String, T)> {
    let path = path_ref.as_ref();
    ensure!(
        path.extension().is_some_and(|ext| ext == "toml"),
        "Unexpected non toml file {:?} in services directory",
        path
    );

    let content = std::fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("couldn't read file content {path:?}"))?;
    let module_name = path
        .file_stem()
        .expect("already checked that there is stem")
        .to_str()
        .ok_or_else(|| miette::miette!("Couldn't convert os_str to utf-8 string: {path:?} "))?
        .to_string();
    Ok((
        module_name,
        toml::from_str::<T>(&content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't deserialize {path:?}"))?,
    ))
}

impl ServiceBuilder {
    fn from_dir(path_ref: &impl AsRef<Path>) -> miette::Result<Self> {
        let mut path_buf: PathBuf = path_ref.as_ref().into();
        let submodules = path_buf
            .read_dir()
            .into_diagnostic()?
            .filter(|dir_entry_res| {
                // index.toml will be handled separately, skipp that file
                dir_entry_res
                    .as_ref()
                    .is_ok_and(|dir_entry| dir_entry.file_name() != "index.toml")
            })
            .map(|dir_entry_res| {
                // from dir entry if that is file then read it directly else recursively desend and parse inner files
                let dir_entry = dir_entry_res
                    .into_diagnostic()
                    .wrap_err("Couldn't read the directory entry")?;
                let file_type = dir_entry
                    .file_type()
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Couldn't get file type of {dir_entry:?}"))?;
                if file_type.is_dir() {
                    let subm = SubModule::from_dir(&dir_entry.path()).wrap_err_with(|| {
                        format!("Failed to parse submodule from {dir_entry:?}, skipping")
                    })?;
                    let mod_name = dir_entry
                        .file_name()
                        .to_str()
                        .map(|s| s.to_string())
                        .ok_or(miette::miette!(
                        "file path {dir_entry:?} is not utf-8, non utf-8 paths are not supported"
                    ))?;
                    Ok((mod_name, subm))
                } else if file_type.is_file() {
                    parse_file::<SubModule>(&dir_entry.path())
                } else {
                    bail!("unsupported file type {file_type:?} of {dir_entry:?}")
                }
            });
        path_buf.push("index.toml");
        let module_content = std::fs::read_to_string(&path_buf)
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't read file {:?}", path_buf))?;
        let mut module = toml::from_str::<Self>(&module_content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed deserialize file {:?}", path_buf))?;
        for other_subm in submodules {
            let (name, subm) = other_subm?;
            module.submodules.insert(name, subm);
        }
        Ok(module)
    }
}

/// Used incase of environments in submodules
/// these will be used to override environment configurations defined in service-module
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentBuilder {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_scheme")]
    scheme: Option<http::uri::Scheme>,
    host: Option<String>,
    port: Option<u16>,
    prefix: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,

    #[serde(default)]
    store: HashMap<String, String>,
}
/// deserialization function for uri scheme
fn deserialize_scheme<'de, D>(deserializer: D) -> Result<Option<http::uri::Scheme>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let str_val = String::deserialize(deserializer)?;
    Some(
        http::uri::Scheme::from_str(&str_val)
            .map_err(|e| serde::de::Error::custom(format!("Failed to parse uri: {e:?}"))),
    )
    .transpose()
}

impl EnvironmentBuilder {
    /// tries to build environment from given partial builder
    ///
    /// * `parent_env`: parent environment, if this is present then if any of the variables are missing in builder then it will take from parent
    fn build(self, parent_env: Option<&Environment>) -> Option<Environment> {
        let Self {
            scheme,
            host,
            port,
            prefix,
            headers: builder_headers,
            store: builder_key_store,
        } = self;
        let Some(template) = parent_env else {
            return Some(Environment {
                scheme: scheme?,
                host: host?,
                port,
                prefix: None,
                headers: HashMap::new(),
                store: builder_key_store,
            });
        };

        let mut key_store = template.store.clone();
        key_store.extend(builder_key_store);
        let mut headers = template.headers.clone();
        headers.extend(builder_headers);
        Some(Environment {
            scheme: scheme.unwrap_or(template.scheme.clone()),
            host: host.unwrap_or(template.host.clone()),
            port: port.or(template.port),
            prefix: prefix.or(template.prefix.clone()),
            headers,
            store: key_store,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubModule {
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    #[serde(rename = "environment")]
    environments: HashMap<String, EnvironmentBuilder>,
    #[serde(default)]
    #[serde(rename = "endpoint")]
    endpoints: HashMap<String, RawEndpoint>,
    #[serde(default)]
    submodules: HashMap<String, Self>,
}

impl SubModule {
    fn from_dir(path_ref: &impl AsRef<Path>) -> miette::Result<Self> {
        let mut path_buf: PathBuf = path_ref.as_ref().into();
        let submodules = path_buf
            .read_dir()
            .into_diagnostic()
            .wrap_err_with(||"Couldn't read directory {path_buf:?}")?
            .filter_map(|dir_entry_res| {
            let dir_entry = dir_entry_res.ok()?;
            if dir_entry.file_name() == "index.toml" {
                // index.toml will be handled separately
                return None;
            }
            match dir_entry.file_type() {
                Ok(ft) => {
                    if ft.is_dir() {
                        match SubModule::from_dir(&dir_entry.path()) {
                            Ok(sm) => {
                                let Some(mod_name) = dir_entry.file_name().to_str().map(|s| s.to_string()) else {
                                    warn!(mod_name=?dir_entry.file_name(), "Failed to convert module name to utf-8 String, currently only utf-8 strings are supported");
                                    return None
                                };
                                Some((mod_name, sm))
                            },
                            Err(e) => {
                                warn!(file=?dir_entry.path(), error=?e, "Failed to get submodule from directory, skipping");
                                None
                            }
                        }
                    } else if ft.is_file() {
                        match parse_file::<SubModule>(&dir_entry.path()) {
                            Ok(sm) => Some(sm),
                            Err(e) => {
                                warn!(file=?dir_entry.path(), error=?e, "Failed to get submodule, skipping");
                                None
                            }
                        }
                    } else {
                        warn!(file=?dir_entry.file_name(), "Currently {ft:?} is not supported");
                        None
                    }
                }
                Err(e) => {
                    warn!(file=?dir_entry.file_name(), "Failed to get file type for {:?}", e);
                    None
                }
            }
        });
        path_buf.push("index.toml");
        let module_content = std::fs::read_to_string(&path_buf)
            .into_diagnostic()
            .wrap_err_with(|| "Couldn't read file: {path_buf:?}")?;
        let mut module = toml::from_str::<Self>(&module_content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't deserialize file: {path_buf:?}"))?;
        module.submodules.extend(submodules);
        Ok(module)
    }

    #[tracing::instrument(skip(self, parent_env_list))]
    pub fn into_module(self, parent_env_list: &HashMap<String, Rc<Environment>>) -> Service {
        debug!("converting submodule: {self:?} to module with env {parent_env_list:?}");
        let SubModule {
            environments: sub_mod_environs,
            endpoints,
            submodules,
            alias,
            description,
        } = self;
        // get current module private environment list inheriting parent environments
        let mut environments = sub_mod_environs
            .into_iter()
            .filter_map(|(current_env_name, current_env)| {
                let parent_env =
                    parent_env_list
                        .iter()
                        .find_map(|(parent_env_name, parent_env)| {
                            if parent_env_name == &current_env_name {
                                Some(parent_env.as_ref())
                            } else {
                                None
                            }
                        });
                match current_env.build(parent_env) {
                    Some(e) => Some((current_env_name, Rc::new(e))),
                    None => {
                        warn!("Failed to construct environ, skipping");
                        None
                    }
                }
            })
            .collect::<HashMap<_, _>>();
        // add any missing environments which are in parent but are not in current mod
        parent_env_list.iter().for_each(|(penv_name, penv)| {
            if !environments.contains_key(penv_name) {
                environments.insert(penv_name.clone(), penv.clone());
            }
        });

        let submodules = submodules
            .into_iter()
            .map(|(name, sub_mod)| (name, sub_mod.into_module(&environments)))
            .collect::<HashMap<String, Service>>();
        Service {
            environments,
            endpoints,
            submodules,
            alias,
            description,
        }
    }
}

#[derive(Debug)]
pub struct Substituted;
#[derive(Debug)]
pub struct NotSubstituted;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndPoint<T> {
    pub description: Option<String>,
    pub alias: Option<String>,
    pub method: Method,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub params: Vec<(String, String)>,
    pub body: Option<Body>,
    pub pre_hook: Option<crate::hook::Hook>,
    pub post_hook: Option<crate::hook::Hook>,
    pub path: String,
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
    pub fn substitute(
        &self,
        config_store: &crate::store::Store,
        base_headers: &HashMap<String, String>,
    ) -> Result<EndPoint<Substituted>, subst::Error> {
        trace!("Constructing query by substing values from config_store");
        let key_val_store = config_store.deref();

        // substitute url
        let url_path = subst::substitute(&self.path, key_val_store)?;

        // substitute url
        let mut params = Vec::with_capacity(self.params.len());
        for (key, val) in &self.params {
            let key = subst::substitute(key, key_val_store)?;
            let val = subst::substitute(val, key_val_store)?;
            params.push((key, val))
        }

        // substitute headers
        let mut headers = HashMap::with_capacity(self.headers.len());
        for (key, value) in base_headers
            .iter()
            // only take keys from base which are not present in self headers
            .filter(|(key, _)| !self.headers.contains_key(key.as_str()))
            .chain(&self.headers)
        {
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
pub struct Body {
    pub kind: String,
    #[serde(flatten)]
    pub data: BodyData,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub enum BodyData {
    #[serde(rename = "data")]
    Inline(String),
    #[serde(rename = "file")]
    Path(std::path::PathBuf),
}
