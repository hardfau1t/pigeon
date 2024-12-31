use core::str;
use std::{collections::HashMap, io::Read, ops::DerefMut, str::FromStr};

use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace, warn};
use yansi::Paint;

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

fn default_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(30)
}

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
            self.port.get_or_insert(*parent_port);
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

    pub fn to_row(&self) -> Vec<String> {
        let scheme = self.scheme.clone().unwrap_or_default();
        let host = self.host.clone().unwrap_or_default();
        let port = self.port.map(|p| p.to_string()).unwrap_or_default();
        vec![scheme, host, port]
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
impl TryFrom<reqwest::Version> for HttpVersion {
    type Error = miette::Error;
    fn try_from(value: reqwest::Version) -> Result<Self, Self::Error> {
        match value {
            reqwest::Version::HTTP_09 => Ok(HttpVersion::Http09),
            reqwest::Version::HTTP_10 => Ok(HttpVersion::Http10),
            reqwest::Version::HTTP_11 => Ok(HttpVersion::Http11),
            reqwest::Version::HTTP_2 => Ok(HttpVersion::Http2),
            reqwest::Version::HTTP_3 => Ok(HttpVersion::Http3),
            _ => miette::bail!("Unsupported http version {value:?}"),
        }
    }
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Deserialize, Clone, Serialize)]
struct BasicAuth {
    user_name: String,
    password: Option<String>,
}

