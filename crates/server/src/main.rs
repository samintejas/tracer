#![recursion_limit = "256"] // the tool list in mcp.rs is one big json! literal
mod cli;
mod limit;
mod mcp;
mod oauth;
mod rest;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use cli::{Cli, Cmd};
use tracer_core::{Config, Store};

#[tokio::main]
async fn main() {
    // `tracer tx list | head` should end quietly when the reader goes away, not panic on a broken pipe
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "tracer=info,tower_http=info".into()))
        .init();
    let cli = Cli::parse();
    if let Err(e) = real_main(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn real_main(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let tz: chrono_tz::Tz = cli.tz.parse().map_err(|_| format!("unknown time zone '{}', use a name like Asia/Kolkata or UTC", cli.tz))?;
    let store = Store::open_pool(&cli.db, cli.db_pool).await?.with_config(Config { tz, signups_open: cli.signups == "open" });
    match cli.cmd {
        Cmd::Serve { listen, ui_dir, trust_proxy, public_url, google_client_id, google_client_secret, github_client_id, github_client_secret, job_secs } => {
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            tracing::info!("listening on http://{listen}  (rest /api, mcp /mcp, ui {ui_dir}; time zone {tz}, sign-ups {})", cli.signups);
            if job_secs > 0 {
                tokio::spawn(background(store.clone(), Duration::from_secs(job_secs)));
            }
            let mut providers = HashMap::new();
            if let (Some(id), Some(secret)) = (google_client_id, google_client_secret) {
                providers.insert("google", oauth::Provider::google(id, secret));
            }
            if let (Some(id), Some(secret)) = (github_client_id, github_client_secret) {
                providers.insert("github", oauth::Provider::github(id, secret));
            }
            let oauth = oauth::Oauth::new(&public_url, providers);
            tracing::info!("sign in with: {}", if oauth.enabled().is_empty() { "email and password only".to_string() } else { oauth.enabled().join(", ") });
            let app = rest::router(store, ui_dir.into(), Arc::new(limit::Limits::new(trust_proxy)), Arc::new(oauth));
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown()).await?;
        }
        Cmd::Mcp => {
            let caller = cli::caller(&store, &cli.who).await?;
            mcp::serve_stdio(store, caller).await?;
        }
        _ => cli::run(cli, store).await?,
    }
    Ok(())
}

/// Renewals, reminders and tidying, on a timer. Each step is once-only, so a slow run or two servers do no harm.
async fn background(store: Store, every: Duration) {
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        match store.run_jobs().await {
            Ok(r) if r.renewals + r.reminders > 0 => tracing::info!("jobs: {} renewals posted, {} reminders made, {} rows removed", r.renewals, r.reminders, r.pruned),
            Ok(_) => {}
            Err(e) => tracing::error!("jobs failed: {e}"),
        }
    }
}

/// Finish the requests in flight, then stop, on ctrl-c or the termination signal a service manager sends.
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}
