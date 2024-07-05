use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashMap, io::Write, str::FromStr};
use tracing::{debug, error, info, trace, warn};

pub mod constants;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    #[allow(dead_code)]
    version: String,
    #[serde(rename = "environment")]
    environments: Vec<Environment>,
    #[serde(rename = "service")]
    pub services: Vec<Service>,
}

impl Document {
    pub fn get_service(&self, service_name: &str) -> Option<&Service> {
        self.services.iter().find(|svc| {
            svc.name == service_name
                || svc
                    .alias
                    .as_ref()
                    .is_some_and(|alias| alias == service_name)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Environment {
    name: String,
    service: Vec<ServiceEnv>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceEnv {
    name: String,
    #[serde(deserialize_with = "deserialize_scheme")]
    scheme: http::uri::Scheme,
    host: String,
}

impl TryInto<url::Url> for &ServiceEnv {
    type Error = url::ParseError;

    fn try_into(self) -> Result<url::Url, Self::Error> {
        url::Url::from_str(&format!("{}://{}", self.scheme.as_str(), self.host))
    }
}

/// represents single microservice
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub alias: Option<String>,
    pub endpoint: Vec<EndPoint>,
}

impl Service {
    pub fn get_endpoint(&self, ep_name: &str) -> Option<&EndPoint> {
        self.endpoint.iter().find(|ep| {
            ep.name == ep_name || ep.alias.as_ref().is_some_and(|alias| alias == ep_name)
        })
    }
}

/// represents one endpoint of given microservice
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndPoint {
    pub name: String,
    pub alias: Option<String>,
    method: Method,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Body>,
    pre_hook: Option<Vec<Hook>>,
    post_hook: Option<Vec<Hook>>,
    #[serde(default)]
    flags: Vec<String>,
    path: String,
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

/// Http Methods
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
enum Method {
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

impl std::string::ToString for Method {
    fn to_string(&self) -> String {
        match self {
            Method::Get => "GET".to_string(),
            Method::Post => "POST".to_string(),
            Method::Put => "PUT".to_string(),
            Method::Delete => "DELETE".to_string(),
            Method::Head => "HEAD".to_string(),
            Method::Options => "OPTIONS".to_string(),
            Method::Connect => "CONNECT".to_string(),
            Method::Patch => "PATCH".to_string(),
            Method::Trace => "TRACE".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
enum Hook {
    Closure(()),
    #[serde(rename = "script")]
    Path(std::path::PathBuf),
}

/// deserialization function for uri scheme
fn deserialize_scheme<'de, D>(deserializer: D) -> Result<http::uri::Scheme, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let str_val = String::deserialize(deserializer)?;
    http::uri::Scheme::from_str(&str_val)
        .map_err(|e| serde::de::Error::custom(format!("Failed to parse uri: {e:?}")))
}

/// parses document and run given query
pub fn execute(
    document: &Document,
    service_name: &str,
    endpoint_name: &str,
) -> Result<(), anyhow::Error> {
    let current_env = std::env::var(constants::KEY_CURRENT_ENVIRONMENT)?;
    // get service with given service_name
    let service = document
        .services
        .iter()
        .find_map(|svc| {
            if svc.name == service_name
                || svc
                    .alias
                    .as_ref()
                    .is_some_and(|alias| alias == service_name)
            {
                Some(svc)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to get service with name/alias: {service_name}"))?;
    // get the service config of given service in given environment
    let env = document
        .environments
        .iter()
        .find_map(|env| {
            if env.name == current_env {
                Some(env)
            } else {
                None
            }
        })
        .and_then(|env| {
            env.service.iter().find_map(|service_env| {
                if service_env.name == service.name {
                    Some(service_env)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Failed to get service: {service_name} from env: {current_env}")
        })?;
    // seach for given endpoint
    let endpoint = service
        .endpoint
        .iter()
        .find_map(|ep| {
            if ep.name == endpoint_name
                || ep
                    .alias
                    .as_ref()
                    .is_some_and(|alias| alias == endpoint_name)
            {
                Some(ep)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Couldn't find such endpoint: {endpoint_name} in service {service_name}"
            )
        })?;
    let host = env.try_into()?;
    call_request(host, endpoint)
}

fn call_request(host: url::Url, endpoint: &EndPoint) -> anyhow::Result<()> {
    let uri = host.join(&endpoint.path)?;
    let req = ureq::request(&endpoint.method.to_string(), uri.as_str());
    debug!(headers = ?endpoint.headers);
    let req = endpoint
        .headers
        .iter()
        .fold(req, |req, header| req.set(&header.0, &header.1))
        // set query params
        .query_pairs(
            endpoint
                .params
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
    // if there is body then set request with kind and return some body else just return req
    // HACK: can't use map_or becausecurrently rust can't differenciate whether one of two closure would get executed so it moves into both, thus that requires clone
    // that would look like this
    // .map_or((req, None), |(kind, content)| {
    //     (req.set("Content-Type", kind), Some(content))
    // });
    let (req, body) = if let Some((content_type, body)) = endpoint
        .body
        .as_ref()
        .map(|body| {
            match &body.data {
                BodyData::Inline(b) => Ok(b.clone()),
                BodyData::Path(path) => std::fs::read_to_string(path),
            }
            .map(|content| (&body.kind, content))
        })
        .transpose()?
    {
        (
            req.set("Content-Type", content_type),
            Some(body.into_bytes()),
        )
    } else {
        (req, None)
    };
    let flags = endpoint
        .flags
        .iter()
        .map(|flag| flag.as_str())
        .collect::<Vec<_>>();
    let f = flags.as_slice();
    // if prehook is present then execute pre hook else set the content type and return content
    let (req, body) = if let Some(hooks) = endpoint.pre_hook.as_ref() {
        hooks.iter().fold((req, body), |(req, body), hook| {
            exec_prehook(req, body.as_ref().map(|b_vec| b_vec.as_slice()), hook, f)
        })
    } else {
        (req, body)
    };
    let response = if let Some(ref body) = body {
        info!( request= ?req, body= ?body, "sending request with body");
        req.send_bytes(body.as_slice())
    } else {
        info!( request= ?req, "sending request");
        req.call()
    }?;

    if let Some(post_hook) = &endpoint.post_hook {
        let obj = PostHookObject::from(response);
        let final_obj = post_hook
            .iter()
            .fold(obj, |last_obj, hook| exec_posthook(&last_obj, hook, &[]));
        info!("response headers: {:#?}", final_obj.headers);
        info!("response status: {:#?}, status_text: {:#?}", final_obj.status, final_obj.status_text);
        if let Some(data) = final_obj.body.as_ref() {
            std::io::stdout().write_all(data)?;
        }
    } else {
        let resp_header_names = response.headers_names();
        let resp_headers = resp_header_names
            .iter()
            .map(|name| {
                let vals = response.all(name.as_str());
                (name, vals)
            })
            .collect::<HashMap<_, _>>();
        info!("response headers: {resp_headers:#?}");
        let mut body = Vec::new();
        response.into_reader().read_to_end(&mut body)?;
        std::io::stdout().write_all(&body)?;
    }
    Ok(())
}

/// this will be given to prehook script
#[derive(Debug, Serialize, Deserialize)]
struct PreHookObject<'headers, 'params, 'body> {
    #[serde(borrow)]
    headers: HashMap<&'headers str, Vec<&'headers str>>,
    #[serde(borrow)]
    params: Vec<(&'params str, &'params str)>,
    body: Option<&'body [u8]>,
}
/// this is the output of pre-hook script
#[derive(Debug, Serialize, Deserialize)]
struct PreHookObjectResponse {
    headers: HashMap<String, Vec<String>>,
    params: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

/// this will be given to prehook script
#[derive(Debug, Deserialize, Serialize)]
struct PostHookObject {
    headers: HashMap<String, Vec<String>>,
    body: Option<Vec<u8>>,
    status: u16,
    status_text: String,
}

impl From<ureq::Response> for PostHookObject {
    fn from(response: ureq::Response) -> Self {
        let mut body = Vec::new();
        let header_keys = response.headers_names();
        let status = response.status();
        let status_text = response.status_text().to_string();
        let headers: HashMap<_, _> = header_keys
            .into_iter()
            .map(|header_name| {
                let vals = response
                    .all(&header_name)
                    .iter()
                    .map(|val_ref| val_ref.to_string())
                    .collect();
                (header_name, vals)
            })
            .collect();
        if let Err(e) = response.into_reader().read_to_end(&mut body) {
            warn!("Error while reading response body: {e}, truncate body");
            body.clear();
        };
        PostHookObject {
            headers,
            body: Some(body),
            status,
            status_text,
        }
    }
}

/// this will be given to prehook script
#[derive(Debug, Deserialize, Serialize)]
struct PostHookObjectResponse {
    headers: HashMap<String, Vec<String>>,
    body: Option<Vec<u8>>,
    status: u16,
    status_text: String,
}

fn exec_prehook(
    req: ureq::Request,
    body: Option<&[u8]>,
    hook: &Hook,
    flags: &[&str],
) -> (ureq::Request, Option<Vec<u8>>) {
    let header_keys = req.header_names();
    let headers: HashMap<_, _> = header_keys
        .iter()
        .map(|header_name| (header_name.as_str(), req.all(header_name)))
        .collect();
    let url = req.url();
    let query_params =
        url::Url::parse(url).expect("Invalid url shouldn't be accepted in the first place");
    let query_pairs_pars: Vec<_> = query_params.query_pairs().collect();
    let params = query_pairs_pars
        .iter()
        .map(|(ref key, ref val)| (Cow::as_ref(key), Cow::as_ref(val)))
        .collect::<Vec<(&'_ str, &'_ str)>>();

    let obj = PreHookObject {
        headers,
        params,
        body,
    };
    debug!("pre-hook obj sending to pre-hook: {obj:?}");
    // size will always be larger than obj, but atleast optimize is for single allocation
    let body_buf = rmp_serde::encode::to_vec_named(&obj).unwrap_or_else(|e| {
        error!("Failed to serialize pre-hook obj: {e}");
        panic!("Failed to call pre-hook")
    });

    match hook {
        Hook::Closure(_) => unimplemented!("Currently closures are not supported"),
        Hook::Path(path) => {
            trace!("Executing pre-hook script");
            let mut child = std::process::Command::new(path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .args(flags)
                .spawn()
                .unwrap_or_else(|e| {
                    error!("Failed to spawn {path:?} : {e}");
                    panic!("Failed call pre-hook")
                });
            debug!("writing to child: {body_buf:?}");
            child
                .stdin
                .take()
                .expect("Childs stdin is not open, eventhough body is present")
                .write_all(&body_buf)
                .unwrap_or_else(|e| {
                    error!("Failed to write body data: {e}");
                    panic!("Couldn't pass body to pre-hook")
                });
            let output = child.wait_with_output().unwrap_or_else(|e| {
                error!("Failed to read pre-hook stdout: {e}");
                panic!("Couldn't read pre-hook output")
            });
            debug!(output=?output.stdout, "pre-hook output");
            let mut pre_hook_obj: PreHookObjectResponse =
                rmp_serde::from_slice(output.stdout.as_ref()).unwrap_or_else(|e| {
                    error!("Failed to deserialize pre-hook output: {e}");
                    panic!("Unexpected pre-hook output")
                });
            debug!(
                "pre-hook stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let body = pre_hook_obj.body.take();
            let req = ureq::request(req.method(), req.url());
            // set all headers
            let req = pre_hook_obj
                .headers
                .iter()
                .fold(req, |req, (key, values)| {
                    values.iter().fold(req, |req, value| req.set(key, value))
                })
                .query_pairs(
                    pre_hook_obj
                        .params
                        .iter()
                        .map(|(ref key, ref val)| (key.as_str(), val.as_str())),
                );
            (req, body)
        }
    }
}

fn exec_posthook(obj: &PostHookObject, hook: &Hook, flags: &[&str]) -> PostHookObject {
    // size will always be larger than obj, but atleast optimize is for single allocation
    let body_buf = rmp_serde::encode::to_vec_named(&obj).unwrap_or_else(|e| {
        error!("Failed to serialize pre-hook obj: {e}");
        panic!("Failed to call pre-hook")
    });

    match hook {
        Hook::Closure(_) => unimplemented!("Currently closures are not supported"),
        Hook::Path(path) => {
            trace!("Executing post-hook script");
            let mut child = std::process::Command::new(path)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .args(flags)
                .spawn()
                .unwrap_or_else(|e| {
                    error!("Failed to spawn {path:?} : {e}");
                    panic!("Failed call pre-hook")
                });
            debug!("writing to child: {body_buf:?}");
            child
                .stdin
                .take()
                .expect("Childs stdin is not open, eventhough body is present")
                .write_all(&body_buf)
                .unwrap_or_else(|e| {
                    error!("Failed to write body data: {e}");
                    panic!("Couldn't pass body to pre-hook")
                });
            let output = child.wait_with_output().unwrap_or_else(|e| {
                error!("Failed to read pre-hook stdout: {e}");
                panic!("Couldn't read pre-hook output")
            });
            debug!(output=?output.stdout, "post-hook output");
            let post_hook_resp =
                rmp_serde::from_slice(output.stdout.as_ref()).unwrap_or_else(|e| {
                    error!("Failed to deserialize pre-hook output: {e}");
                    panic!("Unexpected pre-hook output")
                });
            debug!(
                "post-hook stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            post_hook_resp
        }
    }
}
