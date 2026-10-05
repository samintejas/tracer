mod cli;
mod mcp;
mod rest;

use clap::Parser;
use cli::{Cli, Cmd};
use tracer_core::Store;

#[tokio::main]
async fn main() {
    // `tracer tx list | head` should end quietly when the reader goes away, not panic on a broken pipe
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "tracer=info".into())).init();
    let cli = Cli::parse();
    if let Err(e) = real_main(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn real_main(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(&cli.db).await?;
    match cli.cmd {
        Cmd::Serve { listen, ui_dir } => {
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            tracing::info!("listening on http://{listen}  (rest /api, mcp /mcp, ui {ui_dir})");
            axum::serve(listener, rest::router(store, ui_dir.into())).await?;
        }
        Cmd::Mcp => {
            let caller = cli::caller(&store, &cli.who).await?;
            mcp::serve_stdio(store, caller).await?;
        }
        _ => cli::run(cli, store).await?,
    }
    Ok(())
}
