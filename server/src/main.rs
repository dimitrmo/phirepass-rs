mod server;

mod cli;
mod connection;
mod env;
mod http;
mod node;
mod state;
mod web;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::parse();
    match cli.command {
        Some(cli::Commands::Version) => {
            println!("{}", env::version());
            Ok(())
        }
        Some(cli::Commands::Start) | None => {
            phirepass_common::logger::init("phirepass:server");
            let config = env::init()?;
            server::start(config).await
        }
    }
}
