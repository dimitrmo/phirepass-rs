use log::warn;
use phirepass_common::runtime::RuntimeBuilder;

mod agent;
mod cli;
mod common;
mod creds;
mod env;
mod error;
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
                phirepass_common::logger::init("phirepass:agent");
                let config = env::init()?;
                agent::start(config).await
            }
            Some(cli::Commands::Login(args)) => {
                phirepass_common::logger::init("phirepass:agent");
                if let Err(err) = agent::login(
                    args.server_host,
                    args.server_port,
                    args.from_file,
                    args.from_stdin,
                )
                .await
                {
                    warn!("error login in {}", err)
                }
                Ok(())
            }
            Some(cli::Commands::Logout(args)) => {
                phirepass_common::logger::init("phirepass:agent");
                if let Err(err) = agent::logout(args.server_host, args.server_port).await {
                    warn!("error logging out {}", err);
                }
                Ok(())
            }
            Some(cli::Commands::Version) => {
                println!("{}", env::version());
                Ok(())
            }
        }
    })
}
