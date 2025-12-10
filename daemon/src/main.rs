mod daemon;
mod env;
mod http;
mod ssh;
mod ws;

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
    /// Start the daemon
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
            phirepass_common::logger::init_logger("phirepass:daemon");
            let config = init_env()?;
            daemon::start(config).await
        }
    }
}
