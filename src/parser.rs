use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, error, trace, warn};
use yansi::Paint;

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

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum GroupInfo {
    Http {
        #[serde(default, rename = "query")]
        queries: HashMap<String, agent::http::Query>,
        #[serde(default, rename = "environment")]
        environments: HashMap<String, agent::http::Environment>,
    },
    Generic,
}

impl GroupInfo {
    fn find_query(&self, name: &str) -> Option<QuerySearchResult> {
        match self {
            GroupInfo::Http {
                queries,
                environments,
            } => {
                let q = queries.get(name)?;
                Some(QuerySearchResult::Http {
                    environments: environments.clone(),
                    query: q.clone(),
                })
            }
            GroupInfo::Generic => None,
        }
    }
    fn format_print(&self, my_name: &Option<impl std::fmt::Debug>) {
        match self {
            GroupInfo::Http { queries, .. } => {
                if !queries.is_empty() {
                    let mut subq_table = default_table_structure();
                    if let Some(name) = my_name {
                        eprintln!("{:?} Sub Queries", name.bold().green().bright());
                    } else {
                        eprintln!("Sub Queries");
                    }
                    let query_headers = agent::http::Query::headers();
                    let headers = ["name"].iter().chain(query_headers);
                    subq_table.set_header(headers);

                    let query_rows = queries.iter().map(|(name, query)| {
                        [name.clone()]
                            .into_iter()
                            .chain(query.into_row().into_iter())
                    });
                    subq_table.add_rows(query_rows);
                    eprintln!("{subq_table}");
                }
            }
            GroupInfo::Generic => todo!(),
        }
    }
}

impl Default for GroupInfo {
    fn default() -> Self {
        Self::Generic
    }
}

#[derive(Debug, Deserialize, Default, PartialEq, Eq, Clone, Serialize)]
pub struct Group {
    #[serde(default, rename = "group")]
    sub_groups: HashMap<String, Group>,
    // TODO: This will cause error if the file doesn't have `type`, eventhough default it is generic
    #[serde(flatten)]
    info: GroupInfo,
}

impl Group {
    pub fn from_dir(path: impl AsRef<std::path::Path>) -> miette::Result<Self> {
        trace!("reading dir: {:?}", path.as_ref());

        let mut sub_dir_entries = std::fs::read_dir(path.as_ref())
            .into_diagnostic()
            .wrap_err("Couldn't read directory group")?
            .collect::<Result<Vec<_>, _>>()
            .into_diagnostic()
            .wrap_err_with(|| format!("Invalid file entry: {:?}", path.as_ref()))?;

        let mut group = sub_dir_entries
            .iter()
            .position(|e| e.file_name() == constants::GROUP_FILE_NAME)
            .map(|file_index| sub_dir_entries.swap_remove(file_index).path()) // this will not panic because it is taken from position
            .map(|group_path| Self::from_file(group_path))
            .transpose()?
            .unwrap_or_default(); // create generic group

        let subgroups = sub_dir_entries
            .into_iter()
            .filter(|entry| {
                if !entry.path().ends_with("toml") {
                    true
                } else {
                    warn!("ignoring non toml file: {:?}", entry.path());
                    false
                }
            })
            .map(|file| {
                let name = file
                    .path()
                    .file_stem()
                    .unwrap_or(file.file_name().as_os_str())
                    .to_str()
                    .ok_or(miette::miette!(
                        "Invalid utf-8 file name: {:?}",
                        file.file_name()
                    ))?
                    .to_string();
                let subg = Self::from_path(file.path())?;
                miette::Result::Ok((name, subg))
            })
            .collect::<Result<HashMap<_, _>, miette::Error>>()
            .wrap_err("Couldn't read group")?;

        group.sub_groups.extend(subgroups);

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

    /// find given query/group from the tree
    pub fn find<'a, 's>(
        &'a self,
        search_path: &'s [impl AsRef<str>],
    ) -> Option<SearchResult<'a, 's>> {
        let Some((key, rest)) = search_path.split_first() else {
            debug!("empty search query, showing top level groups");
            return Some(SearchResult {
                name: None,
                sub_query: None,
                sub_group: Some(GroupSearchResult {
                    queries: &self.info,
                    sub_groups: &self.sub_groups,
                }),
            });
        };

        if rest.is_empty() {
            trace!("finding group/query {}", key.as_ref());
            let sub_query = self.info.find_query(key.as_ref());
            let sub_group = self
                .sub_groups
                .get(key.as_ref())
                .map(|g| GroupSearchResult::from(g));

            if sub_query.is_none() && sub_group.is_none() {
                warn!("no such group/query: {}", key.as_ref());
                return None;
            }
            Some(SearchResult {
                name: Some(key.as_ref()),
                sub_query,
                sub_group,
            })
        } else {
            trace!("finding group with name {}", key.as_ref());
            // if there are no subgroup but query still has params then search is invalid so return None
            let sub_group = self.sub_groups.get(key.as_ref())?;

            // if one of the subgroup finds None then popout that None
            let mut qset = sub_group.find(rest)?;
            if let Some(ref mut qresult) = qset.sub_query {
                qresult.apply_group_env(&self.info);
            }
            Some(qset)
        }
    }

    fn headers() -> &'static [&'static str] {
        &["kind"]
    }
    fn into_row(&self) -> Vec<String> {
        match &self.info {
            GroupInfo::Http { .. } => {
                vec!["http".to_string()]
            }
            GroupInfo::Generic => vec!["generic".to_string()],
        }
    }
}

