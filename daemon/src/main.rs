mod cli;
mod daemon;
mod env;
mod ssh;
mod ws;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::parse();
    match cli.command {
        Some(cli::Commands::Version) => {
            println!("{}", env::version());
            Ok(())
        }
        Some(cli::Commands::Start) | None => {
            phirepass_common::logger::init("phirepass:daemon");
            let config = env::init()?;
            daemon::start(config).await
        }
    }
}
