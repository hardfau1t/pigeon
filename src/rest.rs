use tracing::debug;

#[derive(clap::Args, Debug, Clone)]
#[command()]
pub struct Arguments {
    #[command(subcommand)]
    action: Actions,
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum Actions {
    /// execute a rest api call
    Exec,
    /// Create a new api
    Insert,
    /// view given api details
    View,
    /// list all the available api's
    List,
}

pub fn handler(args: &Arguments) -> Result<(), anyhow::Error> {
    debug!("args {args:?}");
    Ok(())
}
