use std::collections::HashMap;

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, trace, warn};

use crate::{agent, constants};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: semver::Version,
    /// To distinguish different versions of identifiers
    pub project: String,
    /// where to find for api's
    pub api_directory: std::path::PathBuf,
}

impl Config {
    /// read the config file and do the version check and parse the config file
    pub fn open(file_path: &impl AsRef<std::path::Path>) -> miette::Result<Self> {
        let current_package_version =
            semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("cargo pkg is not semver?");
        debug!(version=?current_package_version, "current binary version");
        let config = toml::from_str::<Self>(
            &std::fs::read_to_string(file_path.as_ref())
                .into_diagnostic()
                .wrap_err_with(|| format!("Couldn't read {:?}", file_path.as_ref()))?,
        )
        .into_diagnostic()
        .wrap_err("Couldn't deserialize config file")?;

        if current_package_version.major != config.version.major {
            error!(binary_version=?current_package_version, config_version=?config.version, "major versions of binary and config are not matching");
            miette::bail!("Unsupported config set")
        }

        if current_package_version.major == 0
            && current_package_version.minor != config.version.minor
        {
            // 0 major version is beta stage so breaking changes are expected at minor versions
            error!(binary_version=?current_package_version, config_version=?config.version, "binary version is beta version and minor versions are not matching");
            miette::bail!("Unsupported config set")
        }
        if current_package_version < config.version {
            warn!(binary_version=?current_package_version, config_version=?config.version, "binary version is smaller than config, things may not work as expected");
        }
        Ok(config)
    }
}

pub trait Handler<'a> {
    type Environment;
    type Query;
    type Error;
    type Output;
    fn handle(
        &self,
        env: Self::Environment,
        query: Self::Query,
    ) -> Result<Self::Output, Self::Error>;
}

#[derive(Debug, Deserialize, Default, PartialEq, Eq, Clone, Serialize)]
pub struct Group {
    #[serde(default)]
    environment: HashMap<String, Environment>,
    #[serde(default)]
    group: HashMap<String, Group>,
    #[serde(default)]
    query: HashMap<String, Query>,
}

impl Group {
    pub fn from_dir(path: impl AsRef<std::path::Path>) -> miette::Result<Self> {
        trace!("reading dir: {:?}", path.as_ref());

        let mut group_entries = std::fs::read_dir(path.as_ref())
            .into_diagnostic()
            .wrap_err("Couldn't read directory group")?
            .collect::<Result<Vec<_>, _>>()
            .into_diagnostic()
            .wrap_err_with(|| format!("Invalid file entry: {:?}", path.as_ref()))?;

        let mut group = group_entries
            .iter()
            .position(|e| e.file_name() == constants::GROUP_FILE_NAME)
            .map(|file_index| group_entries.swap_remove(file_index).path()) // this will not panic because it is taken from position
            .map(|group_path| Self::from_file(group_path))
            .transpose()?
            .unwrap_or_default();

        let subgroups = group_entries
            .into_iter()
            .map(|file| {
                file.file_name()
                    .to_str()
                    .ok_or(miette::miette!(
                        "Invalid utf-8 file name: {:?}",
                        file.file_name()
                    ))
                    .and_then(|name| Self::from_path(file.path()).map(|e| (name.to_string(), e)))
            })
            .collect::<Result<HashMap<_, _>, _>>()
            .wrap_err("Couldn't read group")?;

        group.group.extend(subgroups);

        Ok(group)
    }

    /// path is a file and read all the environment and queries from that file
    fn from_file(path: impl AsRef<std::path::Path>) -> miette::Result<Self> {
        trace!("reading file: {:?}", path.as_ref());

        let file_content = std::fs::read_to_string(path.as_ref())
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't read file: {:?}", path.as_ref()))?;

        toml::from_str(file_content.as_str())
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't deserialize {:?}", path.as_ref()))
    }

    /// unsure about the path, it could be directory in which case it doesn't contains any environments or queries
    /// or file which can optionally have these
    pub fn from_path(path: impl AsRef<std::path::Path>) -> miette::Result<Self> {
        let path = path.as_ref();
        if path.is_dir() {
            Self::from_dir(path)
        } else if path.is_file() {
            Self::from_file(path)
        } else {
            miette::bail!("couldn't access {path:?}, may be you don't have permission or its a broken symlink")
        }
    }

    /// find given query from the tree
    pub fn find<'a>(&'a self, search_path: &[impl AsRef<str>]) -> Option<SearchResult<'a>> {
        let Some((key, rest)) = search_path.split_first() else {
            return Some(SearchResult {
                sub_query: None,
                sub_group: Some(GroupSearchResult {
                    queries: &self.query,
                    groups: &self.group,
                }),
            });
        };

        if rest.is_empty() {
            let sub_query = self.query.get(key.as_ref()).map(|q| QuerySearchResult {
                query: q,
                environments: self.environment.clone(),
            });
            let sub_group = Some(GroupSearchResult {
                queries: &self.query,
                groups: &self.group,
            });
            Some(SearchResult {
                sub_query,
                sub_group,
            })
        } else {
            // if there are no subgroup but query still has params then search is invalid so return None
            let sub_group = self.group.get(key.as_ref())?;

            // if one of the subgroup finds None then popout that None
            let mut qset = sub_group.find(rest)?;
            if let Some(q) = &mut qset.sub_query {
                // if the search result has query then append my environments also to list of environments so that later it can be squashed
                q.environments.extend(self.environment.clone());
            };
            Some(qset)
        }
    }
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
enum Environment {
    Rest(agent::http2::RestEnvironment),
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone, Serialize)]
enum Query {
    Rest(agent::http2::Query),
}

#[derive(Debug, Serialize)]
pub struct QuerySearchResult<'g> {
    environments: HashMap<String, Environment>,
    query: &'g Query,
}

/// set of environments and query result
/// search result can be another group or a query
#[derive(Debug, Serialize)]
pub struct GroupSearchResult<'g> {
    /// search result can optionally contain a group
    groups: &'g HashMap<String, Group>,
    queries: &'g HashMap<String, Query>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult<'g> {
    sub_query: Option<QuerySearchResult<'g>>,
    sub_group: Option<GroupSearchResult<'g>>,
}

impl<'g> SearchResult<'g> {
    pub fn format_print(&self) -> miette::Result<()> {
        todo!()
    }

    pub fn json_print(&self) -> miette::Result<()> {
        let stdout = std::io::stdout();
        serde_json::to_writer(stdout, self)
            .into_diagnostic()
            .wrap_err("Couldn't write serialized Search results")
    }
}

/*
 *  g11/
 *      g21/
 *          e31 q41
 *              e41 q51
 *              g41 q52
 *      g22/
 *          g32
 *
 *  group:
 *      transparent
 *      can be a file or group
 *  environment:
 *      - transparent
 *      - host
 *      - port
* query:
 *
 */
