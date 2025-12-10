use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Print version information
    Version,
    /// Start the daemon
    Start,
}

pub(crate) fn parse() -> Cli {
    Cli::parse()
}
