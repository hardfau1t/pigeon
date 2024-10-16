use crate::{
    parser::{BodyData, Bundle, EndPoint, Substituted},
    store::Store,
};
use miette::{bail, Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, collections::HashMap, io::Read, ops::Deref};
use tracing::{debug, info, instrument, trace, warn};

#[derive(Debug, Serialize, Deserialize)]
struct RequestHookObject {
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    params: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    path: String,
    method: crate::parser::Method,
    #[serde(default)]
    config: HashMap<String, String>,
}

impl RequestHookObject {
    fn into_request(self, base_url: url::Url) -> Result<ureq::Request, url::ParseError> {
        let path = self.path.as_str().trim_start_matches('/');
        let url = base_url.join(path)?;
        let request = ureq::request(&self.method.to_string(), url.as_str());

        // add headers
        let request = self
            .headers
            .iter()
            .fold(request, |request, (key, value)| {
                request.set(key.as_str(), value.as_str())
            })
            .query_pairs(
                self.params
                    .iter()
                    .map(|(key, val)| (key.as_str(), val.as_str())),
            );
        Ok(request)
    }
}

/// this will be given to prehook script
#[derive(Debug, Deserialize, Serialize)]
struct ResponseHookObject {
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    status: u16,
    status_text: String,
    #[serde(default)]
    config: HashMap<String, String>,
}

