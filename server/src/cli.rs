use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Start the agent
    Start,
    /// Print version information
    Version,
}

pub(crate) fn parse() -> Cli {
    Cli::parse()
}
