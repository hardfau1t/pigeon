use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, error, warn};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: Version,
    /// To distinguish different versions of identifiers
    project: String,
    /// can be overriden with environment variable
    default_environment: String,
    /// where to find for api's
    api_directory: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Given config is not supported")]
    UnSupportedVersion,
    #[error("Failed to read config file content")]
    CouldntReadFile(#[from] std::io::Error),
    #[error("Failed to deserialize config file")]
    InvalidConfigFile(#[from] toml::de::Error),
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

    pub fn populate(&self) {

    }
}
