mod constants;
mod executor;
mod store;

use clap::Parser;
use executor::{parse_and_exec_service, Document};
use tracing::{debug, error, info};
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
    /// list available options (services/endpoints)
    #[arg(short, long)]
    list: bool,
    #[arg(required_unless_present("list"))]
    service: Option<String>,
    #[arg()]
    #[arg(required_unless_present("list"))]
    endpoint: Option<String>,
    /// arguments for hooks, note to make it unamgious add -- before providing any flags
    /// add another -- separator to separate between prehook flags and post hook flags
    #[arg(trailing_var_arg(true), allow_hyphen_values(true))]
    args: Vec<String>,
}

fn main() -> Result<(), anyhow::Error> {
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
    debug!(extra_args=?args.args);
    let document = toml::from_str::<Document>(&std::fs::read_to_string(&args.config_file)?)?;
    info!(file=?args.config_file, "parsed succesfully");
    if args.list {
        // if service name is present then then list end points
        if let Some(service_name) = args.service {
            if let Some(svc) = document.get_service(&service_name) {
                // if endpoint is present then dump the content of endpoint
                if let Some(ep_name) = args.endpoint {
                    if let Some(ep) = svc.get_endpoint(&ep_name) {
                        println!("endpoint: {:#?}", ep);
                        Ok(())
                    } else {
                        error!(service = service_name, "Couldn't find service");
                        Err(anyhow::anyhow!("Failed to list services"))
                    }
                } else {
                    svc.endpoint.iter().for_each(|ep| {
                        println!(
                            "--> name: {}, alias: {}",
                            ep.name,
                            ep.alias.as_deref().unwrap_or("null")
                        )
                    });
                    Ok(())
                }
            } else {
                error!(service = service_name, "Couldn't find service");
                Err(anyhow::anyhow!("Failed to list services"))
            }
        } else {
            // list services since service name is not present
            document
                .services
                .iter()
                .for_each(|svc| println!("-> name: {}, alias: {}", svc.name, svc.alias.as_deref().unwrap_or("null")));
            Ok(())
        }
    } else {
        let Some(service_name) = args.service else {
            error!("service is required field, unless listing");
            return Err(anyhow::anyhow!("missing param"));
        };
        let Some(endpoint_name) = args.endpoint else {
            error!("service is required field, unless listing");
            return Err(anyhow::anyhow!("missing param"));
        };
        let flags = args.args.iter().map(|arg| arg.as_str()).collect::<Vec<_>>();
        parse_and_exec_service(&document, &service_name, &endpoint_name, flags.as_slice())
    }
}
