mod constants;
mod registry;
mod store;

use clap::Parser;
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
    /// list available options (services/endpoints)
    #[arg(short, long)]
    list: bool,
    #[arg(required_unless_present("list"))]
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
    } else {
        services.run(&args.endpoint, &args.args)?;
    }
    Ok(())
}
