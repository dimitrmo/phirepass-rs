use phirepass_common::runtime::RuntimeBuilder;

mod cli;
mod common;
mod daemon;
mod env;
mod http;
mod session;
mod sftp;
mod ssh;
mod ws;

fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let rt = RuntimeBuilder::create().with_worker_threads(2).build()?;

    rt.block_on(async {
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
    })
}
