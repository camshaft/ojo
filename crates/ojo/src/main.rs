//! # ojo
//!
//! CLI for the ojo tracer with watch and serve subcommands

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "ojo")]
#[command(about = "Transport protocol event tracer", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Watch trace files and transform them into queryable format (headless mode)
    Watch {
        /// Directory containing trace files to process
        #[arg(long, default_value = "./traces/output")]
        input_dir: PathBuf,

        /// Path to the database
        #[arg(long, default_value = "./traces.db")]
        db_path: PathBuf,

        /// Clean up trace files older than this many days
        #[arg(long)]
        cleanup_days: Option<u64>,
    },

    /// Serve the web interface for exploring traces
    Serve {
        /// Path to the database
        #[arg(long, default_value = "./traces.db")]
        db_path: PathBuf,

        /// Port to bind the web server to
        #[arg(long, short, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
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

    match args.command {
        Commands::Watch {
            input_dir,
            db_path,
            cleanup_days,
        } => {
            info!("Starting ojo watch mode");
            info!("  Input directory: {:?}", input_dir);
            info!("  Database path: {:?}", db_path);

            let config = ojo::WatcherConfig {
                input_dir,
                db_path,
                cleanup_age_secs: cleanup_days.map(|d| d * 24 * 60 * 60),
            };

            let watcher = ojo::Watcher::new(config)?;

            // Process existing files first
            info!("Processing existing trace files...");
            watcher.process_existing_files().await?;

            // Continue watching
            info!("Watching for new trace files...");
            watcher.watch().await?;
        }

        Commands::Serve {
            db_path,
            port,
            host,
        } => {
            info!("Starting ojo serve mode");
            info!("  Database path: {:?}", db_path);
            info!("  Binding to: {}:{}", host, port);

            let config = ojo::ExplorerConfig {
                db_path,
                port,
                host: host.clone(),
            };

            let explorer = ojo::Explorer::new(config)?;

            info!("Server starting at http://{}:{}", host, port);
            explorer.serve().await?;
        }
    }

    Ok(())
}
