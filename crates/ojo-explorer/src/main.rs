//! # ojo-explorer
//!
//! CLI for the ojo explorer web server

use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "ojo-explorer")]
#[command(about = "Web-based explorer for ojo trace data", long_about = None)]
struct Args {
    /// Path to the SQLite database
    #[arg(long, default_value = "./traces.db")]
    db_path: PathBuf,

    /// Port to bind the web server to
    #[arg(long, short, default_value = "8080")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    info!("Starting ojo-explorer");
    info!("  Database path: {:?}", args.db_path);
    info!("  Binding to: {}:{}", args.host, args.port);

    let config = ojo_explorer::ExplorerConfig {
        db_path: args.db_path,
        port: args.port,
        host: args.host.clone(),
    };

    let explorer = ojo_explorer::Explorer::new(config)?;

    info!("Server starting at http://{}:{}", args.host, args.port);
    explorer.serve().await?;

    Ok(())
}
