//! # ojo-watcher
//!
//! CLI for the ojo watcher daemon

use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "ojo-watcher")]
#[command(about = "Watch and transform ojo trace files into queryable format", long_about = None)]
struct Args {
    /// Directory containing trace files to process
    #[arg(long, default_value = "./traces/output")]
    input_dir: PathBuf,

    /// Path to the SQLite database
    #[arg(long, default_value = "./traces.db")]
    db_path: PathBuf,

    /// Continuously watch for new files
    #[arg(long)]
    watch: bool,

    /// Clean up trace files older than this many days
    #[arg(long)]
    cleanup_days: Option<u64>,
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

    info!("Starting ojo-watcher");
    info!("  Input directory: {:?}", args.input_dir);
    info!("  Database path: {:?}", args.db_path);
    info!("  Watch mode: {}", args.watch);

    let config = ojo_watcher::WatcherConfig {
        input_dir: args.input_dir,
        db_path: args.db_path,
        watch: args.watch,
        cleanup_age_secs: args.cleanup_days.map(|d| d * 24 * 60 * 60),
    };

    let watcher = ojo_watcher::Watcher::new(config)?;

    // Process existing files first
    info!("Processing existing trace files...");
    watcher.process_existing_files().await?;

    // If watch mode, continue monitoring
    if args.watch {
        info!("Watching for new trace files...");
        watcher.watch().await?;
    } else {
        info!("Done processing trace files");
    }

    Ok(())
}
