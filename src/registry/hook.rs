use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{borrow::Borrow, io::Write};
use tracing::{debug, info, trace};

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    Closure(String),
    #[serde(rename = "script")]
    Path(std::path::PathBuf),
}

impl Hook {
    pub fn run<T: Serialize + DeserializeOwned>(
        &self,
        input: &T,
        args: &[impl Borrow<str>],
    ) -> anyhow::Result<T> {
        // size will always be larger than obj, but atleast optimize is for single allocation
        let body_buf = rmp_serde::encode::to_vec_named(&input)?;
        match self {
            Hook::Closure(_cl) => unimplemented!("Currently closures are not supported"),
            Hook::Path(path) => {
                trace!("Executing pre-hook script");
                let mut child = std::process::Command::new(path)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .args(args.iter().map(|arg| arg.borrow()))
                    .spawn()?;
                debug!("writing to child: {body_buf:x?}");
                child
                    .stdin
                    .take()
                    .expect("Childs stdin is not open, eventhough body is present")
                    .write_all(&body_buf)?;
                let output = child.wait_with_output()?;
                debug!("pre-hook output: {:x?}", output.stdout);
                info!(
                    "pre-hook stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let pre_hook_obj: T = rmp_serde::from_slice(output.stdout.as_ref()).unwrap();
                Ok(pre_hook_obj)
            }
        }
    }
}