impl ResponseHookObject {
    fn from_response(response: ureq::Response, config: HashMap<String, String>) -> Self {
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
        ResponseHookObject {
            headers,
            body: Some(body),
            status,
            status_text,
            config,
        }
    }
}
pub fn execute(
    end_point: EndPoint<Substituted>,
    base_url: url::Url,
    config_store: &mut Store,
    flags: &[impl Borrow<str>],
    dry_run: bool,
    skip_prehook: bool,
    skip_posthook: bool,
    input_file: Option<&std::path::Path>,
) -> miette::Result<Option<Vec<u8>>> {
    trace!("executing query");
    let mut flags_iter = flags.split(|flag| flag.borrow() == "--");
    let request_hook_flags = flags_iter.next().unwrap_or(&[]);
    let response_hook_flags = flags_iter.next().unwrap_or(&[]);
    let EndPoint {
        method,
        mut headers,
        params,
        body,
        pre_hook,
        post_hook,
        path,
        ..
    } = end_point;

    let body = body
        .map(|body| {
            if let Some(path_override) = input_file {
                // read from either stdin or file
                if path_override == (AsRef::<std::path::Path>::as_ref("-")) {
                    let mut buf = Vec::<u8>::new();
                    std::io::stdin().read_to_end(&mut buf).map(|_| buf)
                } else {
                    std::fs::read(path_override)
                }
                // map the errors
                .into_diagnostic()
                .wrap_err_with(|| format!("Couldn't read file {path_override:?}"))
                .map(|content| (body.kind, content))
            } else {
                match body.data {
                    BodyData::Inline(d) => Ok((body.kind, d.into_bytes())),
                    BodyData::Path(path) => std::fs::read(&path)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("Couldn't read file {path:?}"))
                        .map(|content| (body.kind, content)),
                }
            }
        })
        .transpose()?;
    let body = body.map(|(kind, body)| {
        headers.insert("Content-Type".to_string(), kind);
        body
    });
    let request_object = RequestHookObject {
        headers,
        params,
        body,
        path,
        method,
        // HACK: I couldn't figure out how to send reference of hashmap to sereialize and take hashmap from deserialize
        config: config_store.deref().deref().clone(),
    };

    // run pre-hook if it is available
    let mut mapped_request_obj = pre_hook
        .filter(|_| !skip_prehook)
        .map(|hook| hook.run(&request_object, request_hook_flags))
        .transpose()?
        .map(|mut obj| {
            // pre hook was present so update the config store
            config_store.extend(obj.config.drain());
            obj
        })
        .unwrap_or(request_object);
    let body = mapped_request_obj.body.take();
    let request = mapped_request_obj
        .into_request(base_url)
        .into_diagnostic()
        .wrap_err("failed to create request object")?;
    info!("Query {} {}", request.method(), request.url());
    info!("headers:\n{}", {
        let mut headers = request.header_names();
        headers.dedup();
        headers
            .iter()
            .map(|key| {
                let value = request.all(key).join(",");
                format!("> {key}: {value}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    });

    // generate request object
    let resp = if let Some(body) = body {
        match std::str::from_utf8(body.as_slice()) {
            Ok(str_body) => info!("request body: '{str_body}'"),
            Err(e) => {
                warn!("Couldn't decode body as utf8 string: {e}");
                info!("request body: {body:x?}");
            }
        }
        if dry_run {
            return Ok(None);
        }
        request.send_bytes(body.as_slice())
    } else {
        if dry_run {
            return Ok(None);
        }

        request.call()
    };
    let response = match resp {
        Ok(ok_val) => ok_val,
        Err(e) => match e {
            ureq::Error::Status(code, response) => {
                warn!("Request Failed with code: {code}");
                response
            }
            ureq::Error::Transport(e) => {
                bail!("Transport error occurred during processing of request: {e}")
            }
        },
    };

    let post_hook_obj: ResponseHookObject =
        ResponseHookObject::from_response(response, config_store.deref().deref().clone());
    // display response
    info!(
        "response status: {} {}",
        post_hook_obj.status, post_hook_obj.status_text
    );

    info!(
        "headers:\n{}",
        post_hook_obj
            .headers
            .iter()
            .map(|(key, value)| { format!("< {key}: {value}") })
            .collect::<Vec<_>>()
            .join("\n")
    );
    let hook_response = post_hook
        .filter(|_| !skip_posthook)
        .map(|hook| hook.run(&post_hook_obj, response_hook_flags))
        .transpose()?
        .map(|mut obj| {
            config_store.extend(obj.config.drain());
            obj
        })
        .unwrap_or(post_hook_obj);
    Ok(hook_response.body)
}
/// run query pointed by keys
///
/// * `keys`: path which points to given query
/// * `flags`: flags for hooks `--` will separate flags into pre-hook and post-hook flags
/// * `persistent_config`: whether to store changes to config back
#[instrument(skip(hooks_flags, bundle))]
pub fn run<T: std::borrow::Borrow<str> + std::fmt::Debug>(
    bundle: &Bundle,
    keys: &[T],
    hooks_flags: &[impl std::borrow::Borrow<str>],
    persistent_config: bool,
    dry_run: bool,
    skip_prehook: bool,
    skip_posthook: bool,
    current_env: Option<&str>,
    input_file: Option<&std::path::Path>,
) -> miette::Result<Option<Vec<u8>>> {
    trace!("running query");
    let (Some((endpoint, environments)), _) = bundle.find(keys) else {
        bail!("couldn't find endpoint with {}", keys.join("."));
    };
    let mut config_store = crate::store::Store::with_env(&bundle.package)
        .into_diagnostic()
        .wrap_err_with(|| format!("Couldn't read store values of {}", bundle.package))?;
    debug!("current config: {config_store:?}");
    config_store.persistent(persistent_config);
    let Some(current_env_name) = current_env.or_else(|| {
        config_store
            .get(crate::constants::KEY_CURRENT_ENVIRONMENT)
            .map(|s| s.as_str())
    }) else {
        bail!(
            "missing environment, set: {}",
            crate::constants::KEY_CURRENT_ENVIRONMENT
        )
    };
    let Some(current_env) = environments.get(current_env_name) else {
        let a = environments
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
            .as_slice()
            .join(", ");
        bail!(
            "{current_env_name} environment is not configured, available are: {}",
            a
        )
    };
    debug!("Current environment: {current_env:?}");
    current_env.store.iter().for_each(|(key, value)| {
        let entry = config_store.entry(key.clone());
        entry.or_insert(value.clone());
    });
    let built_endpoint = endpoint
        .substitute(&config_store, &current_env.as_ref().headers)
        .into_diagnostic()
        .wrap_err("Failed to substitute key values in query")?;
    execute(
        built_endpoint,
        current_env
            .as_ref()
            .try_into()
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "Couldn't convert current environment {} into url",
                    current_env
                )
            })?,
        &mut config_store,
        hooks_flags,
        dry_run,
        skip_prehook,
        skip_posthook,
        input_file,
    )
}
