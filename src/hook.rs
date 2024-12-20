use miette::{Context, IntoDiagnostic};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{borrow::Borrow, io::Write, os::unix::process::ExitStatusExt};
use tracing::{debug, error, instrument, trace};

// TODO: add Hook executor which takes arguments like executor which executes given script
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
        let body_buf = to_msgpack(&input)
            .into_diagnostic()
            .wrap_err("serializing input body")?;
        match self {
            Hook::Closure(_cl) => unimplemented!("Currently closures are not supported"),
            Hook::Path(path) => {
                debug!("Executing hook: {path:?}");
                // setup child to take stdin and return both stdout and stdin
                let mut child = std::process::Command::new(path)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .args(args.iter().map(|arg| arg.borrow()))
                    .spawn()
                    .into_diagnostic()
                    .wrap_err("Couldn't run hook")?;

                debug!("writing to child: {body_buf:x?}");

                // execute child with input
                child
                    .stdin
                    .take()
                    .expect("Childs stdin is not open, eventhough body is present")
                    .write_all(&body_buf)
                    .into_diagnostic()
                    .wrap_err("Failed to send body to hook")?;

                // collect child output
                let output = child
                    .wait_with_output()
                    .into_diagnostic()
                    .wrap_err("Failed to read hook output")?;
                debug!("pre-hook output: {:x?}", output.stdout);

                // assuming stderr to be utf-8
                let child_stderr = String::from_utf8_lossy(&output.stderr);

                if !child_stderr.is_empty() {
                    error!("pre-hook stderr: `{}`", child_stderr);
                }
                // check if the execution is success or not
                if !output.status.success() {
                    let code =
                        std::process::ExitStatus::from_raw(output.status.code().unwrap_or(1));
                    miette::bail!("hook exited with error: {code}")
                }

                // deserialize output and read from stdout
                let pre_hook_obj: T = rmp_serde::from_slice(output.stdout.as_ref())
                    .into_diagnostic()
                    .wrap_err("Failed to deserialize output of hooks")?;

                Ok(pre_hook_obj)
            }
        }
    }
}

pub fn to_msgpack<T: Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut output = Vec::new();
    let mut serializer = rmp_serde::Serializer::new(&mut output)
        .with_binary()
        .with_struct_map()
        .with_bytes(rmp_serde::config::BytesMode::ForceAll);
    value.serialize(&mut serializer)?;
    Ok(output)
}
