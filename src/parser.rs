use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    #[error("Failed to read content of service directory")]
    InvalidServiceDirectory(#[from] std::io::Error),
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
                    let Some(module_name) = dir_entry.file_name().to_str().map(|name| name.to_string()) else {
                        warn!("service name is not valid utf-8, please change it utf-8 string");
                        return None
                    };
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
                        ServiceModule::from_dir(&dir_entry.path())
                    } else {
                        warn!(file=?module_name, "direntry is neither file or directory, its not handled yet skipping");
                        return None;

                    };
                    match res {
                            Ok(sm) => Some((module_name, sm)),
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
    headers: Vec<(String, String)>,
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
    environments: Vec<Environment>,
    #[serde(default)]
    endpoints: Vec<EndPoint>,
    #[serde(default)]
    submodules: HashMap<String, SubModule>,
}

impl ServiceModule {
    fn from_file(path_ref: &impl AsRef<Path>) -> Result<Self, ()> {
        let path = path_ref.as_ref();
        if let None = path.extension().filter(|ext| *ext == "toml") {
            warn!(file_path= ?path, "Non toml file ignoring");
            todo!()
        }
        todo!()
    }
    fn from_dir(path: &impl AsRef<Path>) -> Result<Self, ()> {
        todo!()
    }
}

/// Used incase of environments in submodules
/// these will be used to override environment configurations defined in service-module
#[derive(Debug, Deserialize)]
struct EnvironmentBuilder {
    name: Option<String>,
    scheme: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    #[serde(default)]
    store: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct SubModule {
    #[serde(default)]
    environments: Vec<EnvironmentBuilder>,
    endpoints: Vec<EndPoint>,
    #[serde(default)]
    submodules: Vec<Self>,
}
