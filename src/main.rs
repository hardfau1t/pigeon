mod constants;
mod registry;
mod store;

use std::io::Write;

use clap::Parser;
use color_eyre::eyre::Context;
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;

use registry::Bundle;

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

    /// output collected services as json output
    #[arg(short, long, conflicts_with_all(["list", "endpoint"]))]
    json: bool,

    #[arg(required_unless_present_any(["list", "json"]))]
    endpoint: Vec<String>,
    /// arguments for hooks, note to make it unamgious add -- before providing any flags
    /// add another -- separator to separate between prehook flags and post hook flags
    #[arg(allow_hyphen_values(true), last(true))]
    args: Vec<String>,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
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

    let services = Bundle::open(&args.config_file)?;
    debug!(services=?services, "parsed services");

    if args.list {
        services.view(&args.endpoint);
    } else if args.json {
        let stdout = std::io::stdout();
        serde_json::to_writer(stdout, &services)?;
    } else {
        let response_body = services.run(
            &args.endpoint,
            &args.args,
            !args.no_persistent,
            args.dry_run,
            args.skip_hooks || args.skip_prehook,
            args.skip_hooks || args.skip_posthook,
            args.environment.as_deref(),
        )?;
        if let Some(body) = response_body {
            if let Some(output_file) = args.output {
                std::fs::write(&output_file, body)
                    .wrap_err_with(|| format!("Failed to write response body to {output_file:?}"))?
            } else {
                std::io::stdout()
                    .write_all(&body)
                    .wrap_err("Failed to write body to stdout")?
            }
        }
    }
    Ok(())
}
