mod agent;
mod constants;
mod hook;
mod parser;
mod store;

use std::io::{IsTerminal, Read, Write};

use clap::Parser;
use miette::{Context, IntoDiagnostic};
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, clap::Parser)]
#[command(author, version, about)]
/// make rest queries, automate
struct Arguments {
    #[arg(short, long, global=true, action=clap::ArgAction::Count)]
    verbose: u8,
    /// configuration file containing queries
    #[arg(short, long, default_value = "./pigeon.toml")]
    config_file: std::path::PathBuf,
    /// don't store changes to config store back to disk
    #[arg(short('p'), long("no-persistent"))]
    no_persistent: bool,

    // write output to given file
    #[arg(short, long)]
    output: Option<std::path::PathBuf>,

    /// list available options (services/endpoints)
    #[arg(short, long)]
    list: bool,

    /// use given environment
    #[arg(short, long)]
    environment: Option<String>,

    /// don't run the query just run till pre-hook
    /// use with --verbose(-v) to be useful
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,

    /// don't run any hooks
    #[arg(short = 's', long = "skip-hooks")]
    skip_hooks: bool,

    /// don't run pre request hook
    #[arg(long = "skip-prehook", conflicts_with("skip_hooks"))]
    skip_prehook: bool,

    /// don't run post responnse hook
    #[arg(long = "skip-posthook", conflicts_with("skip_hooks"))]
    skip_posthook: bool,

    /// stop before pre hook and write pre hook data to stdout. Useful for developing pre-hook
    #[arg(long = "inspect-request", conflicts_with_all(["skip_hooks", "skip_prehook"]))]
    inspect_request: bool,

    /// stop before post hook and write post hook data to stdout. Useful for developing post-hook
    #[arg(long = "inspect-response", conflicts_with_all(["skip_hooks", "skip_posthook"]))]
    inspect_response: bool,

    /// output collected services as json output
    #[arg(long("list-json"), conflicts_with("list"))]
    list_json: bool,

    #[arg(required_unless_present_any(["list", "list_json"]))]
    endpoint: Vec<String>,
    /// arguments for hooks, note to make it unamgious add -- before providing any flags
    /// add another -- separator to separate between prehook flags and post hook flags
    #[arg(allow_hyphen_values(true), last(true))]
    args: Vec<String>,
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let args = Arguments::parse();
    let log_level = match args.verbose {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        3 => LevelFilter::TRACE,
        _ => {
            eprintln!(concat!(
                "One of the developer of ",
                env!("CARGO_PKG_NAME"),
                " coming to help debug your code"
            ));
            LevelFilter::TRACE
        }
    };
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
        .init();
    debug!("Log level set to : {log_level:?}");

    debug!(extra_args=?args.args, "Arguments for the scripts");

    let config = parser::Config::open(&args.config_file)?;

    let groups = parser::Group::from_dir(config.api_directory)?;

    debug!(query_set=?groups, "parsed services");

    let query_set = groups
        .find(&args.endpoint)
        .ok_or_else(|| miette::miette!("no such query or group found"))?;

    if args.list || args.list_json {
        debug!(found=?query_set, "found query/group");
        if args.list_json {
            query_set.json_print()?;
        } else {
            query_set.format_print();
        }
    } else {
        let env = match args.environment {
            Some(ref v) => v.clone(),
            None => std::env::var(constants::KEY_CURRENT_ENVIRONMENT)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!(
                        "Couldn't get environment,{} ",
                        constants::KEY_CURRENT_ENVIRONMENT
                    )
                })?,
        };
        let mut config_store = crate::store::Store::with_env(&config.project, env.clone())
            .into_diagnostic()
            .wrap_err_with(|| format!("Couldn't read store values of {}", config.project))?;

        config_store.persistent(!args.no_persistent);

        debug!("current config: {config_store:?}");

        let Some(query_result) = query_set.query else {
            if let Some(name) = query_set.name {
                miette::bail!("{name} is not an query")
            } else {
                miette::bail!("Couldn't find query")
            }
        };

        let mut stdin_buffer = Vec::new();
        let mut stdin = std::io::stdin();
        // if the input is from pipe then consider else, don't wait for input
        let stdin_body = if !stdin.is_terminal() {
            let read_bytes = stdin
                .read_to_end(&mut stdin_buffer)
                .into_diagnostic()
                .wrap_err("Couldn't read stdin")?;
            if read_bytes > 0 {
                Some(&stdin_buffer[..read_bytes])
            } else {
                None
            }
        } else {
            None
        };
        let response_body = query_result
            .exec_with_args(&args, &env, &mut config_store, stdin_body)
            .await?;

        if let Some(body) = response_body {
            if let Some(output_file) = args.output {
                std::fs::write(&output_file, body)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Failed to write response body to {output_file:?}"))?
            } else {
                std::io::stdout()
                    .write_all(&body)
                    .into_diagnostic()
                    .wrap_err("Failed to write body to stdout")?
            }
        }
    }
    Ok(())
}