#[derive(Debug, Serialize)]
pub enum QuerySearchResult {
    Http {
        environments: HashMap<String, agent::http::Environment>,
        query: agent::http::Query,
    },
}

impl QuerySearchResult {
    fn apply_group_env(&mut self, group: &GroupInfo) {
        match (self, group) {
            (
                QuerySearchResult::Http { environments, .. },
                GroupInfo::Http {
                    environments: parent_env,
                    ..
                },
            ) => {
                parent_env.iter().for_each(|(key, parent_env)| {
                    environments
                        .entry(key.to_owned())
                        .and_modify(|cur_env| cur_env.apply(parent_env)) // if the current env is not empty then just apply missing fields from parent env
                        .or_insert_with(|| parent_env.clone()); // there is no such env so just copy parent env
                });
            }
            (_, GroupInfo::Generic) => debug!("parent group is generic group, ignoring"),
        }
    }

    fn format_print(&self) {
        match self {
            QuerySearchResult::Http {
                environments,
                query,
            } => {
                let formatted_query = query.to_string();
                eprintln!("{formatted_query}");

                eprintln!("Environments:");
                let mut table = default_table_structure();
                let env_headers = agent::http::Environment::headers();
                let headers = ["name"].iter().chain(env_headers.into_iter());

                table.set_header(headers);
                let rows = environments
                    .iter()
                    .map(|(name, e)| [name.clone()].into_iter().chain(e.into_row().into_iter()));
                table.add_rows(rows);
                eprintln!("{table}");
            }
        }
    }
    pub async fn exec_with_args(
        self,
        args: &crate::Arguments,
        env: &str,
        store: &crate::store::Store,
    ) -> miette::Result<Option<QueryResponse>> {
        match self {
            QuerySearchResult::Http {
                mut environments,
                query,
            } => {
                let Some(env) = environments.remove(env) else {
                    let available_env: Vec<_> = environments.keys().collect();
                    miette::bail!(
                        help = format!("set {}", crate::constants::KEY_CURRENT_ENVIRONMENT),
                        "Couldn't find environment {env}, available are {available_env:?}"
                    )
                };
                query.execute(env, store, args).await
            }
        }
    }
}

pub type QueryResponse = Vec<u8>;

/// set of environments and query result
/// search result can be another group or a query
#[derive(Debug, Serialize)]
pub struct GroupSearchResult<'g> {
    /// search result can optionally contain a group
    sub_groups: &'g HashMap<String, Group>,
    queries: &'g GroupInfo,
}

impl<'g> From<&'g Group> for GroupSearchResult<'g> {
    fn from(value: &'g Group) -> Self {
        Self {
            sub_groups: &value.sub_groups,
            queries: &value.info,
        }
    }
}

impl<'g> GroupSearchResult<'g> {
    fn format_print(&self) {
        if !self.sub_groups.is_empty() {
            let mut subg_table = default_table_structure();

            let headers = ["name"].iter().chain(Group::headers().iter());
            subg_table.set_header(headers);

            let subg_rows = self
                .sub_groups
                .iter()
                .map(|(name, subg)| [name.clone()].into_iter().chain(subg.into_row()));
            subg_table.add_rows(subg_rows);
            eprintln!("{subg_table}");
        }
    }
}

fn default_table_structure() -> comfy_table::Table {
    let mut table = comfy_table::Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
    table
}

#[derive(Debug, Serialize)]
pub struct SearchResult<'g, 'i> {
    pub name: Option<&'i str>,
    pub sub_query: Option<QuerySearchResult>,
    pub sub_group: Option<GroupSearchResult<'g>>,
}

impl<'g, 'i> SearchResult<'g, 'i> {
    pub fn format_print(&'i self) {
        if let Some(query) = &self.sub_query {
            let name = self.name.expect("name cannot be None for matched query");
            eprintln!("Query: \"{}\"", name.green().bold().bright());
            query.format_print();
        };
        if let Some(group) = &self.sub_group {
            if !group.sub_groups.is_empty() {
                if let Some(name) = self.name {
                    eprintln!("\"{}\" Sub Groups", name.green().bold().bright());
                } else {
                    eprintln!("Sub Groups");
                }
                group.format_print()
            }
            group.queries.format_print(&self.name);
        }
    }

    pub fn json_print(&self) -> miette::Result<()> {
        let stdout = std::io::stdout();
        serde_json::to_writer(stdout, self)
            .into_diagnostic()
            .wrap_err("Couldn't write serialized Search results")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_deserialize_empty_generic() {
        let s = "";
        let g: Group = toml::from_str(s).unwrap();
        assert_eq!(
            g,
            Group {
                sub_groups: HashMap::new(),
                info: GroupInfo::Generic
            }
        )
    }
    #[test]
    fn group_deserialize_empty_http() {
        let s = "type = \"http\"";
        let g: Group = toml::from_str(s).unwrap();
        assert_eq!(
            g,
            Group {
                sub_groups: HashMap::new(),
                info: GroupInfo::Http {
                    queries: HashMap::new(),
                    environments: HashMap::new()
                }
            }
        )
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
