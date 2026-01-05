mod cli;
mod daemon;
mod env;
mod http;
mod session;
mod sftp;
mod ssh;
mod ws;

use tokio::runtime::{Builder, Runtime};

fn build_runtime_from_env() -> Runtime {
    let flavor = std::env::var("TOKIO_FLAVOR").unwrap_or_else(|_| "multi_thread".to_string());

    let worker_threads: Option<usize> = std::env::var("TOKIO_WORKER_THREADS")
        .ok()
        .and_then(|v| v.parse().ok());

    let max_blocking_threads: usize = std::env::var("TOKIO_MAX_BLOCKING_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);

    let mut builder = match flavor.as_str() {
        "current_thread" => Builder::new_current_thread(),
        "multi_thread" | "multi" | "" => Builder::new_multi_thread(),
        other => {
            eprintln!("Invalid TOKIO_FLAVOR={other:?}; using multi_thread");
            Builder::new_multi_thread()
        }
    };

    if flavor != "current_thread" {
        if let Some(worker_threads) = worker_threads {
            builder.worker_threads(worker_threads);
        }
    }

    builder
        .max_blocking_threads(max_blocking_threads)
        .enable_all();

    builder.build().expect("failed to build Tokio runtime")
}

fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let rt = build_runtime_from_env();

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