impl BasicAuth {
    fn substitute(self, vars: &HashMap<String, String>) -> Result<Self, subst::Error> {
        let Self {
            user_name,
            password,
        } = self;
        let user_name = subst::substitute(&user_name, vars)?;
        let password = password.map(|p| subst::substitute(&p, vars)).transpose()?;
        Ok(Self {
            user_name,
            password,
        })
    }
    fn apply_request(self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(self.user_name, self.password)
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum StdinBody {
    Tagged(TaggedBody),
    Form(HashMap<String, String>),
    Multipart(HashMap<String, Part>),
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    description: Option<String>,
    path: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    args: Vec<(String, String)>,
    #[serde(default = "default_timeout")]
    timeout: std::time::Duration,
    #[serde(default)]
    version: HttpVersion,
    basic_auth: Option<BasicAuth>,
    bearer_auth: Option<String>,
    pre_hook: Option<crate::hook::Hook>,
    post_hook: Option<crate::hook::Hook>,
    body: Option<TaggedBody>,
    form: Option<HashMap<String, String>>,
    multipart: Option<HashMap<String, Part>>,
}

impl Query {
    /// Gives columns presennt in this structure
    /// this is used for formatting
    pub fn headers() -> &'static [&'static str] {
        &["method", "path"]
    }

    /// gives vec of cells, used for format printing queries
    pub fn to_row(&self) -> Vec<String> {
        vec![self.method.clone(), self.path.clone()]
    }

    pub async fn execute(
        mut self,
        environ: Environment,
        store: &mut crate::store::Store,
        cmd_args: &crate::Arguments,
        stdin: Option<&[u8]>,
    ) -> miette::Result<Option<crate::parser::QueryResponse>> {
        trace!("Merging Query wit env");
        let Environment {
            scheme,
            host,
            port,
            prefix: env_prefix,
            mut headers,
            store: env_store,
            args: mut query_args,
        } = environ;
        let host = host.ok_or(miette::miette!("Host is empty"))?;
        let scheme = scheme.ok_or(miette::miette!("Scheme is empty"))?;
        headers.extend(self.headers);
        self.headers = headers;
        query_args.extend(self.args);
        self.args = query_args;

        let url_str = if let Some(port) = port {
            format!("{scheme}://{host}:{port}",)
        } else {
            format!("{scheme}://{host}")
        };

        let url = reqwest::Url::parse(&url_str)
            .into_diagnostic()
            .wrap_err("Couldn't parse given url")?;
        let base_url = if let Some(prefix) = env_prefix {
            url.join(&prefix)
                .into_diagnostic()
                .wrap_err_with(|| format!("Couldn't append environment prefix: {prefix}"))?
        } else {
            url
        };

        debug!(url = ?base_url, "Costructed base Url");
        let mut local_store = std::ops::Deref::deref(store).clone();
        local_store.extend(env_store);

        let pre_hook = self.pre_hook.take();
        let post_hook = self.post_hook.take();
        let mut hook_args = cmd_args.args.split(|flag| flag == "--");
        let pre_hook_args = hook_args.next().unwrap_or(&[]);
        let post_hook_args = hook_args.next().unwrap_or(&[]);

        if let Some(stdin) = stdin {
            let stdin_body = rmp_serde::decode::from_slice::<StdinBody>(stdin)
                .into_diagnostic()
                .wrap_err("Couldn't deserialize stdin as body")?;
            match stdin_body {
                StdinBody::Tagged(tagged_body) => self.body = Some(tagged_body),
                StdinBody::Form(hash_map) => self.form = Some(hash_map),
                StdinBody::Multipart(hash_map) => self.multipart = Some(hash_map),
            }
        }

        let prepared_query: PreparedQuery = self.try_into().wrap_err("Couldn't Create Query")?;
        if cmd_args.inspect_request {
            let body_buf = crate::hook::to_msgpack(&prepared_query)
                .into_diagnostic()
                .wrap_err("serializing input body")?;
            return Ok(Some(body_buf));
        }
        let query = pre_hook
            .filter(|_| !(cmd_args.skip_hooks || cmd_args.skip_prehook))
            .map(|hook| hook.run(&prepared_query, pre_hook_args))
            .transpose()
            .wrap_err("Failed to run pre hook")?
            .unwrap_or(prepared_query);

        let substituted_query = query
            .substitute(&local_store)
            .into_diagnostic()
            .wrap_err("Couldn't substitute Query request")?;
        let client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .build()
            .into_diagnostic()
            .wrap_err("Couldn't build client")?;

        let request = substituted_query
            .into_request(base_url, &client)
            .wrap_err("Couldn't construct Query")?;

        display_request(&request);

        let response = client
            .execute(request)
            .await
            .into_diagnostic()
            .wrap_err("Request failed")?;

        // convert response so that it can be sent to post hook
        let response = Response::read_response(response)
            .await
            .wrap_err("Couldn't read response")?;

        if cmd_args.inspect_response {
            let body_buf = crate::hook::to_msgpack(&response)
                .into_diagnostic()
                .wrap_err("failed to serialize response")?;
            return Ok(Some(body_buf));
        }

        let mut response = post_hook
            .filter(|_| !(cmd_args.skip_hooks || cmd_args.skip_posthook))
            .map(|hook| hook.run(&response, post_hook_args))
            .transpose()
            .wrap_err("Failed to run post hook")?
            .unwrap_or(response);
        if !response.store.is_empty() {
            store.deref_mut().extend(response.store.drain());
        }

        Ok(response.into())
    }
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum UnpackedBody {
    Utf8(String),
    Raw(Vec<u8>),
}

impl UnpackedBody {
    fn substitute(self, vars: &HashMap<String, String>) -> Result<Self, subst::Error> {
        match self {
            UnpackedBody::Utf8(s) => Ok(Self::Utf8(subst::substitute(&s, vars)?)),
            UnpackedBody::Raw(vec) => Ok(Self::Raw(vec)),
        }
    }
}

impl From<UnpackedBody> for reqwest::Body {
    fn from(value: UnpackedBody) -> Self {
        match value {
            UnpackedBody::Utf8(s) => reqwest::Body::from(s),
            UnpackedBody::Raw(vec) => reqwest::Body::from(vec),
        }
    }
}

/// unpacked version of multiparts Part type
/// all file contents are extracted
#[derive(Debug, Deserialize, Serialize)]
struct MultiPartUnPacked {
    body: UnpackedBody,
    #[serde(default)]
    headers: HashMap<String, String>,
    file_name: Option<String>,
}

impl MultiPartUnPacked {
    fn substitute(self, vars: &HashMap<String, String>) -> Result<Self, subst::Error> {
        let Self {
            body,
            headers,
            file_name,
        } = self;
        let headers = headers
            .into_iter()
            .map(|(key, value)| {
                let key = subst::substitute(&key, vars)?;
                let val = subst::substitute(&value, vars)?;
                Ok((key, val))
            })
            .collect::<Result<_, subst::Error>>()?;
        let file_name = file_name
            .map(|name| subst::substitute(&name, vars))
            .transpose()?;
        Ok(Self {
            body: body.substitute(vars)?,
            headers,
            file_name,
        })
    }
}

/// multipart value struct
#[derive(Debug, Deserialize, Clone, Serialize)]
struct Part {
    body: TaggedBody,
    #[serde(default)]
    headers: HashMap<String, String>,
    file_name: Option<String>,
}

impl Part {
    fn unpack(self) -> miette::Result<MultiPartUnPacked> {
        let Self {
            body,
            mut headers,
            file_name,
        } = self;
        let (content_type, body) = body.unpack()?;
        headers.insert(reqwest::header::CONTENT_TYPE.to_string(), content_type);
        Ok(MultiPartUnPacked {
            body,
            headers,
            file_name,
        })
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum TaggedBody {
    #[serde(rename = "application/json")]
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

impl TaggedBody {
    fn unpack(self) -> miette::Result<(String, UnpackedBody)> {
        match self {
            TaggedBody::ApplicationJson(content) => {
                let val = content
                    .get_value()
                    .wrap_err("Couldn't extract application/json body")?;
                Ok((
                    mime::APPLICATION_JSON.as_ref().to_string(),
                    UnpackedBody::Utf8(val),
                ))
            }
            TaggedBody::Raw { content_type, data } => {
                let val = data
                    .get_value()
                    .wrap_err("Couldn't extract application/json body")?;
                Ok((content_type, UnpackedBody::Raw(val)))
            }
            TaggedBody::RawText { content_type, data } => {
                let val = data
                    .get_value()
                    .wrap_err("Couldn't extract application/json body")?;
                Ok((content_type, UnpackedBody::Utf8(val)))
            }
        }
    }
}

trait FromBytes {
    type Error: core::error::Error + Send + Sync + 'static;
    fn from_bytes(vec: Vec<u8>) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl FromBytes for Vec<u8> {
    type Error = std::convert::Infallible;

    fn from_bytes(vec: Vec<u8>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        Ok(vec)
    }
}

impl FromBytes for String {
    type Error = std::string::FromUtf8Error;

    fn from_bytes(vec: Vec<u8>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        String::from_utf8(vec)
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum Content<T: FromBytes> {
    File(std::path::PathBuf),
    Inline(T),
}

impl<T: FromBytes> Content<T> {
    fn get_value(self) -> miette::Result<T> {
        match self {
            Content::File(path_buf) => {
                let mut file = std::fs::File::open(&path_buf)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Couldn't open file: {path_buf:?}"))?;
                let mut content = Vec::new();
                let read_bytes = file
                    .read_to_end(&mut content)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Couldn't read file: {path_buf:?}"))?;
                debug!("read: {read_bytes} bytes from {path_buf:?}");
                T::from_bytes(content)
                    .into_diagnostic()
                    .wrap_err("Couldn't convert file content to intented type")
            }
            Content::Inline(i) => Ok(i),
        }
    }
}

/// Query generated keeping required parts of Query which are required for generating query
#[derive(Debug, Serialize, Deserialize)]
struct PreparedQuery {
    path: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    args: Vec<(String, String)>,
    #[serde(default = "default_timeout")]
    timeout: std::time::Duration,
    #[serde(default)]
    version: HttpVersion,
    basic_auth: Option<BasicAuth>,
    bearer_auth: Option<String>,
    body: Option<UnpackedBody>,
    form: Option<HashMap<String, String>>,
    multipart: Option<HashMap<String, MultiPartUnPacked>>,
}

impl TryFrom<Query> for PreparedQuery {
    type Error = miette::Error;

    fn try_from(query: Query) -> Result<Self, Self::Error> {
        let mut headers = query.headers;
        let body = query
            .body
            .map(|tagged_body| -> miette::Result<_> {
                let (content_type, unpacked_body) = tagged_body.unpack()?;
                headers.insert(reqwest::header::CONTENT_TYPE.to_string(), content_type);
                Ok(unpacked_body)
            })
            .transpose()
            .wrap_err("Couldn't unpack request body")?;
        let multipart = query
            .multipart
            .map(|m| {
                m.into_iter()
                    .map(|(k, part)| {
                        let unpacked_part = part.unpack()?;

                        Ok((k, unpacked_part))
                    })
                    .collect::<Result<HashMap<_, _>, miette::Error>>()
                    .wrap_err("Couldn't unpack request")
            })
            .transpose()?;
        Ok(Self {
            path: query.path,
            method: query.method,
            headers,
            args: query.args,
            timeout: query.timeout,
            version: query.version,
            basic_auth: query.basic_auth,
            bearer_auth: query.bearer_auth,
            body,
            form: query.form,
            multipart,
        })
    }
}

impl PreparedQuery {
    fn into_request(
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
        let builder = client
            .request(method, url)
            .headers(headers)
            .timeout(self.timeout)
            .query(&self.args)
            .version(self.version.into());
        let builder = if let Some(body) = self.body {
            builder.body(body)
        } else {
            builder
        };

        let builder = if let Some(bearer_auth) = self.bearer_auth {
            builder.bearer_auth(bearer_auth)
        } else {
            builder
        };

        let builder = if let Some(basic_auth) = self.basic_auth {
            basic_auth.apply_request(builder)
        } else {
            builder
        };

        let builder = if let Some(form) = self.form {
            builder.form(&form)
        } else {
            builder
        };

        let builder = if let Some(multipart) = self.multipart {
            let form = multipart
                .into_iter()
                .try_fold(
                    reqwest::multipart::Form::new(),
                    |form, (name, part)| -> miette::Result<reqwest::multipart::Form> {
                        let MultiPartUnPacked {
                            body,
                            headers,
                            file_name,
                        } = part;
                        let part = match body {
                            UnpackedBody::Utf8(c) => reqwest::multipart::Part::text(c),
                            UnpackedBody::Raw(vec) => reqwest::multipart::Part::bytes(vec),
                        };
                        let part = if let Some(file_name) = file_name {
                            part.file_name(file_name)
                        } else {
                            part
                        };
                        let headers = (&headers)
                            .try_into()
                            .into_diagnostic()
                            .wrap_err("Invalid headers")?;
                        let part = part.headers(headers);
                        Ok(form.part(name, part))
                    },
                )
                .wrap_err("Couldn't construct multiform request")?;
            builder.multipart(form)
        } else {
            builder
        };

        builder
            .build()
            .into_diagnostic()
            .wrap_err("Couldn't build request")
    }

    fn substitute(self, vars: &HashMap<String, String>) -> Result<Self, subst::Error> {
        let Self {
            path,
            method,
            headers,
            args,
            timeout,
            basic_auth,
            bearer_auth,
            version,
            body,
            form,
            multipart,
        } = self;
        let path = subst::substitute(&path, vars)?;
        let method = subst::substitute(&method, vars)?;

        let headers = headers
            .into_iter()
            .map(|(key, value)| {
                let key = subst::substitute(&key, vars)?;
                let val = subst::substitute(&value, vars)?;
                Ok((key, val))
            })
            .collect::<Result<_, subst::Error>>()?;

        let args = args
            .into_iter()
            .map(|(key, value)| {
                let key = subst::substitute(&key, vars)?;
                let val = subst::substitute(&value, vars)?;
                Ok((key, val))
            })
            .collect::<Result<_, subst::Error>>()?;

        let basic_auth = basic_auth.map(|b| b.substitute(vars)).transpose()?;
        let bearer_auth = bearer_auth
            .map(|b| subst::substitute(&b, vars))
            .transpose()?;

        let form = form
            .map(|form| {
                form.into_iter()
                    .map(|(key, value)| {
                        let key = subst::substitute(&key, vars)?;
                        let val = subst::substitute(&value, vars)?;
                        Ok((key, val))
                    })
                    .collect::<Result<_, subst::Error>>()
            })
            .transpose()?;

        let multipart = multipart
            .map(|form| {
                form.into_iter()
                    .map(|(key, value)| {
                        let key = subst::substitute(&key, vars)?;
                        let val = value.substitute(vars)?;
                        Ok((key, val))
                    })
                    .collect::<Result<_, subst::Error>>()
            })
            .transpose()?;

        Ok(Self {
            path,
            headers,
            args,
            method,
            timeout,
            version,
            basic_auth,
            bearer_auth,
            body: body.map(|body| body.substitute(vars)).transpose()?,
            form,
            multipart,
        })
    }
}

/// To display headers
struct DisplayResponseHeaders<'a>(&'a reqwest::header::HeaderMap);

impl std::fmt::Display for DisplayResponseHeaders<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (key, val) in self.0 {
            write!(f, "\n< {}: {:?}", key.yellow(), val)?
        }
        Ok(())
    }
}

/// To display headers
struct DisplayRequestHeaders<'a>(&'a reqwest::header::HeaderMap);

impl std::fmt::Display for DisplayRequestHeaders<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (key, val) in self.0 {
            write!(f, "\n> {}: {:?}", key.yellow(), val)?
        }
        Ok(())
    }
}

fn is_extension_method(method: &reqwest::Method) -> bool {
    !matches!(
        method.as_str(),
        "GET" | "PUT" | "POST" | "HEAD" | "PATCH" | "TRACE" | "DELETE" | "OPTIONS" | "CONNECT"
    )
}

fn display_request(request: &reqwest::Request) {
    let method = request.method();
    let url = request.url().as_str();
    if is_extension_method(method) {
        warn!("using non-standard extension method: {method}");
        info!("[{}]: {url}", method.red().bold());
    } else {
        info!("[{method}]: {url}");
    }
    let headers = DisplayRequestHeaders(request.headers());
    info!("headers: {headers}");

    if let Some(body) = request.body() {
        if let Some(body_bytes) = body.as_bytes().map(str::from_utf8) {
            if let Ok(decoded_body) = body_bytes {
                info!("body: {decoded_body}")
            } else {
                info!("body: {body_bytes:x?}")
            }
        } else {
            info!("body: Streaming data, body cannot be displayed")
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Response {
    status_code: u16,
    version: HttpVersion,
    headers: HashMap<String, String>,
    store: HashMap<String, String>,
    body: Vec<u8>,
}

impl Response {
    async fn read_response(mut response: reqwest::Response) -> miette::Result<Self> {
        info!("status: {}", response.status());
        info!("version: {:?}", response.version());
        let header_map = DisplayResponseHeaders(response.headers());
        info!("headers: {header_map}");
        Ok(Self {
            status_code: response.status().into(),
            version: response
                .version()
                .try_into()
                .wrap_err("Unexpected response version")?,
            headers: response
                .headers_mut()
                .into_iter()
                .map(|(key, val)| {
                    Ok((
                        key.to_string(),
                        val.to_str()
                            .into_diagnostic()
                            .wrap_err("Unexpected header value")?
                            .to_string(),
                    ))
                })
                .collect::<Result<HashMap<_, _>, miette::Error>>()?,
            body: response
                .bytes()
                .await
                .into_diagnostic()
                .wrap_err("Couldn't read response body")?
                .into(),
            store: HashMap::new(),
        })
    }
}

impl From<Response> for Option<crate::parser::QueryResponse> {
    fn from(value: Response) -> Self {
        Some(value.body)
    }
}
