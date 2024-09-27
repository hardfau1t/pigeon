use miette::{Context, IntoDiagnostic};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{borrow::Borrow, io::Write};
use tracing::{debug, info, instrument, trace};

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    Closure(String),
    #[serde(rename = "script")]
    Path(std::path::PathBuf),
}

impl Hook {
    #[instrument(skip(input, args))]
    pub fn run<T: Serialize + DeserializeOwned>(
        &self,
        input: &T,
        args: &[impl Borrow<str>],
    ) -> miette::Result<T> {
        trace!("running Hook");
        // size will always be larger than obj, but atleast optimize is for single allocation
        let body_buf = rmp_serde::encode::to_vec_named(&input)
            .into_diagnostic()
            .wrap_err("serializing input body")?;
        match self {
            Hook::Closure(_cl) => unimplemented!("Currently closures are not supported"),
            Hook::Path(path) => {
                debug!("Executing hook: {path:?}");
                let mut child = std::process::Command::new(path)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .args(args.iter().map(|arg| arg.borrow()))
                    .spawn()
                    .into_diagnostic()
                    .wrap_err("Couldn't run hook")?;
                debug!("writing to child: {body_buf:x?}");
                child
                    .stdin
                    .take()
                    .expect("Childs stdin is not open, eventhough body is present")
                    .write_all(&body_buf)
                    .into_diagnostic()
                    .wrap_err("Failed to send body to hook")?;
                let output = child
                    .wait_with_output()
                    .into_diagnostic()
                    .wrap_err("Failed to read hook output")?;
                debug!("pre-hook output: {:x?}", output.stdout);
                info!(
                    "pre-hook stderr: `{}`",
                    String::from_utf8_lossy(&output.stderr)
                );
                let pre_hook_obj: T = rmp_serde::from_slice(output.stdout.as_ref())
                    .into_diagnostic()
                    .wrap_err("Failed to deserialize output of hooks")?;
                Ok(pre_hook_obj)
            }
        }
    }
}
