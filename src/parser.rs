use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{collections::HashMap, rc::Rc};
use tracing::{debug, error, warn};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Given config is not supported")]
    UnSupportedVersion,
    #[error("Failed to read config file content")]
    CouldntReadFile(#[from] std::io::Error),
    #[error("Failed to deserialize config file")]
    InvalidConfigFile(#[from] toml::de::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum PopulateError {
    #[error("Failed to read content of service directory or file : {0:?}")]
    InvalidServiceDirectoryOrFile(#[from] std::io::Error),
    #[error("Unexpected file, expecting only toml files: {0:?}")]
    UnexpectedFile(PathBuf),
    #[error("Failed to parse file: {0:?}")]
    ParseError(#[from] toml::de::Error),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: Version,
    /// To distinguish different versions of identifiers
    project: String,
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

    pub fn populate(&self) -> Result<HashMap<String, ServiceModule>, PopulateError> {
        let services = self
            .api_directory
            .read_dir()?
            .filter_map(|file| match file {
                Ok(dir_entry) => {
                    let ft = match dir_entry.file_type() {
                        Ok(ft) => ft,
                        Err(e) => {
                            warn!(error= ?e, "Failed to get file type, skipping");
                            return None;
                        }
                    };
                    let res = if ft.is_file() {
                        ServiceModule::from_file(&dir_entry.path())
                    } else if ft.is_dir() {
                        let Some(module_name) = dir_entry.file_name().to_str().map(|name| name.to_string()) else {
                            warn!("service name is not valid utf-8, please change it utf-8 string");
                            return None
                        };
                        ServiceModule::from_dir(&dir_entry.path()).map(|sm| (module_name, sm))
                    } else {
                        warn!(file=?dir_entry.path(), "direntry is neither file or directory, its not handled yet skipping");
                        return None;

                    };
                    match res {
                            Ok(sm) => Some(sm),
                            Err(e) => {
                                warn!(file=?dir_entry.path(), "Failed to parse config file: {e:?}");
                                None
                            },
                        }
                }
                Err(e) => {
                    warn!(error=?e, "Failed to read entry of service directory");
                    None
                }
            }).collect::<HashMap<_, _>>();
        Ok(services)
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

#[derive(Debug, Deserialize)]
pub struct ServiceModule {
    #[serde(rename = "environment")]
    environments: Vec<Environment>,
    #[serde(default)]
    endpoints: Vec<EndPoint>,
    #[serde(default)]
    submodules: HashMap<String, SubModule>,
}

impl ServiceModule {
    fn from_file(path_ref: &impl AsRef<Path>) -> Result<(String, Self), PopulateError> {
        let path = path_ref.as_ref();
        if let None = path.extension().filter(|ext| *ext == "toml") {
            return Err(PopulateError::UnexpectedFile(path.into()));
        }
        let content = std::fs::read_to_string(path)?;
        let module_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(PopulateError::UnexpectedFile(path.into()))?
            .to_string();
        Ok((module_name, toml::from_str::<Self>(&content)?))
    }
    fn from_dir(path_ref: &impl AsRef<Path>) -> Result<Self, PopulateError> {
        let mut path_buf: PathBuf = path_ref.as_ref().into();
        let submodules = path_buf.read_dir()?.filter_map(|dir_entry_res| {
            let dir_entry = dir_entry_res.ok()?;
            if dir_entry.file_name() == "index.toml" {
                // index.toml will be handled separately
                return None;
            }
            match SubModule::from_file(&dir_entry.path()) {
                Ok(sm) => Some(sm),
                Err(e) => {
                    warn!(error=?e, "Failed to get submodule, skipping");
                    None
                }
            }
        });
        path_buf.push("index.toml");
        let module_content = std::fs::read_to_string(&path_buf)?;
        let mut module = toml::from_str::<Self>(&module_content)?;
        module.submodules.extend(submodules);
        Ok(module)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModuleSetError {}

#[derive(Debug)]
struct Module {
    environments: Vec<std::rc::Rc<Environment>>,
    endpoints: Vec<EndPoint>,
    submodules: HashMap<String, Self>,
}

impl Module {
    fn from_submodule(submodule: SubModule, parent_env_list: &[Rc<Environment>]) -> Self {
        let SubModule {
            environments: sub_mod_environs,
            endpoints,
            submodules,
        } = submodule;
        let environments = sub_mod_environs
            .into_iter()
            .filter_map(|environ| {
                let parent_env = parent_env_list.iter().find_map(|env| {
                    if env.name == environ.name {
                        Some(env.as_ref())
                    } else {
                        None
                    }
                });
                match environ.build(parent_env) {
                    Some(e) => Some(Rc::new(e)),
                    None => {
                        warn!(
                            environ = environ.name,
                            "Failed to construct environ, skipping"
                        );
                        None
                    }
                }
            })
            .collect::<Vec<_>>();
        Self {
            environments,
            endpoints,
            submodules: todo!(),
        }
    }
}

#[derive(Debug)]
struct ModuleSet(HashMap<String, Module>);

impl TryFrom<HashMap<String, ServiceModule>> for ModuleSet {
    type Error = ModuleSetError;

    fn try_from(value: HashMap<String, ServiceModule>) -> Result<Self, Self::Error> {
        let inner = value
            .into_iter()
            .map(|(name, service_mod)| {
                let module = {
                    let ServiceModule {
                        environments,
                        endpoints,
                        submodules,
                    } = service_mod;
                    let environments = environments
                        .into_iter()
                        .map(|environ| Rc::new(environ))
                        .collect();

                    let submodules = submodules
                        .into_iter()
                        .map(|(name, sub_mod)| {
                            let SubModule {
                                environments,
                                endpoints,
                                submodules,
                            } = sub_mod;
                            let module = Module {
                                environments,
                                endpoints,
                                submodules,
                            };
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
        Ok(Self(inner))
    }
}

/// Used incase of environments in submodules
/// these will be used to override environment configurations defined in service-module
#[derive(Debug, Deserialize)]
struct EnvironmentBuilder {
    name: String,
    scheme: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    #[serde(default)]
    store: HashMap<String, String>,
}

impl EnvironmentBuilder {
    fn build(self, template_opt: Option<&Environment>) -> Option<Environment> {
        let Self {
            name,
            scheme,
            host,
            port,
            store: builder_key_store,
        } = self;
        let Some(template) = template_opt else {
            return Some(Environment {
                name,
                scheme: scheme?,
                host: host?,
                port,
                store: builder_key_store,
            });
        };
        if name != template.name {
            return None;
        }

        let mut key_store = template.store.clone();
        key_store.extend(builder_key_store.into_iter());
        Some(Environment {
            name,
            scheme: scheme.unwrap_or(template.scheme.clone()),
            host: host.unwrap_or(template.host.clone()),
            port: port.or(template.port),
            store: key_store,
        })
    }
}

#[derive(Debug, Deserialize)]
struct SubModule {
    #[serde(default)]
    #[serde(rename = "environment")]
    environments: Vec<EnvironmentBuilder>,
    #[serde(rename = "endpoint")]
    endpoints: Vec<EndPoint>,
    #[serde(default)]
    submodules: Vec<Self>,
}

impl SubModule {
    fn from_file(path_ref: &impl AsRef<Path>) -> Result<(String, Self), PopulateError> {
        let path = path_ref.as_ref();
        if let None = path.extension().filter(|ext| *ext == "toml") {
            return Err(PopulateError::UnexpectedFile(path.into()));
        }
        let content = std::fs::read_to_string(path)?;
        let module_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(PopulateError::UnexpectedFile(path.into()))?
            .to_string();
        Ok((module_name, toml::from_str::<Self>(&content)?))
    }

    /// promotes itself to service module, by referencing all of its environments
    fn promote(&self, parent_environments: &[Environment]) -> Result<ServiceModule, ()> {
        todo!()
    }
}
