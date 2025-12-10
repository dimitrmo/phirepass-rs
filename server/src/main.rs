mod server;

mod connection;
mod env;
mod http;
mod node;
mod state;
mod web;

use crate::env::init_env;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print version information
    Version,
    /// Start the server
    Start,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(Commands::Start) | None => {
            phirepass_common::logger::init_logger("phirepass:server");
            let config = init_env()?;
            server::start(config).await
        }
    }
}
