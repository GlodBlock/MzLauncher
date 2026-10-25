use std::path::PathBuf;

use mzlauncher_core::{server::Server, CORE_VERSION};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut args = std::env::args_os().skip(1);
    let data_dir = match (args.next(), args.next()) {
        (Some(flag), Some(path)) if flag == "--data-dir" => Some(PathBuf::from(path)),
        (None, None) => None,
        (Some(flag), None) if flag == "--version" => {
            eprintln!("mzlauncher-core {CORE_VERSION}");
            return Ok(());
        }
        _ => {
            eprintln!("usage: mzlauncher-core [--data-dir <path>] | [--version]");
            std::process::exit(2);
        }
    };

    if args.next().is_some() {
        eprintln!("usage: mzlauncher-core [--data-dir <path>] | [--version]");
        std::process::exit(2);
    }

    Server::new(data_dir)?.run().await?;
    Ok(())
}
