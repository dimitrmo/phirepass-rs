mod cli;
mod db;
mod env;
mod proxy;

fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let cli = cli::parse();
    match cli.command {
        Some(cli::Commands::Start) | None => {
            phirepass_common::logger::init("phirepass:relay");
            let config = env::init()?;
            proxy::start(config)
        }
        Some(cli::Commands::Version) => {
            println!("{}", env::version());
            Ok(())
        }
    }
}
