mod server;

mod connection;
mod env;
mod http;
mod node;
mod state;
mod web;
mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::parse();
    match cli.command {
        Some(cli::Commands::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(cli::Commands::Start) | None => {
            phirepass_common::logger::init("phirepass:server");
            let config = env::init()?;
            server::start(config).await
        }
    }
}
