//! used to store environment variables for pigeon
//! Why not use shell environment variables, because it is hard to use environment variables when you are a independent binary

use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use miette::Diagnostic;
use tracing::{debug, error, instrument, trace, warn};

/// per environment config store
type EnvStore = HashMap<String, HashMap<String, String>>;

fn read_env_store(config_path: &impl AsRef<std::path::Path>) -> Result<EnvStore, StoreError> {
    let config_path = config_path.as_ref();
    match std::fs::read_to_string(config_path) {
        Ok(content) => toml::from_str::<EnvStore>(&content).map_err(|e| {
            error!(
                "Deserialization of cached config failed: {e} Try after removing {config_path:?}"
            );
            StoreError::CorruptedPackage
        }),
        Err(e) => {
            warn!("Couldn't read store file {:?}: {e}", config_path);
            Ok(HashMap::new())
        }
    }
}

/// Main interface for managing variables
#[derive(Debug)]
pub struct Store {
    config: HashMap<String, String>,
    current_env: String,
    persistent: bool,
    package: std::path::PathBuf,
    used_with_env: bool,
}

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum StoreError {
    #[error("XdgCache path is missing from the system")]
    XdgCacheMissing,
    #[error("content of config file is invalid")]
    CorruptedPackage,
    #[error("store path is not directory, or failed to create directory")]
    InvalidPath,
    #[error("Couldn't find environment")]
    MissingEnvironment(#[from] std::env::VarError),
}

impl Store {
    /// open keystore for given package/project
    #[instrument(skip(package))]
    pub fn open(
        package: &impl AsRef<std::path::Path>,
        current_env: String,
    ) -> Result<Self, StoreError> {
        trace!("Reading config store");

        let mut config_path = dirs::cache_dir().ok_or(StoreError::XdgCacheMissing)?;
        config_path.push(env!("CARGO_PKG_NAME"));

        // check if the store directory present if not create new
        if config_path.exists() {
            if !config_path.is_dir() {
                warn!("{config_path:?} is not directory, try to remove it and try again");
                return Err(StoreError::InvalidPath);
            }
            // directory doesn't exists so if creation success then ok else error out
        } else if let Err(e) = std::fs::create_dir(&config_path) {
            debug!("Failed to create config store directory: {e}");
            return Err(StoreError::InvalidPath);
        };

        config_path.push(package);
        debug!("config store path: {config_path:?}");
        let mut pairs = read_env_store(&config_path)?;
        Ok(Self {
            config: pairs.remove(&current_env).unwrap_or_default(),
            current_env,
            persistent: true,
            package: config_path,
            used_with_env: false,
        })
    }

    /// open the store and overwrite values with environment variables and insert new
    #[instrument(skip(package))]
    pub fn with_env(
        package: &impl AsRef<std::path::Path>,
        current_env: String,
    ) -> Result<Self, StoreError> {
        trace!("Creating store with environment");
        let mut store = Self::open(package, current_env)?;
        store.config.extend(std::env::vars());
        store.used_with_env = true;
        Ok(store)
    }

    /// make changes permanent
    /// by default all changes are permanent and store in cache
    /// set as false to make it temporary
    pub fn persistent(&mut self, is_persistent: bool) {
        trace!(
            "making configurations: {}",
            if is_persistent {
                "persistent"
            } else {
                "not persistent"
            }
        );
        self.persistent = is_persistent;
    }
}

impl Deref for Store {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl DerefMut for Store {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.config
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        trace!("writing configurations back to file: {:?}", self.package);
        if self.used_with_env {
            std::env::vars().for_each(|(key, env_val)| {
                if self.config.get(&key).is_some_and(|val| val == &env_val) {
                    self.config.remove(&key);
                }
            })
        }
        let env_store = self.config.drain().collect();

        let mut store = match read_env_store(&self.package) {
            Ok(store) => store,
            Err(e) => {
                warn!("Couldn't write back store variables: {e}");
                return;
            }
        };
        store.insert(self.current_env.clone(), env_store);

        let Ok(serialized_config) = toml::to_string(&store) else {
            warn!("Failed to serialize the config store, not writing to disk");
            return;
        };
        if let Err(e) = std::fs::write(&self.package, serialized_config) {
            warn!(
                "Session store write to disk failed for {:?}: {e}",
                &self.package
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    use super::*;

    #[traced_test]
    #[test]
    fn store_and_get() {
        let mut store = Store::open(&"test_package", "dev".to_string()).unwrap();
        store.persistent(false);
        let key = "key1".to_string();
        let value = "value1".to_string();

        store.insert(key.clone(), value.clone());
        assert_eq!(store.get(&key), Some(&value));
    }

    #[traced_test]
    #[test]
    fn store_and_get_persistent() {
        let key = "key1".to_string();
        let value = "value1".to_string();
        {
            let mut store = Store::open(&"test_package", "dev".to_string()).unwrap();
            store.insert(key.clone(), value.clone());
        }

        let mut new_store = Store::open(&"test_package", "dev".to_string()).unwrap();
        new_store.persistent(false);
        assert_eq!(new_store.get(&key), Some(&value));
    }
}
