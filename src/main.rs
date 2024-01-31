use clap::Parser;
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, clap::Parser)]
#[command(author, version, about)]
/// make rest queries, automate
struct Arguments {
    #[arg(short, long, global=true, action=clap::ArgAction::Count)]
    verbose: u8,
    /// prefix directory, where all the data is stored
    /// default = ~/.local/share/pigeon
    #[arg(short, long)]
    prefix: Option<std::path::PathBuf>,
    #[command(subcommand)]
    function: Functionalities,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Functionalities {
    /// rest api's
    Rest(pigeon::rest::Arguments),
    /// sql queries
    Sql,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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
    let mut local_dir = args.prefix.or_else(dirs::data_local_dir).unwrap();
    local_dir.push(env!("CARGO_PKG_NAME"));
    match args.function {
        Functionalities::Rest(rest_args) => pigeon::rest::handler(&rest_args, &local_dir).await,
        Functionalities::Sql => todo!(),
    }
}
