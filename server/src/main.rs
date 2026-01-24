use phirepass_common::runtime::RuntimeBuilder;

mod cli;
mod connection;
mod db;
mod env;
mod error;
mod http;
mod node;
mod server;
mod web;

fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let rt = RuntimeBuilder::create().build()?;

    rt.block_on(async {
        let cli = cli::parse();
        match cli.command {
            Some(cli::Commands::Start) | None => {
                phirepass_common::logger::init("phirepass:server");
                let config = env::init()?;
                server::start(config).await
            }
            Some(cli::Commands::Version) => {
                println!("{}", env::version());
                Ok(())
            }
        }
    })
}
