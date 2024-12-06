use std::collections::HashMap;

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::{agent, constants};

pub trait Handler<'a> {
    type Environment: Deserialize<'a>;
    type Query: Deserialize<'a>;
    type Error;
    type Output: Serialize;
    fn handle(
        &self,
        env: Self::Environment,
        query: Self::Query,
    ) -> Result<Self::Output, Self::Error>;
}

#[derive(Debug, Deserialize, Default, PartialEq, Eq)]
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
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq)]
enum Environment {
    Rest(agent::http2::RestEnvironment),
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq)]
enum Query {
    Rest(agent::http2::Query),
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
