mod cli;
mod daemon;
mod env;
mod http;
mod sftp2;
mod ssh;
mod state;
mod ws;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let cli = cli::parse();
    match cli.command {
        Some(cli::Commands::Start) | None => {
            phirepass_common::logger::init("phirepass:daemon");
            let config = env::init()?;
            daemon::start(config).await
        }
        Some(cli::Commands::Version) => {
            println!("{}", env::version());
            Ok(())
        }
    }
}
