use std::{collections::HashMap, ops::Deref, str::FromStr};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace};
use yansi::Paint;

//NOTE: if any new field is added to this, update apply method
/// HTTP environment
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    scheme: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    prefix: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    store: HashMap<String, String>,
    #[serde(default)]
    args: Vec<(String, String)>,
}

impl Environment {
    pub fn apply(&mut self, other: &Self) {
        if let Some(parent_host) = &other.host {
            self.host.get_or_insert_with(|| parent_host.clone());
        }
        if let Some(parent_port) = &other.port {
            self.port.get_or_insert_with(|| parent_port.clone());
        }
        if let Some(parent_prefix) = &other.prefix {
            self.prefix.get_or_insert_with(|| parent_prefix.clone());
        }
        if !other.headers.is_empty() {
            self.headers.extend(other.headers.clone());
        }
        if !other.store.is_empty() {
            self.store.extend(other.store.clone());
        }
        if !other.args.is_empty() {
            self.args.extend(other.args.clone());
        }
    }

    /// Gives columns presennt in this structure
    /// this is used for formatting
    pub fn headers() -> &'static [&'static str] {
        &["scheme", "host", "port"]
    }

    pub fn into_row(&self) -> Vec<String> {
        let scheme = self.scheme.clone().unwrap_or_default();
        let host = self.host.clone().unwrap_or_default();
        let port = self.port.clone().map(|p| p.to_string()).unwrap_or_default();
        vec![scheme, host, port]
    }
}

fn default_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(30)
}

/// Marker Struct for indicating that Query is Substituted
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Substituted;
/// Marker Struct for indicating that Query is not built
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct NotSubstituted;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
enum HttpVersion {
    Http09,
    Http10,
    Http11,
    Http2,
    Http3,
}

impl Default for HttpVersion {
    fn default() -> Self {
        Self::Http11
    }
}

impl From<HttpVersion> for reqwest::Version {
    fn from(value: HttpVersion) -> Self {
        match value {
            HttpVersion::Http09 => reqwest::Version::HTTP_09,
            HttpVersion::Http10 => reqwest::Version::HTTP_10,
            HttpVersion::Http11 => reqwest::Version::HTTP_11,
            HttpVersion::Http2 => reqwest::Version::HTTP_2,
            HttpVersion::Http3 => reqwest::Version::HTTP_3,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    description: Option<String>,
    path: String,
    // TODO: method: http::method::Method,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    args: Vec<(String, String)>,
    #[serde(default = "default_timeout")]
    timeout: std::time::Duration,
    #[serde(default)]
    version: HttpVersion,
    pre_hook: Option<crate::hook::Hook>,
    post_hook: Option<crate::hook::Hook>,
    body: Body,
}

impl PartialEq for Query {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.method == other.method
            && self.headers == other.headers
            && self.args == other.args
    }
}

impl Eq for Query {}

impl std::fmt::Display for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        static KEY_STYLE: yansi::Style = yansi::Color::Yellow.bold();
        if let Some(description) = &self.description {
            writeln!(f, "{}: {}", "description".paint(KEY_STYLE), description)?;
        }
        writeln!(f, "{}: {}", "method".paint(KEY_STYLE), self.method)?;
        writeln!(f, "{}: {}", "path".paint(KEY_STYLE), self.path)
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
enum Body {
    ApplicationJson(Content<String>),
    Raw {
        content_type: String,
        data: Content<Vec<u8>>,
    },
    RawText {
        content_type: String,
        data: Content<String>,
    },
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum Content<T> {
    File(std::path::PathBuf),
    Inline(T),
}

impl Query {
    /// Gives columns presennt in this structure
    /// this is used for formatting
    pub fn headers() -> &'static [&'static str] {
        &["method", "path"]
    }

    /// gives vec of cells, used for format printing queries
    pub fn into_row(&self) -> Vec<String> {
        vec![self.method.clone(), self.path.clone()]
    }

    pub async fn execute(
        mut self,
        environ: Environment,
        store: &crate::store::Store,
        args: &crate::Arguments,
    ) -> miette::Result<Option<crate::parser::QueryResponse>> {
        trace!("Merging Query wit env");
        let Environment {
            scheme,
            host,
            port,
            prefix: env_prefix,
            mut headers,
            store: env_store,
            mut args,
        } = environ;
        let host = host.ok_or(miette::miette!("Host is empty"))?;
        let scheme = scheme.ok_or(miette::miette!("Scheme is empty"))?;
        headers.extend(self.headers);
        self.headers = headers;
        args.extend(self.args);
        self.args = args;

        let url_str = if let Some(port) = port {
            format!("{scheme}://{host}:{port}",)
        } else {
            format!("{scheme}://{host}")
        };
        debug!(url = url_str, "Constructed Base Url");

        let url = reqwest::Url::parse(&url_str)
            .into_diagnostic()
            .wrap_err("Couldn't parse given url")?;
        let base_url = if let Some(prefix) = env_prefix {
            url.join(&prefix)
                .into_diagnostic()
                .wrap_err_with(|| format!("Couldn't append environment prefix: {prefix}"))?
        } else {
            url
        }
        .join(&self.path)
        .into_diagnostic()
        .wrap_err_with(|| format!("Couldn't append path {}", self.path))?;

        debug!(url = ?base_url, "Costructed base Url");
        let mut local_store = store.deref().clone();
        local_store.extend(env_store);

        let pre_hook = self.pre_hook.take();
        let post_hook = self.post_hook.take();
        let substituted_query = self
            .substitute(local_store)
            .into_diagnostic()
            .wrap_err("Couldn't substitute Query request")?;

        let q = if let Some(pre_hook) = pre_hook {
            let args: &[&str] = &[todo!()];
            pre_hook.run(&substituted_query, args)?
        } else {
            substituted_query
        };

        let client = todo!();

        let request = q
            .prepare(base_url, &client)
            .wrap_err("Couldn't construct Query")?;

        let response = client
            .execute(request)
            .await
            .into_diagnostic()
            .wrap_err("Request failed")?;

        // convert response so that it can be sent to post hook
        let response = Response::read_response(response)
            .await
            .wrap_err("Couldn't read response")?;

        let response = if let Some(post_hook) = post_hook {
            let args: &[&str] = &[todo!()];
            post_hook
                .run(&response, args)
                .wrap_err("Failed to run post hook")?
        } else {
            response
        };

        todo!()
    }

    fn substitute(self, vars: HashMap<String, String>) -> Result<MergedQuery, subst::Error> {
        let path = subst::substitute(&self.path, &vars)?;
        let method = subst::substitute(&self.method, &vars)?;

        let headers = self
            .headers
            .into_iter()
            .map(|(key, value)| {
                let key = subst::substitute(&key, &vars)?;
                let val = subst::substitute(&value, &vars)?;
                Ok((key, val))
            })
            .collect::<Result<_, subst::Error>>()?;

        let args = self
            .args
            .into_iter()
            .map(|(key, value)| {
                let key = subst::substitute(&key, &vars)?;
                let val = subst::substitute(&value, &vars)?;
                Ok((key, val))
            })
            .collect::<Result<_, subst::Error>>()?;

        Ok(MergedQuery {
            path,
            headers,
            args,
            method,
            timeout: self.timeout,
            version: self.version,
            body: todo!(),
        })
    }
}

/// Generated after merging Query and environment
#[derive(Debug, Deserialize, Serialize)]
struct MergedQuery {
    path: String,
    headers: HashMap<String, String>,
    args: Vec<(String, String)>,
    timeout: std::time::Duration,
    version: HttpVersion,
    body: Body,
    method: String,
}

impl MergedQuery {
    fn prepare(
        self,
        base_url: reqwest::Url,
        client: &reqwest::Client,
    ) -> miette::Result<reqwest::Request> {
        let url = base_url
            .join(&self.path)
            .into_diagnostic()
            .wrap_err("Couldn't construct url")?;
        let method = reqwest::Method::from_str(&self.method)
            .into_diagnostic()
            .wrap_err_with(|| format!("invalid method: {}", self.method))?;

        let headers = (&self.headers)
            .try_into()
            .into_diagnostic()
            .wrap_err("Invalid headers")?;
        // TODO: add basic auth and bearer auth
        // TODO: support for multipart
        // TODO: support for form
        client
            .request(method, url)
            .headers(headers)
            .timeout(self.timeout)
            .version(self.version.into())
            .build()
            .into_diagnostic()
            .wrap_err("Couldn't build request")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Response {}

impl Response {
    async fn read_response(response: reqwest::Response) -> miette::Result<Self> {
        todo!()
    }
}
